//! The tree settings shown in the Trees tab, with their ranges.

use procgena_tree::{
    BARK_KINDS, BarkOptions, BarkUvMode, Billboard, BranchOptions, ClusterKind, ClusterPlacement,
    ClusterShape, Facing, FlowerOptions, FlowerShape, FruitOptions, FruitShape, LEAF_KINDS,
    LeafOptions, TreeOptions, TreeType, TrellisOptions,
};

use crate::params::{Group, Param, Section, choice, color, count, name_choice, number, toggle};

/// Every tree settings section, in display order. The seed and preset are
/// edited separately.
pub fn sections() -> Vec<Section<TreeOptions>> {
    vec![
        Section {
            title: "Bark",
            groups: bark()
                .into_iter()
                .map(|group| lift(group, "bark", |o| &o.bark, |o| &mut o.bark))
                .collect(),
        },
        Section {
            title: "Branches",
            groups: branches(),
        },
        Section {
            title: "Leaves",
            groups: leaves(),
        },
        Section {
            title: "Fruit",
            groups: fruit(),
        },
        Section {
            title: "Flowers",
            groups: flowers(),
        },
        Section {
            title: "Trellis",
            groups: trellis(),
        },
    ]
}

/// Lifts a group of `S` settings into tree settings under `prefix`.
fn lift<S: 'static>(
    group: Group<S>,
    prefix: &str,
    get: fn(&TreeOptions) -> &S,
    get_mut: fn(&mut TreeOptions) -> &mut S,
) -> Group<TreeOptions> {
    group.lift(prefix, get, get_mut)
}

fn bark() -> Vec<Group<BarkOptions>> {
    let look = Group::plain(vec![
        color!(BarkOptions, "tint", "Tint", |b| b.tint),
        toggle!(BarkOptions, "flatShading", "Flat Shading", |b| b
            .flat_shading),
        toggle!(BarkOptions, "textured", "Textured", |b| b.textured),
    ]);
    let texture = Group::plain(vec![
        name_choice!(BarkOptions, "type", "Type", BARK_KINDS, |b| b.kind),
        number!(
            BarkOptions,
            "textureScale.x",
            "Texture Scale X",
            (5.0, 200.0, 1.0),
            |b| b.texture_scale.x
        )
        .exponential(),
        number!(
            BarkOptions,
            "textureScale.y",
            "Texture Scale Y",
            (0.5, 20.0, 0.1),
            |b| b.texture_scale.y
        )
        .exponential(),
        choice!(BarkOptions, "uvMode", "UV Mode", [
            "Mirrored" => BarkUvMode::Mirrored,
            "Continuous" => BarkUvMode::Continuous,
        ], |b| b.uv_mode),
    ])
    .shown_if(|b| b.textured);
    vec![look, texture]
}

/// Whether any branch grows at `level`. The trunk always does. A deeper level
/// grows from the one before it, through side branches or, on deciduous trees,
/// the continuation at each tip.
fn grows(o: &TreeOptions, level: usize) -> bool {
    (1..=level).all(|l| {
        l as u32 <= o.branch.levels
            && (o.tree_type == TreeType::Deciduous || o.branch.children[l - 1] > 0)
    })
}

/// Whether the trunk (level 0) or side branches at `level` grow. Only these use
/// the level's angle, start, radius, sections and segments; tip continuations
/// copy them from their parent.
fn has_side_branches(o: &TreeOptions, level: usize) -> bool {
    level == 0
        || (grows(o, level - 1)
            && level as u32 <= o.branch.levels
            && o.branch.children[level - 1] > 0)
}

/// Whether branches at `level` grow side branches of their own.
fn has_children(o: &TreeOptions, level: usize) -> bool {
    (level as u32) < o.branch.levels && grows(o, level)
}

/// The number setting for `field[level]` of [`BranchOptions`], labelled
/// `label`, shown while `shown(options, level)` holds.
macro_rules! level_number {
    ($field:ident, $level:expr, $label:expr, $range:expr, $shown:expr) => {{
        let level: usize = $level;
        let shown: fn(&TreeOptions, usize) -> bool = $shown;
        Param::number(
            format!("branch.{}.{level}", stringify!($field)),
            $label,
            $range,
            move |o: &TreeOptions| o.branch.$field[level],
            move |o: &mut TreeOptions, value| o.branch.$field[level] = value,
        )
        .shown_if(move |o| shown(o, level))
    }};
}

/// The whole-number setting for `field[level]` of [`BranchOptions`],
/// labelled `label`, shown while `shown(options, level)` holds.
macro_rules! level_count {
    ($field:ident, $level:expr, $label:expr, $range:expr, $shown:expr) => {{
        let level: usize = $level;
        let shown: fn(&TreeOptions, usize) -> bool = $shown;
        Param::count(
            format!("branch.{}.{level}", stringify!($field)),
            $label,
            $range,
            move |o: &TreeOptions| o.branch.$field[level],
            move |o: &mut TreeOptions, value| o.branch.$field[level] = value,
        )
        .shown_if(move |o| shown(o, level))
    }};
}

/// Sub-headings of the branch level groups, indexed by level.
const LEVEL_TITLES: [&str; 4] = ["Trunk", "Level 1", "Level 2", "Level 3"];

/// Length given to a branch level that appears with none, as a share of its
/// parent level's length.
const REVIVED_LENGTH_SHARE: f32 = 0.6;

/// Shortest length given to a branch level that appears with none, in metres.
const REVIVED_LENGTH_MIN: f32 = 0.3;

/// The settings of branch `level`, under its own sub-heading. The child
/// count comes last, since it decides whether the next level grows.
///
/// A deeper level that appears with no length, taper, angle or children gets
/// values based on its parent level, so it can be seen at once.
fn level_group(level: usize) -> Group<TreeOptions> {
    let deciduous = |o: &TreeOptions, level| o.tree_type == TreeType::Deciduous && grows(o, level);
    let parent = level.saturating_sub(1);
    let mut length = level_number!(length, level, "Length", (0.0, 10.0, 0.01), grows).exponential();
    if level > 0 {
        length = length.revive_with(move |o| {
            (o.branch.length[parent] * REVIVED_LENGTH_SHARE).max(REVIVED_LENGTH_MIN)
        });
    }
    let mut params = vec![length];

    // The trunk radius is in metres; deeper levels multiply the parent's radius.
    params.push(if level == 0 {
        level_number!(radius, 0, "Radius", (0.01, 0.5, 0.001), has_side_branches).exponential()
    } else {
        level_number!(
            radius,
            level,
            "Radius ×",
            (0.1, 5.0, 0.01),
            has_side_branches
        )
        .exponential()
        .revive(0.7)
    });

    // Side branches leave their parent at an angle, from a start position.
    if level > 0 {
        params.extend([
            level_number!(angle, level, "Angle", (0.0, 180.0, 1.0), has_side_branches).revive(45.0),
            level_number!(
                start,
                level,
                "Start Position",
                (0.0, 1.0, 0.01),
                has_side_branches
            ),
        ]);
    }

    // Shape along the branch; evergreen branches always taper to a point.
    let mut taper = level_number!(taper, level, "Taper", (0.0, 1.0, 0.01), deciduous);
    if level > 0 {
        taper = taper.revive_with(move |o| match o.branch.taper[parent] {
            0.0 => 0.7,
            inherited => inherited,
        });
    }
    params.extend([
        level_number!(gnarliness, level, "Gnarliness", (-0.5, 0.5, 0.01), grows),
        level_number!(twist, level, "Twist", (-0.5, 0.5, 0.01), grows),
        taper,
        level_count!(sections, level, "Sections", (1, 20), has_side_branches),
        level_count!(segments, level, "Segments", (3, 16), has_side_branches),
    ]);

    // Child counts per level; the deepest level has none. The trunk usually
    // carries more children than each branch.
    let (max_children, revived_children) = if level == 0 { (100, 8.0) } else { (10, 4.0) };
    if level < procgena_tree::MAX_LEVELS as usize {
        let children = level_count!(children, level, "Children", (0, max_children), has_children);
        params.push(if level == 0 {
            children.exponential().revive(revived_children)
        } else {
            children.revive(revived_children)
        });
    }
    Group::titled(LEVEL_TITLES[level], params)
}

fn branches() -> Vec<Group<TreeOptions>> {
    let type_and_levels = Group::plain(vec![
        choice!(TreeOptions, "type", "Tree Type", [
            "Deciduous" => TreeType::Deciduous,
            "Evergreen" => TreeType::Evergreen,
        ], |o| o.tree_type),
        count!(
            TreeOptions,
            "branch.levels",
            "Levels",
            (0, procgena_tree::MAX_LEVELS),
            |o| o.branch.levels
        ),
        // Several stems make a multi-stemmed shrub; the tilt only matters then.
        count!(TreeOptions, "branch.stems", "Stems", (1, 40), |o| o
            .branch
            .stems)
        .exponential(),
        number!(
            TreeOptions,
            "branch.stemAngle",
            "Stem Angle",
            (0.0, 90.0, 1.0),
            |o| o.branch.stem_angle
        )
        .shown_if(|o| o.branch.stems > 1)
        .revive(20.0),
    ]);

    // The direction only matters while the strength is not zero.
    let strength = Group::titled(
        "Growth Direction",
        vec![
            number!(
                BranchOptions,
                "force.strength",
                "Strength",
                (-0.01, 0.01, 0.0001),
                |b| b.force.strength
            )
            .exponential(),
        ],
    );
    let direction = Group::plain(vec![
        number!(
            BranchOptions,
            "force.direction.x",
            "Direction X",
            (-1.0, 1.0, 0.01),
            |b| b.force.direction.x
        ),
        number!(
            BranchOptions,
            "force.direction.y",
            "Direction Y",
            (-1.0, 1.0, 0.01),
            |b| b.force.direction.y
        ),
        number!(
            BranchOptions,
            "force.direction.z",
            "Direction Z",
            (-1.0, 1.0, 0.01),
            |b| b.force.direction.z
        ),
    ])
    .shown_if(|b| b.force.strength != 0.0);
    let branch =
        |group: Group<BranchOptions>| lift(group, "branch", |o| &o.branch, |o| &mut o.branch);

    // Tree-wide settings first, then one group per branch level, trunk first.
    let mut groups = vec![type_and_levels, branch(strength), branch(direction)];
    groups.extend((0..=procgena_tree::MAX_LEVELS as usize).map(level_group));
    groups
}

/// Whether the tree has any leaves: they grow on the deepest branch level,
/// along each branch and, on deciduous trees, one at each tip. A count of
/// zero means no leaves at all, tip leaves included.
fn has_leaves(o: &TreeOptions) -> bool {
    let deepest = o.branch.levels as usize;
    grows(o, deepest) && o.leaves.count > 0
}

fn leaves() -> Vec<Group<TreeOptions>> {
    let count = Group::plain(vec![
        count!(LeafOptions, "count", "Count", (0, 100), |l| l.count).exponential(),
    ]);
    let along = Group::plain(vec![
        number!(LeafOptions, "start", "Start", (0.0, 1.0, 0.01), |l| l.start),
        number!(LeafOptions, "angle", "Angle", (0.0, 100.0, 1.0), |l| l
            .angle)
        .revive(40.0),
        // Turns leaves toward lying level and face up, as on cedar sprays.
        number!(LeafOptions, "flatten", "Flatten", (0.0, 1.0, 0.01), |l| l
            .flatten),
    ])
    .shown_if(|l| l.count > 0);
    let look = Group::plain(vec![
        name_choice!(LeafOptions, "type", "Type", LEAF_KINDS, |l| l.kind),
        color!(LeafOptions, "tint", "Tint", |l| l.tint),
        choice!(LeafOptions, "billboard", "Billboard", [
            "Single" => Billboard::Single,
            "Double" => Billboard::Double,
        ], |l| l.billboard),
        number!(LeafOptions, "size", "Size", (0.0, 1.0, 0.01), |l| l.size).revive(0.25),
        number!(
            LeafOptions,
            "sizeVariance",
            "Size Variance",
            (0.0, 1.0, 0.01),
            |l| l.size_variance
        ),
        number!(
            LeafOptions,
            "alphaTest",
            "Alpha Test",
            (0.0, 1.0, 0.01),
            |l| l.alpha_test
        ),
        toggle!(LeafOptions, "roundedNormals", "Rounded Normals", |l| l
            .rounded_normals),
    ]);

    let leaves =
        |group: Group<LeafOptions>| lift(group, "leaves", |o| &o.leaves, |o| &mut o.leaves);
    vec![
        leaves(count),
        leaves(along).shown_if(has_leaves),
        leaves(look).shown_if(has_leaves),
    ]
}

/// Whether any clusters are asked for: along branches, at tips, or both.
fn clusters_grow(p: &ClusterPlacement) -> bool {
    p.count > 0 || p.density > 0.0 || p.tips > 0.0
}

/// Whether clusters grow along the sides of branches, by count or by length.
fn side_clusters(p: &ClusterPlacement) -> bool {
    p.count > 0 || p.density > 0.0
}

/// Where clusters grow and how big they are; shared by fruit and flowers.
/// Only the cluster count, density and tip chance show while no clusters
/// are asked for.
fn placement() -> Group<ClusterPlacement> {
    let rest = Group::plain(vec![
        count!(
            ClusterPlacement,
            "minLevel",
            "From Level",
            (0, procgena_tree::MAX_LEVELS),
            |p| p.min_level
        ),
        number!(ClusterPlacement, "start", "Start", (0.0, 1.0, 0.01), |p| p
            .start)
        .shown_if(side_clusters),
        choice!(ClusterPlacement, "facing", "Facing", [
            "Hang" => Facing::Hang,
            "Up" => Facing::Up,
            "Outward" => Facing::Outward,
        ], |p| p.facing),
        number!(
            ClusterPlacement,
            "facingJitter",
            "Facing Jitter",
            (0.0, 90.0, 1.0),
            |p| p.facing_jitter
        ),
        number!(ClusterPlacement, "size", "Size", (0.005, 0.5, 0.005), |p| p
            .size)
        .exponential()
        .revive(0.1),
        number!(
            ClusterPlacement,
            "sizeVariance",
            "Size Variance",
            (0.0, 1.0, 0.01),
            |p| p.size_variance
        ),
    ])
    .shown_if(clusters_grow);

    let mut params = vec![
        count!(ClusterPlacement, "count", "Per Branch", (0, 20), |p| p
            .count)
        .exponential(),
        number!(
            ClusterPlacement,
            "density",
            "Per Metre",
            (0.0, 50.0, 0.5),
            |p| p.density
        )
        .exponential(),
        number!(
            ClusterPlacement,
            "tips",
            "Tip Chance",
            (0.0, 1.0, 0.01),
            |p| p.tips
        ),
    ];
    params.extend(rest.params);
    Group::titled("Placement", params)
}

/// Whether a cluster has stalks or a main stem to draw.
fn has_stalks(c: &ClusterShape) -> bool {
    !matches!(c.kind, ClusterKind::Single | ClusterKind::Umbel) || c.stalk_length > 0.0
}

/// How items are arranged within a cluster; shared by fruit and flowers.
/// The stalk colour is with each one's other colours.
fn cluster() -> Group<ClusterShape> {
    use ClusterKind::{Catkin, Panicle, Raceme, Single};
    let several = |c: &ClusterShape| c.kind != Single;
    let main_stem = |c: &ClusterShape| matches!(c.kind, Raceme | Panicle | Catkin);
    let side_stems = |c: &ClusterShape| c.kind == Panicle;
    Group::titled(
        "Cluster",
        vec![
            choice!(ClusterShape, "kind", "Kind", [
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
                "itemsVariance",
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
                "axisLength",
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
            count!(ClusterShape, "sideStalks", "Side Stalks", (1, 12), |c| c
                .side_stalks)
            .shown_if(side_stems),
            number!(
                ClusterShape,
                "sideLength",
                "Side Length",
                (0.0, 2.0, 0.01),
                |c| c.side_length
            )
            .shown_if(side_stems)
            .revive(0.4),
            number!(
                ClusterShape,
                "stalkLength",
                "Stalk Length",
                (0.0, 0.3, 0.001),
                |c| c.stalk_length
            )
            .exponential(),
            number!(
                ClusterShape,
                "stalkRadius",
                "Stalk Radius",
                (0.0, 0.02, 0.0005),
                |c| c.stalk_radius
            )
            .shown_if(has_stalks)
            .exponential()
            .revive(0.003),
        ],
    )
}

fn fruit() -> Vec<Group<TreeOptions>> {
    let maturity = Group::plain(vec![number!(
        FruitOptions,
        "maturity",
        "Ripeness",
        (0.0, 1.0, 0.01),
        |f| f.maturity
    )]);
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
                "topDimple",
                "Top Dimple",
                (0.0, 1.0, 0.01),
                |s| s.top_dimple
            ),
            number!(
                FruitShape,
                "bottomTaper",
                "Bottom Taper",
                (0.0, 1.0, 0.01),
                |s| s.bottom_taper
            ),
            count!(FruitShape, "lobes", "Lobes", (0, 12), |s| s.lobes),
            number!(
                FruitShape,
                "lobeDepth",
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
    // Catkin fruit have no stems of their own.
    let stem = Group::plain(vec![
        number!(
            FruitShape,
            "stemLength",
            "Stem Length",
            (0.0, 1.0, 0.01),
            |s| s.stem_length
        ),
        number!(
            FruitShape,
            "stemRadius",
            "Stem Radius",
            (0.0, 0.2, 0.005),
            |s| s.stem_radius
        )
        .shown_if(|s| s.stem_length > 0.0)
        .exponential()
        .revive(0.03),
        number!(
            FruitShape,
            "stemCurve",
            "Stem Curve",
            (0.0, 1.0, 0.01),
            |s| s.stem_curve
        )
        .shown_if(|s| s.stem_length > 0.0),
    ]);
    let has_stem =
        |f: &FruitOptions| f.cluster.kind != ClusterKind::Catkin && f.shape.stem_length > 0.0;

    // Unripe fruit show only the unripe colour, and ripe fruit only the ripe
    // one. The blush grows with ripeness.
    let colors = Group::titled(
        "Colour",
        vec![
            color!(FruitOptions, "shape.color", "Ripe", |f| f.shape.color)
                .shown_if(|f| f.maturity > 0.0),
            color!(FruitOptions, "shape.unripeColor", "Unripe", |f| f
                .shape
                .unripe_color)
            .shown_if(|f| f.maturity < 1.0),
            number!(
                FruitOptions,
                "shape.blushAmount",
                "Blush Amount",
                (0.0, 1.0, 0.01),
                |f| f.shape.blush_amount
            )
            .shown_if(|f| f.maturity > 0.0),
            color!(FruitOptions, "shape.blushColor", "Blush", |f| f
                .shape
                .blush_color)
            .shown_if(|f| f.maturity > 0.0 && f.shape.blush_amount > 0.0),
            color!(FruitOptions, "cluster.stalkColor", "Stalks", |f| f
                .cluster
                .stalk_color)
            .shown_if(move |f| has_stalks(&f.cluster) || has_stem(f)),
        ],
    );

    let lift_shape = |group: Group<FruitShape>| {
        group.lift("shape", |f: &FruitOptions| &f.shape, |f| &mut f.shape)
    };
    let grown = [
        maturity,
        cluster().lift("cluster", |f: &FruitOptions| &f.cluster, |f| &mut f.cluster),
        lift_shape(shape),
        lift_shape(stem).shown_if(|f| f.cluster.kind != ClusterKind::Catkin),
        colors,
    ]
    .map(|group| group.shown_if(|f| clusters_grow(&f.placement)));
    let [maturity, cluster, shape, stem, colors] = grown;

    let placement = placement().lift(
        "placement",
        |f: &FruitOptions| &f.placement,
        |f| &mut f.placement,
    );
    [placement, maturity, cluster, shape, stem, colors]
        .into_iter()
        .map(|group| lift(group, "fruit", |o| &o.fruit, |o| &mut o.fruit))
        .collect()
}

fn flowers() -> Vec<Group<TreeOptions>> {
    let maturity = Group::plain(vec![number!(
        FlowerOptions,
        "maturity",
        "Bloom",
        (0.0, 1.0, 0.01),
        |f| f.maturity
    )]);
    let shape = Group::titled(
        "Shape",
        vec![
            count!(FlowerShape, "petalCount", "Petals", (1, 20), |s| s
                .petal_count),
            count!(FlowerShape, "rings", "Rings", (1, 6), |s| s.rings),
            number!(
                FlowerShape,
                "petalLength",
                "Petal Length",
                (0.0, 2.0, 0.01),
                |s| s.petal_length
            )
            .revive(0.5),
            number!(
                FlowerShape,
                "petalWidth",
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
                "petalSegments",
                "Petal Segments",
                (1, 12),
                |s| s.petal_segments
            ),
            number!(
                FlowerShape,
                "centerSize",
                "Centre Size",
                (0.0, 1.0, 0.01),
                |s| s.center_size
            ),
        ],
    );
    // Flowers go from bud colour (bloom 0) through the open colours (0.5) to
    // the faded colour (1). Catkin flowers have no petal base or centre.
    let catkin = |f: &FlowerOptions| f.cluster.kind == ClusterKind::Catkin;
    let open = |f: &FlowerOptions| f.maturity > 0.0 && f.maturity < 1.0;
    let colors = Group::titled(
        "Colour",
        vec![
            color!(FlowerOptions, "shape.petalColor", "Petal", |f| f
                .shape
                .petal_color)
            .shown_if(open),
            color!(FlowerOptions, "shape.baseColor", "Petal Base", |f| f
                .shape
                .base_color)
            .shown_if(move |f| open(f) && !catkin(f)),
            color!(FlowerOptions, "shape.centerColor", "Centre", |f| f
                .shape
                .center_color)
            .shown_if(move |f| !catkin(f) && f.shape.center_size > 0.0),
            color!(FlowerOptions, "shape.budColor", "Bud", |f| f
                .shape
                .bud_color)
            .shown_if(|f| f.maturity < 0.5),
            color!(FlowerOptions, "shape.fadeColor", "Faded", |f| f
                .shape
                .fade_color)
            .shown_if(|f| f.maturity > 0.5),
            color!(FlowerOptions, "cluster.stalkColor", "Stalks", |f| f
                .cluster
                .stalk_color)
            .shown_if(|f| has_stalks(&f.cluster)),
        ],
    );

    let grown = [
        maturity,
        cluster().lift(
            "cluster",
            |f: &FlowerOptions| &f.cluster,
            |f| &mut f.cluster,
        ),
        shape
            .lift("shape", |f: &FlowerOptions| &f.shape, |f| &mut f.shape)
            .shown_if(move |f| !catkin(f)),
        colors,
    ]
    .map(|group| group.shown_if(|f| clusters_grow(&f.placement)));
    let [maturity, cluster, shape, colors] = grown;

    let placement = placement().lift(
        "placement",
        |f: &FlowerOptions| &f.placement,
        |f| &mut f.placement,
    );
    [placement, maturity, cluster, shape, colors]
        .into_iter()
        .map(|group| lift(group, "flowers", |o| &o.flowers, |o| &mut o.flowers))
        .collect()
}

fn trellis() -> Vec<Group<TreeOptions>> {
    let switches = Group::plain(vec![
        toggle!(TrellisOptions, "enabled", "Enabled", |t| t.enabled),
        toggle!(TrellisOptions, "visible", "Visible", |t| t.visible).shown_if(|t| t.enabled),
    ]);
    let position = Group::titled(
        "Position",
        vec![
            number!(TrellisOptions, "position.x", "X", (-2.0, 2.0, 0.01), |t| t
                .position
                .x),
            number!(TrellisOptions, "position.y", "Y", (-1.0, 1.0, 0.01), |t| t
                .position
                .y),
            number!(TrellisOptions, "position.z", "Z", (-2.0, 2.0, 0.01), |t| t
                .position
                .z),
        ],
    );
    let dimensions = Group::titled(
        "Dimensions",
        vec![
            number!(TrellisOptions, "width", "Width", (0.1, 5.0, 0.05), |t| t
                .width),
            number!(TrellisOptions, "height", "Height", (0.1, 5.0, 0.05), |t| t
                .height),
            number!(
                TrellisOptions,
                "spacing",
                "Spacing",
                (0.05, 1.0, 0.01),
                |t| t.spacing
            )
            .exponential(),
        ],
    );
    let force = Group::titled(
        "Force",
        vec![
            number!(
                TrellisOptions,
                "force.strength",
                "Strength",
                (0.0, 0.02, 0.0001),
                |t| t.force.strength
            )
            .exponential(),
            number!(
                TrellisOptions,
                "force.maxDistance",
                "Max Distance",
                (0.05, 2.0, 0.01),
                |t| t.force.max_distance
            )
            .shown_if(|t| t.force.strength > 0.0)
            .exponential(),
            number!(
                TrellisOptions,
                "force.falloff",
                "Falloff",
                (0.1, 3.0, 0.1),
                |t| t.force.falloff
            )
            .shown_if(|t| t.force.strength > 0.0),
        ],
    );
    let appearance = Group::titled(
        "Appearance",
        vec![
            number!(
                TrellisOptions,
                "cylinderRadius",
                "Wire Radius",
                (0.001, 0.05, 0.001),
                |t| t.cylinder_radius
            )
            .exponential(),
            color!(TrellisOptions, "color", "Colour", |t| t.color),
        ],
    )
    .shown_if(|t| t.visible);

    // A disabled trellis neither pulls branches nor is drawn.
    let enabled =
        [position, dimensions, force, appearance].map(|group| group.shown_if(|t| t.enabled));
    std::iter::once(switches)
        .chain(enabled)
        .map(|group| lift(group, "trellis", |o| &o.trellis, |o| &mut o.trellis))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use procgena_tree::TreePreset;
    use serde_json::Value;

    use super::*;
    use crate::params::{Control, checks};

    fn all_params() -> Vec<Param<TreeOptions>> {
        sections()
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .collect()
    }

    /// The JSON value at a dotted `key` path.
    fn json_at<'a>(json: &'a Value, key: &str) -> &'a Value {
        key.split('.').fold(json, |value, part| &value[part])
    }

    fn json(options: &TreeOptions) -> Value {
        serde_json::from_str(&options.to_json()).unwrap()
    }

    #[test]
    fn keys_are_unique() {
        let mut seen = HashSet::new();
        for param in all_params() {
            assert!(
                seen.insert(param.key.clone()),
                "duplicate key {}",
                param.key
            );
        }
    }

    #[test]
    fn every_setting_writes_the_json_field_named_by_its_key() {
        for param in all_params() {
            let mut options = TreeOptions::default();
            let key = &param.key;
            match &param.control {
                Control::Number {
                    min, max, get, set, ..
                } => {
                    // Pick an in-range value different from the current one.
                    let target = if get(&options) == *max { *min } else { *max };
                    set(&mut options, target);
                    assert_eq!(get(&options), target, "{key} reads back");
                    let written = json_at(&json(&options), key).as_f64();
                    let written = written.unwrap_or_else(|| panic!("{key} is not a JSON number"));
                    assert!(
                        (written as f32 - target).abs() < 1e-4,
                        "{key} wrote {written}"
                    );
                }
                Control::Toggle { get, set } => {
                    let flipped = !get(&options);
                    set(&mut options, flipped);
                    assert_eq!(
                        json_at(&json(&options), key),
                        &Value::Bool(flipped),
                        "{key}"
                    );
                }
                Control::Color { get, set } => {
                    set(&mut options, 0x123456);
                    assert_eq!(get(&options), 0x123456, "{key} reads back");
                    assert_eq!(
                        json_at(&json(&options), key),
                        &Value::from(0x123456),
                        "{key}"
                    );
                }
                Control::Choice {
                    options: names,
                    get,
                    set,
                } => {
                    // Every option round-trips, and the choices write distinct JSON.
                    let mut written = HashSet::new();
                    for index in 0..names.len() {
                        set(&mut options, index);
                        assert_eq!(get(&options), index, "{key} option {index}");
                        written.insert(json_at(&json(&options), key).to_string());
                    }
                    assert_eq!(written.len(), names.len(), "{key} writes each choice");
                }
            }
        }
    }

    #[test]
    fn every_range_end_is_valid() {
        for param in all_params() {
            let Control::Number { min, max, set, .. } = &param.control else {
                continue;
            };
            for end in [*min, *max] {
                let mut options = TreeOptions::default();
                set(&mut options, end);
                assert_eq!(options.validate(), Ok(()), "{} at {end}", param.key);
            }
        }
    }

    #[test]
    fn revived_settings_are_valid() {
        checks::revived_values_are_valid(&sections(), &varied_options(), |options| {
            options.validate().map_err(|error| error.to_string())
        });
    }

    #[test]
    fn a_new_branch_level_grows_and_can_be_seen() {
        // Raise Levels on every preset that has room, on both tree types.
        for preset in TreePreset::ALL {
            let before = preset.options();
            let new_level = before.branch.levels as usize + 1;
            if new_level > procgena_tree::MAX_LEVELS as usize {
                continue;
            }
            let mut after = before.clone();
            after.branch.levels += 1;
            crate::params::spec::revive(&sections(), &before, &mut after);
            let name = preset.name();
            assert!(grows(&after, new_level), "{name}: level {new_level} grows");
            assert!(
                after.branch.length[new_level] > 0.0,
                "{name}: level {new_level} has a length"
            );
        }
    }

    /// Default options with fruit and flowers growing in clusters of `kinds`,
    /// and the trellis switched on.
    fn flowering(kinds: (ClusterKind, ClusterKind)) -> TreeOptions {
        let mut options = TreeOptions::default();
        for (placement, cluster, kind) in [
            (
                &mut options.fruit.placement,
                &mut options.fruit.cluster,
                kinds.0,
            ),
            (
                &mut options.flowers.placement,
                &mut options.flowers.cluster,
                kinds.1,
            ),
        ] {
            placement.count = 2;
            placement.tips = 0.5;
            cluster.kind = kind;
            cluster.items = 6;
        }
        options.trellis.enabled = true;
        options
    }

    /// Builds the most detailed mesh, as the viewer does, or describes the
    /// error or panic.
    fn build(options: &TreeOptions) -> Result<procgena_tree::TreeMesh, String> {
        let built = std::panic::catch_unwind(|| {
            let skeleton = procgena_tree::Skeleton::generate(options)?;
            Ok::<_, procgena_tree::OptionsError>(
                skeleton.mesh(options, &procgena_tree::default_lod_levels()[0].detail),
            )
        });
        match built {
            Ok(Ok(mesh)) => Ok(mesh),
            Ok(Err(error)) => Err(format!("rejected: {error}")),
            Err(_) => Err("panicked".to_owned()),
        }
    }

    /// Builds the most detailed mesh and describes any error, panic, or
    /// non-finite vertex position.
    fn build_problem(options: &TreeOptions) -> Option<String> {
        let mesh = match build(options) {
            Ok(mesh) => mesh,
            Err(problem) => return Some(problem),
        };
        let parts = [
            ("branches", &mesh.branches),
            ("leaves", &mesh.leaves),
            ("fruit", &mesh.fruit),
            ("flowers", &mesh.flowers),
        ];
        let broken: Vec<&str> = parts
            .iter()
            .filter(|(_, buffer)| !buffer.positions.iter().flatten().all(|c| c.is_finite()))
            .map(|(name, _)| *name)
            .collect();
        (!broken.is_empty()).then(|| format!("non-finite positions in {broken:?}"))
    }

    #[test]
    fn every_range_end_builds_a_tree() {
        let bases = [
            ("plain", TreeOptions::default()),
            (
                "panicle/umbel",
                flowering((ClusterKind::Panicle, ClusterKind::Umbel)),
            ),
            (
                "catkin/raceme",
                flowering((ClusterKind::Catkin, ClusterKind::Raceme)),
            ),
        ];
        // Each thread checks every `threads`-th setting, with its own copy of
        // the settings table.
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let mut problems: Vec<String> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|first| {
                    let bases = &bases;
                    scope.spawn(move || {
                        let mut problems = Vec::new();
                        let params = all_params().into_iter().skip(first).step_by(threads);
                        for param in params {
                            let Control::Number { min, max, set, .. } = &param.control else {
                                continue;
                            };
                            // Fruit and flower settings only matter where they grow.
                            let clusters = param.key.starts_with("fruit.")
                                || param.key.starts_with("flowers.");
                            for (base_name, base) in bases {
                                if *base_name != "plain" && !clusters {
                                    continue;
                                }
                                for end in [*min, *max] {
                                    let mut options = base.clone();
                                    set(&mut options, end);
                                    if let Some(problem) = build_problem(&options) {
                                        let key = &param.key;
                                        problems.push(format!(
                                            "{base_name}: {key} at {end}: {problem}"
                                        ));
                                    }
                                }
                            }
                        }
                        problems
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|w| w.join().unwrap())
                .collect()
        });
        problems.sort();

        // Dragging one growth direction slider through zero can zero the whole vector.
        let mut options = flowering((ClusterKind::Single, ClusterKind::Single));
        options.branch.force.direction *= 0.0;
        if let Some(problem) = build_problem(&options) {
            problems.push(format!("zero growth direction: {problem}"));
        }

        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    /// Options that between them hide every kind of setting, named for messages.
    fn varied_options() -> Vec<(String, TreeOptions)> {
        use ClusterKind::{Catkin, Panicle, Raceme, Single, Umbel};
        let mut cases: Vec<(String, TreeOptions)> = TreePreset::ALL
            .iter()
            .map(|preset| (preset.name().to_owned(), preset.options()))
            .collect();
        cases.push(("panicle/umbel".into(), flowering((Panicle, Umbel))));
        cases.push(("catkin/catkin".into(), flowering((Catkin, Catkin))));

        // Bloom and ripeness at both ends.
        for maturity in [0.0, 1.0] {
            let mut options = flowering((Single, Raceme));
            options.fruit.maturity = maturity;
            options.flowers.maturity = maturity;
            cases.push((format!("maturity {maturity}"), options));
        }

        // Levels that only grow by tip continuation, or not at all.
        let mut bare = TreeOptions::default();
        bare.branch.children[0] = 0;
        cases.push(("deciduous, no side branches".into(), bare.clone()));
        bare.tree_type = TreeType::Evergreen;
        cases.push(("evergreen trunk only".into(), bare));
        cases
    }

    /// Copies of `options` with `param` set to each end of its range, or to
    /// each choice, each switch state, or two colours.
    fn variants(param: &Param<TreeOptions>, options: &TreeOptions) -> Vec<TreeOptions> {
        let with = |edit: &dyn Fn(&mut TreeOptions)| {
            let mut changed = options.clone();
            edit(&mut changed);
            changed
        };
        match &param.control {
            Control::Number { min, max, set, .. } => {
                vec![with(&|o| set(o, *min)), with(&|o| set(o, *max))]
            }
            Control::Toggle { get, set } => vec![with(&|o| set(o, !get(options)))],
            Control::Color { set, .. } => {
                vec![with(&|o| set(o, 0x123456)), with(&|o| set(o, 0xfedcba))]
            }
            Control::Choice {
                options: names,
                set,
                ..
            } => (0..names.len()).map(|i| with(&|o| set(o, i))).collect(),
        }
    }

    /// The mesh parts that differ by more than rounding error.
    fn mesh_differences(
        a: &procgena_tree::TreeMesh,
        b: &procgena_tree::TreeMesh,
    ) -> Vec<&'static str> {
        fn close<const N: usize>(a: &[[f32; N]], b: &[[f32; N]]) -> bool {
            a.len() == b.len()
                && a.iter()
                    .flatten()
                    .zip(b.iter().flatten())
                    .all(|(x, y)| (x - y).abs() <= 1e-4)
        }
        fn parts(m: &procgena_tree::TreeMesh) -> [(&'static str, &procgena_tree::MeshBuffers); 4] {
            [
                ("branches", &m.branches),
                ("leaves", &m.leaves),
                ("fruit", &m.fruit),
                ("flowers", &m.flowers),
            ]
        }
        parts(a)
            .into_iter()
            .zip(parts(b))
            .filter(|((_, x), (_, y))| {
                !(x.indices == y.indices
                    && close(&x.positions, &y.positions)
                    && close(&x.normals, &y.normals)
                    && close(&x.uvs, &y.uvs)
                    && close(&x.tangents, &y.tangents)
                    && close(&x.wind, &y.wind)
                    && close(&x.colors, &y.colors))
            })
            .map(|((name, _), _)| name)
            .collect()
    }

    /// Material-only settings, such as bark texture and trellis wires, cannot
    /// be checked this way and always pass.
    #[test]
    fn hidden_settings_do_not_change_the_tree() {
        let cases = varied_options();
        // Each thread checks every `threads`-th case, with its own copy of the
        // settings table.
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let mut problems: Vec<String> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|first| {
                    let cases = &cases;
                    scope.spawn(move || {
                        let params = all_params();
                        let mut problems = Vec::new();
                        for (name, options) in cases.iter().skip(first).step_by(threads) {
                            let mesh = match build(options) {
                                Ok(mesh) => mesh,
                                Err(problem) => {
                                    problems.push(format!("{name}: {problem}"));
                                    continue;
                                }
                            };
                            let hidden = params.iter().filter(|p| !p.is_shown(options));
                            for param in hidden {
                                for changed in variants(param, options) {
                                    let differences: Vec<String> = match build(&changed) {
                                        Ok(changed) => mesh_differences(&mesh, &changed)
                                            .into_iter()
                                            .map(str::to_owned)
                                            .collect(),
                                        Err(problem) => vec![problem],
                                    };
                                    if !differences.is_empty() {
                                        problems.push(format!(
                                            "{name}: hidden {} changes {differences:?}",
                                            param.key
                                        ));
                                    }
                                }
                            }
                        }
                        problems
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|w| w.join().unwrap())
                .collect()
        });
        problems.sort();
        problems.dedup();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn settings_come_after_the_settings_that_show_or_hide_them() {
        let params = all_params();
        for (name, options) in varied_options() {
            for (index, master) in params.iter().enumerate() {
                for changed in variants(master, &options) {
                    // Any setting whose visibility follows `master` must be below it.
                    let dependents = params.iter().enumerate().filter(|(_, dependent)| {
                        dependent.is_shown(&options) != dependent.is_shown(&changed)
                    });
                    for (dependent_index, dependent) in dependents {
                        assert!(
                            dependent_index > index,
                            "{name}: {} is shown or hidden by {}, which comes after it",
                            dependent.key,
                            master.key
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn no_setting_hides_itself() {
        let params = all_params();
        for (name, options) in varied_options() {
            for param in params.iter().filter(|p| p.is_shown(&options)) {
                for changed in variants(param, &options) {
                    assert!(
                        param.is_shown(&changed),
                        "{name}: changing {} hides it",
                        param.key
                    );
                }
            }
        }
    }

    #[test]
    fn presets_fit_the_controls() {
        let params = all_params();
        for preset in TreePreset::ALL {
            let options = preset.options();
            let name = preset.name();
            for param in &params {
                let key = &param.key;
                match &param.control {
                    Control::Number { min, max, get, .. } => {
                        let value = get(&options);
                        assert!(
                            (*min..=*max).contains(&value),
                            "{name}: {key} is {value}, outside {min}..={max}"
                        );
                    }
                    // A value missing from the list would show as the first option.
                    Control::Choice {
                        options: names,
                        get,
                        set,
                    } => {
                        let mut shown = options.clone();
                        set(&mut shown, get(&options));
                        assert_eq!(
                            json_at(&json(&shown), key),
                            json_at(&json(&options), key),
                            "{name}: {key} is not one of {names:?}"
                        );
                    }
                    Control::Toggle { .. } | Control::Color { .. } => {}
                }
            }
        }
    }
}
