//! Drawing an Ecotope's stones through GPU pools (feature `gpu`): each
//! stone class becomes one [`StonePool`] whose variants are the class's
//! shapes, generated on the GPU, and each stone one [`PooledStone`].

use bevy::prelude::*;
use procgena_biome::ScatterCopies;
use procgena_stone_bevy::{PooledStone, StonePool};

/// One class of a drawn stone Scatter: the pool that draws its shapes, and
/// each stone's variant and place, relative to this entity.
#[derive(Component, Debug, Clone)]
#[require(Transform, Visibility)]
pub(crate) struct PooledClass {
    pool: StonePool,
    stones: Vec<(u32, Transform)>,
}

/// One [`PooledClass`] per class of the placed stones `copies`, in class
/// order; none for copies of another kind.
///
/// Each pool is the class's stone settings, variant count and stretch
/// jitter in the Scatter's World, so its variants are the shapes the
/// Scatter's own `variant` gives.
pub(crate) fn pooled_classes(copies: &ScatterCopies) -> Vec<PooledClass> {
    let ScatterCopies::Stone(stones) = copies else {
        return Vec::new();
    };
    let classes = &stones.options().classes;
    let mut pooled: Vec<PooledClass> = classes
        .iter()
        .map(|class| PooledClass {
            pool: StonePool {
                options: class.stone.clone(),
                variants: class.variants,
                stretch_jitter: class.stretch_jitter,
                world_seed: stones.world_seed(),
            },
            stones: Vec::new(),
        })
        .collect();
    for stone in stones.instances() {
        let transform = Transform::from_matrix(Mat4::from(stone.transform));
        pooled[stone.class].stones.push((stone.variant, transform));
    }
    pooled
}

/// The pool assets drawing the classes on screen, each kept while some
/// class with equal settings is drawn, so a redrawn Scatter reuses its
/// generated shapes.
#[derive(Resource, Default)]
pub(crate) struct ClassPools(Vec<(StonePool, Handle<StonePool>)>);

/// Gives each new [`PooledClass`] its pool asset, reusing an equal one, and
/// a [`PooledStone`] child per stone. Forgets pools no class draws any
/// more. Does nothing until the stone plugin's pools are set up.
pub(crate) fn spawn_pooled_stones(
    mut commands: Commands,
    added: Query<(Entity, &PooledClass), Added<PooledClass>>,
    drawn: Query<&PooledClass>,
    mut removed: RemovedComponents<PooledClass>,
    assets: Option<ResMut<Assets<StonePool>>>,
    mut pools: ResMut<ClassPools>,
) {
    let Some(mut assets) = assets else {
        return;
    };

    // Each new class's pool and stones.
    for (entity, class) in &added {
        let handle = match pools.0.iter().find(|(pool, _)| *pool == class.pool) {
            Some((_, handle)) => handle.clone(),
            None => {
                let handle = assets.add(class.pool.clone());
                pools.0.push((class.pool.clone(), handle.clone()));
                handle
            }
        };
        for (variant, transform) in &class.stones {
            commands.spawn((
                PooledStone {
                    pool: handle.clone(),
                    variant: *variant,
                },
                *transform,
                ChildOf(entity),
            ));
        }
    }

    // Drop the pools no class on screen draws.
    if removed.read().count() > 0 {
        pools
            .0
            .retain(|(pool, _)| drawn.iter().any(|class| class.pool == *pool));
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::DVec2;
    use procgena_biome::{EcotopePreset, ScatterCopies};
    use procgena_core::FlatGround;
    use procgena_core::scatter::{ScatterArea, ScatterInput};

    use super::*;

    #[test]
    fn each_class_pool_draws_the_shapes_its_scatter_placed() {
        let ecotope = EcotopePreset::pine_forest_floor().ecotope.clone();
        let input = ScatterInput::new(
            &FlatGround,
            ScatterArea::new(DVec2::ZERO, Vec2::splat(30.0)),
            7,
        );
        let placed = ecotope.place(&input).unwrap();
        let copies = placed.get("boulders").unwrap().copies();
        let ScatterCopies::Stone(stones) = copies else {
            panic!("boulders are stones");
        };
        let classes = pooled_classes(copies);
        assert_eq!(classes.len(), stones.options().classes.len());

        // The same shapes as the Scatter's own variants.
        for (index, class) in classes.iter().enumerate() {
            for variant in 0..class.pool.variants {
                assert_eq!(
                    class.pool.variant_options(variant),
                    *stones.variant(index, variant),
                    "class {index} variant {variant}"
                );
            }
        }

        // Every stone, once, in its own class.
        let count: usize = classes.iter().map(|class| class.stones.len()).sum();
        assert_eq!(count, stones.instances().len());
        assert!(count > 0);
    }
}
