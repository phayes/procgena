use std::sync::Arc;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::shader::{Shader, ShaderLoader};
use procgena_core::chain::{DistanceChain, Step};
use procgena_core::{GeneratorKind, OptionsHash, PoolId, SpecimenId};

use super::*;

/// The id of slot `slot` of a test pool.
fn specimen(slot: u32) -> SpecimenId {
    let kind = GeneratorKind::from_key("procgena_core_bevy.test.kind");
    SpecimenId::new(7, PoolId::new(kind, OptionsHash(2)), slot)
}

/// An app with the plugin and no renderer.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Shader>()
        .init_asset_loader::<ShaderLoader>()
        .init_asset::<Mesh>()
        .add_plugins((InstancedPlugin, InstancedPlugin));
    app
}

/// A slot of `specimen` with no parts.
fn slot(specimen: SpecimenId) -> InstancedSlot {
    InstancedSlot {
        specimen,
        piece: 0,
        chain: DistanceChain::DEFAULT,
        cull: true,
        impostor: false,
        parts: Vec::new(),
        bounds: (Vec3::ZERO, 1.0),
    }
}

#[test]
fn the_cache_keeps_used_specimens_and_drops_the_rest() {
    let mut app = app();
    {
        let mut cache = app.world_mut().resource_mut::<SpecimenCache>();
        cache.insert(specimen(0), 0, Step::Game, Vec::new());
        cache.insert(specimen(0), 0, Step::Lower, Vec::new());
        cache.insert(specimen(1), 0, Step::Game, Vec::new());
    }
    app.world_mut().spawn(InstancedScatter {
        slots: Arc::from([slot(specimen(0))]),
        copies: Arc::from([]),
    });

    // Unused entries survive a short while, so rebuilt Scatters find them.
    app.update();
    assert_eq!(app.world().resource::<SpecimenCache>().len(), 3);
    for _ in 0..=PRUNE_FRAMES {
        app.update();
    }
    let cache = app.world().resource::<SpecimenCache>();
    assert_eq!(cache.len(), 2);
    let meshes = [Step::Game, Step::Lower];
    assert!(cache.get_steps(specimen(0), 0, meshes).is_some());
    assert!(cache.get(specimen(1), 0, Step::Game).is_none());
    assert!(cache.get_steps(specimen(0), 0, Step::ALL).is_none());
    assert!(cache.get_steps(specimen(0), 1, [Step::Game]).is_none());
}
