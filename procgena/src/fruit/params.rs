//! The fruit and flower settings shown in the Fruit and Flower tabs, with
//! their ranges.

use std::sync::LazyLock;

use procgena_fruit::{
    ClusterKind, ClusterShape, Facing, FlowerShape, FruitOptions, FruitShape, ItemShape, Variation,
};

use crate::params::{Group, Section, choice, color, count, number, toggle};

/// The fruit shape shown while a cluster holds flowers.
static NO_FRUIT: LazyLock<FruitShape> = LazyLock::new(FruitShape::default);
/// The flower shape shown while a cluster holds fruit.
static NO_FLOWER: LazyLock<FlowerShape> = LazyLock::new(FlowerShape::default);

/// The cluster's fruit shape, or the default one if it holds flowers.
fn fruit_shape(f: &FruitOptions) -> &FruitShape {
    match &f.item {
        ItemShape::Fruit(shape) => shape,
        ItemShape::Flower(_) => &NO_FRUIT,
    }
}

/// The cluster's fruit shape, turning its items into fruit if they are flowers.
fn fruit_shape_mut(f: &mut FruitOptions) -> &mut FruitShape {
    if !f.item.is_fruit() {
        f.item = ItemShape::Fruit(FruitShape::default());
    }
    match &mut f.item {
        ItemShape::Fruit(shape) => shape,
        ItemShape::Flower(_) => unreachable!("the items were just made fruit"),
    }
}

/// The cluster's flower shape, or the default one if it holds fruit.
fn flower_shape(f: &FruitOptions) -> &FlowerShape {
    match &f.item {
        ItemShape::Flower(shape) => shape,
        ItemShape::Fruit(_) => &NO_FLOWER,
    }
}

/// The cluster's flower shape, turning its items into flowers if they are fruit.
fn flower_shape_mut(f: &mut FruitOptions) -> &mut FlowerShape {
    if f.item.is_fruit() {
        f.item = ItemShape::Flower(FlowerShape::default());
    }
    match &mut f.item {
        ItemShape::Flower(shape) => shape,
        ItemShape::Fruit(_) => unreachable!("the items were just made flowers"),
    }
}

/// Every section of the Fruit tab (`fruit` true) or the Flower tab, in
/// display order. The seed and preset are edited separately.
pub fn sections(fruit: bool) -> Vec<Section<FruitOptions>> {
    let item = if fruit { fruit_item() } else { flower_item() };
    vec![
        Section {
            title: "Cluster",
            groups: vec![
                whole(fruit),
                cluster().lift("cluster", |f: &FruitOptions| &f.cluster, |f| &mut f.cluster),
            ],
        },
        Section {
            title: if fruit { "Fruit" } else { "Flower" },
            groups: item,
        },
        Section {
            title: "Variation",
            groups: vec![variation().lift(
                "variation",
                |f: &FruitOptions| &f.variation,
                |f| &mut f.variation,
            )],
        },
    ]
}

/// Facing, size and ripeness or bloom.
fn whole(fruit: bool) -> Group<FruitOptions> {
    Group::plain(vec![
        choice!(FruitOptions, "facing", "Facing", [
            "Hang" => Facing::Hang,
            "Up" => Facing::Up,
            "Outward" => Facing::Outward,
        ], |f| f.facing),
        number!(FruitOptions, "size", "Size", (0.005, 0.5, 0.005), |f| f
            .size)
        .exponential()
        .revive(0.1),
        number!(
            FruitOptions,
            "size_variance",
            "Size Variance",
            (0.0, 1.0, 0.01),
            |f| f.size_variance
        ),
        number!(
            FruitOptions,
            "maturity",
            if fruit { "Ripeness" } else { "Bloom" },
            (0.0, 1.0, 0.01),
            |f| f.maturity
        ),
    ])
}

/// Whether a cluster has stalks or a main stem to draw.
fn has_stalks(c: &ClusterShape) -> bool {
    !matches!(c.kind, ClusterKind::Single | ClusterKind::Umbel) || c.stalk_length > 0.0
}

/// How items are arranged within a cluster, and its stalks.
fn cluster() -> Group<ClusterShape> {
    use ClusterKind::{Catkin, Panicle, Raceme, Single};
    let several = |c: &ClusterShape| c.kind != Single;
    let main_stem = |c: &ClusterShape| matches!(c.kind, Raceme | Panicle | Catkin);
    let side_stems = |c: &ClusterShape| c.kind == Panicle;
    Group::titled(
        "Arrangement",
        vec![
            choice!(ClusterShape, "arrangement", "Kind", [
                "Single" => ClusterKind::Single,
                "Umbel" => ClusterKind::Umbel,
                "Raceme" => ClusterKind::Raceme,
                "Panicle" => ClusterKind::Panicle,
                "Catkin" => ClusterKind::Catkin,
            ], |c| c.kind),
            count!(ClusterShape, "items", "Items", (1, 60), |c| c.items)
                .shown_if(several)
                .revive(6.0),
            number!(
                ClusterShape,
                "items_variance",
                "Items Variance",
                (0.0, 1.0, 0.01),
                |c| c.items_variance
            )
            .shown_if(several),
            number!(ClusterShape, "spread", "Spread", (0.0, 90.0, 1.0), |c| c
                .spread)
            .shown_if(several)
            .revive(30.0),
            number!(
                ClusterShape,
                "axis_length",
                "Axis Length",
                (0.0, 1.0, 0.005),
                |c| c.axis_length
            )
            .shown_if(main_stem)
            .revive(0.2),
            number!(ClusterShape, "droop", "Droop", (0.0, 1.0, 0.01), |c| c
                .droop)
            .shown_if(main_stem),
            number!(ClusterShape, "taper", "Taper", (0.0, 1.0, 0.01), |c| c
                .taper)
            .shown_if(main_stem),
            count!(ClusterShape, "side_stalks", "Side Stalks", (1, 12), |c| c
                .side_stalks)
            .shown_if(side_stems),
            number!(
                ClusterShape,
                "side_length",
                "Side Length",
                (0.0, 2.0, 0.01),
                |c| c.side_length
            )
            .shown_if(side_stems)
            .revive(0.4),
            number!(
                ClusterShape,
                "stalk_length",
                "Stalk Length",
                (0.0, 0.3, 0.001),
                |c| c.stalk_length
            )
            .exponential(),
            number!(
                ClusterShape,
                "stalk_radius",
                "Stalk Radius",
                (0.0, 0.02, 0.0005),
                |c| c.stalk_radius
            )
            .shown_if(has_stalks)
            .exponential()
            .revive(0.003),
            color!(ClusterShape, "stalk_color", "Stalk Colour", |c| c
                .stalk_color)
            .shown_if(has_stalks),
        ],
    )
}

/// A fruit's shape, stem and colours.
fn fruit_item() -> Vec<Group<FruitOptions>> {
    let shape = Group::titled(
        "Shape",
        vec![
            number!(
                FruitShape,
                "elongation",
                "Elongation",
                (0.2, 3.0, 0.01),
                |s| s.elongation
            ),
            number!(
                FruitShape,
                "top_dimple",
                "Top Dimple",
                (0.0, 1.0, 0.01),
                |s| s.top_dimple
            ),
            number!(
                FruitShape,
                "bottom_taper",
                "Bottom Taper",
                (0.0, 1.0, 0.01),
                |s| s.bottom_taper
            ),
            count!(FruitShape, "lobes", "Lobes", (0, 12), |s| s.lobes),
            number!(
                FruitShape,
                "lobe_depth",
                "Lobe Depth",
                (0.0, 0.95, 0.01),
                |s| s.lobe_depth
            )
            .shown_if(|s| s.lobes > 0)
            .revive(0.3),
            count!(FruitShape, "segments", "Segments", (3, 32), |s| s.segments),
            count!(FruitShape, "rings", "Rings", (2, 24), |s| s.rings),
            toggle!(FruitShape, "calyx", "Calyx", |s| s.calyx),
        ],
    );
    let stem = Group::titled(
        "Stem",
        vec![
            number!(
                FruitShape,
                "stem_length",
                "Stem Length",
                (0.0, 1.0, 0.01),
                |s| s.stem_length
            ),
            number!(
                FruitShape,
                "stem_radius",
                "Stem Radius",
                (0.0, 0.2, 0.005),
                |s| s.stem_radius
            )
            .shown_if(|s| s.stem_length > 0.0)
            .exponential()
            .revive(0.03),
            number!(
                FruitShape,
                "stem_curve",
                "Stem Curve",
                (0.0, 1.0, 0.01),
                |s| s.stem_curve
            )
            .shown_if(|s| s.stem_length > 0.0),
        ],
    );
    let colors = Group::titled(
        "Colour",
        vec![
            color!(FruitShape, "color", "Ripe", |s| s.color),
            color!(FruitShape, "unripe_color", "Unripe", |s| s.unripe_color),
            number!(
                FruitShape,
                "blush_amount",
                "Blush Amount",
                (0.0, 1.0, 0.01),
                |s| s.blush_amount
            ),
            color!(FruitShape, "blush_color", "Blush", |s| s.blush_color)
                .shown_if(|s| s.blush_amount > 0.0),
        ],
    );
    // Catkin fruit have no stems of their own.
    let catkin = |f: &FruitOptions| f.cluster.kind == ClusterKind::Catkin;
    vec![
        shape.lift("item", fruit_shape, fruit_shape_mut),
        stem.lift("item", fruit_shape, fruit_shape_mut)
            .shown_if(move |f| !catkin(f)),
        colors.lift("item", fruit_shape, fruit_shape_mut),
    ]
}

/// A flower's petals, centre and colours. Catkin flowers are rounded bumps
/// that use only the petal, bud and faded colours.
fn flower_item() -> Vec<Group<FruitOptions>> {
    let shape = Group::titled(
        "Petals",
        vec![
            count!(FlowerShape, "petal_count", "Petals", (1, 20), |s| s
                .petal_count),
            count!(FlowerShape, "rings", "Rings", (1, 6), |s| s.rings),
            number!(
                FlowerShape,
                "petal_length",
                "Petal Length",
                (0.0, 2.0, 0.01),
                |s| s.petal_length
            )
            .revive(0.5),
            number!(
                FlowerShape,
                "petal_width",
                "Petal Width",
                (0.0, 1.5, 0.01),
                |s| s.petal_width
            )
            .revive(0.35),
            number!(FlowerShape, "cup", "Cup", (0.0, 1.0, 0.01), |s| s.cup),
            number!(FlowerShape, "curl", "Curl", (0.0, 1.0, 0.01), |s| s.curl),
            number!(FlowerShape, "notch", "Notch", (0.0, 1.0, 0.01), |s| s.notch),
            count!(
                FlowerShape,
                "petal_segments",
                "Petal Segments",
                (1, 12),
                |s| s.petal_segments
            ),
            number!(
                FlowerShape,
                "center_size",
                "Centre Size",
                (0.0, 1.0, 0.01),
                |s| s.center_size
            ),
        ],
    );
    let colors = Group::titled(
        "Colour",
        vec![
            color!(FlowerShape, "petal_color", "Petal", |s| s.petal_color),
            color!(FlowerShape, "base_color", "Petal Base", |s| s.base_color),
            color!(FlowerShape, "center_color", "Centre", |s| s.center_color)
                .shown_if(|s| s.center_size > 0.0),
            color!(FlowerShape, "bud_color", "Bud", |s| s.bud_color),
            color!(FlowerShape, "fade_color", "Faded", |s| s.fade_color),
        ],
    );
    let catkin = |f: &FruitOptions| f.cluster.kind == ClusterKind::Catkin;
    vec![
        shape
            .lift("item", flower_shape, flower_shape_mut)
            .shown_if(move |f| !catkin(f)),
        colors.lift("item", flower_shape, flower_shape_mut),
    ]
}

/// How the items of one cluster differ, so the specimens of a pool differ.
fn variation() -> Group<Variation> {
    Group::plain(vec![
        number!(
            Variation,
            "maturity_spread",
            "Maturity Spread",
            (0.0, 1.0, 0.01),
            |v| v.maturity_spread
        ),
        number!(
            Variation,
            "hue_spread",
            "Hue Spread",
            (0.0, 1.0, 0.01),
            |v| v.hue_spread
        ),
        number!(
            Variation,
            "blush_spread",
            "Blush Spread",
            (0.0, 1.0, 0.01),
            |v| v.blush_spread
        ),
        number!(
            Variation,
            "shape_jitter",
            "Shape Jitter",
            (0.0, 1.0, 0.01),
            |v| v.shape_jitter
        ),
        number!(
            Variation,
            "stem_curve_jitter",
            "Stem Curve Jitter",
            (0.0, 1.0, 0.01),
            |v| v.stem_curve_jitter
        ),
    ])
}

#[cfg(test)]
mod tests {
    use procgena_fruit::FruitPreset;

    use super::*;

    /// Every setting's key names a real field of the settings file.
    #[test]
    fn every_key_names_a_setting() {
        for (fruit, preset) in [
            (true, FruitPreset::apple()),
            (false, FruitPreset::dogwood_bloom()),
        ] {
            let value = serde_json::to_value(&preset.fruit).unwrap();
            for section in sections(fruit) {
                for group in &section.groups {
                    for param in &group.params {
                        let mut at = &value;
                        for step in param.key.split('.') {
                            at = &at[step];
                        }
                        assert!(!at.is_null(), "{} has no setting {}", preset.key, param.key);
                    }
                }
            }
        }
    }

    /// Editing a fruit setting keeps fruit, and a flower setting keeps flowers.
    #[test]
    fn edits_keep_the_items_kind() {
        let mut apple = FruitPreset::apple().fruit.clone();
        fruit_shape_mut(&mut apple).lobes = 3;
        assert!(apple.item.is_fruit());
        let mut bloom = FruitPreset::dogwood_bloom().fruit.clone();
        flower_shape_mut(&mut bloom).cup = 0.5;
        assert!(!bloom.item.is_fruit());
        assert_eq!(fruit_shape(&bloom), &FruitShape::default());
    }
}
