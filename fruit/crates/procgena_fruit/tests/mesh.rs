//! Check cluster meshes: fruit bodies, stems and calyxes, flowers and
//! catkins, stalks, maturity and colour, and the cluster detail levels.

use std::ops::Range;

use glam::Vec3;
use procgena_fruit::{
    ClusterDetail, ClusterKind, Facing, FlowerShape, Fruit, FruitMesh, FruitOptions, FruitShape,
    ItemDetail, ItemShape,
};

/// Every cluster arrangement.
const KINDS: [ClusterKind; 5] = [
    ClusterKind::Single,
    ClusterKind::Umbel,
    ClusterKind::Raceme,
    ClusterKind::Panicle,
    ClusterKind::Catkin,
];

/// Item shape detail levels exercised by these tests.
const DETAILS: [ItemDetail; 3] = [ItemDetail::Full, ItemDetail::Simple, ItemDetail::Minimal];

/// Fruit settings arranged as `kind` with five items, on visible stalks.
fn fruit(kind: ClusterKind) -> FruitOptions {
    let mut options = FruitOptions::default();
    options.cluster.kind = kind;
    options.cluster.items = 5;
    options
}

/// Open flowers with two petal layers, arranged as `kind` with five items.
fn flowers(kind: ClusterKind) -> FruitOptions {
    let mut options = fruit(kind);
    options.facing = Facing::Outward;
    options.maturity = 0.5;
    options.item = ItemShape::Flower(FlowerShape {
        rings: 2,
        ..FlowerShape::default()
    });
    options
}

/// A single item attached directly to its host, with no stalk.
fn bare_single(mut options: FruitOptions) -> FruitOptions {
    options.cluster.kind = ClusterKind::Single;
    options.cluster.stalk_length = 0.0;
    options
}

/// The fruit shape, which `options` must have.
fn fruit_shape(options: &mut FruitOptions) -> &mut FruitShape {
    match &mut options.item {
        ItemShape::Fruit(shape) => shape,
        ItemShape::Flower(_) => panic!("not fruit"),
    }
}

/// The flower shape, which `options` must have.
fn flower_shape(options: &mut FruitOptions) -> &mut FlowerShape {
    match &mut options.item {
        ItemShape::Flower(shape) => shape,
        ItemShape::Fruit(_) => panic!("not flowers"),
    }
}

/// Grows the first slot of `options`' pool.
fn grow(options: &FruitOptions) -> Fruit {
    Fruit::new(options.clone(), 11, 0).unwrap()
}

/// The mesh of `options`' first slot at `detail`.
fn mesh(options: &FruitOptions, detail: &ClusterDetail) -> FruitMesh {
    grow(options).mesh(detail)
}

/// A detail level with the given shape detail, item stride and item scale.
fn detail(item_detail: ItemDetail, item_stride: u32, item_scale: f32) -> ClusterDetail {
    ClusterDetail {
        item_detail,
        item_stride,
        item_scale,
        ..ClusterDetail::FULL
    }
}

/// The diagonal of the mesh's bounding box.
fn extent(mesh: &FruitMesh) -> f32 {
    let points = mesh.positions.iter().map(|p| Vec3::from_array(*p));
    let (min, max) = points.fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)));
    min.distance(max)
}

/// Checks a nonempty mesh for finite positions, unit lighting normals, valid
/// indices, triangles with area facing their normals, and colours and
/// flutter within 0–1.
#[track_caller]
fn assert_well_formed(name: &str, mesh: &FruitMesh) {
    let n = mesh.vertex_count();
    assert!(n > 0, "{name}: no vertices");
    assert_eq!(mesh.normals.len(), n, "{name}");
    assert_eq!(mesh.uvs.len(), n, "{name}");
    assert_eq!(mesh.colors.len(), n, "{name}");
    assert_eq!(mesh.flutter.len(), n, "{name}");
    assert_eq!(mesh.indices.len() % 3, 0, "{name}");
    for (p, normal) in mesh.positions.iter().zip(&mesh.normals) {
        assert!(Vec3::from_array(*p).is_finite(), "{name}: {p:?}");
        let length = Vec3::from_array(*normal).length();
        assert!(
            (length - 1.0).abs() < 1e-4,
            "{name}: normal length {length}"
        );
    }
    assert!(mesh.indices.iter().all(|&i| (i as usize) < n), "{name}");
    assert!(
        mesh.flutter.iter().all(|f| (0.0..=1.0).contains(f)),
        "{name}"
    );
    assert!(
        mesh.colors
            .iter()
            .flatten()
            .all(|c| (0.0..=1.0).contains(c)),
        "{name}"
    );

    // Each triangle needs area, and normals pointing toward its front.
    let p = |i: u32| Vec3::from_array(mesh.positions[i as usize]);
    let normal = |i: u32| Vec3::from_array(mesh.normals[i as usize]);
    for t in mesh.indices.chunks(3) {
        let face = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
        assert!(face.length_squared() > 0.0, "{name}: triangle without area");
        let smooth = normal(t[0]) + normal(t[1]) + normal(t[2]);
        assert!(
            face.dot(smooth) > 0.0,
            "{name}: triangle faces away from its normals"
        );
    }
}

/// Counts groups of triangles connected by shared vertex indices.
fn connected_pieces(indices: &[u32], range: Range<usize>) -> usize {
    let mut parent: Vec<usize> = (0..range.len()).collect();
    let mut used = vec![false; range.len()];
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for t in indices.chunks(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| i as usize - range.start);
        for v in [a, b, c] {
            used[v] = true;
        }
        let ra = root(&mut parent, a);
        for other in [b, c] {
            let ro = root(&mut parent, other);
            parent[ro] = ra;
        }
    }
    (0..range.len())
        .filter(|&i| used[i] && root(&mut parent, i) == i)
        .count()
}

/// The number of separately connected pieces in a whole mesh.
fn pieces(mesh: &FruitMesh) -> usize {
    connected_pieces(&mesh.indices, 0..mesh.vertex_count())
}

#[test]
fn every_arrangement_and_detail_builds_well_formed_meshes() {
    for kind in KINDS {
        for base in [fruit(kind), flowers(kind)] {
            for maturity in [0.0, 0.5, 1.0] {
                let mut options = base.clone();
                options.maturity = maturity;
                for item_detail in DETAILS {
                    for stride in [1, 2] {
                        let name = format!("{kind:?} {item_detail:?} {maturity} {stride}");
                        let mesh = mesh(&options, &detail(item_detail, stride, 1.25));
                        assert_well_formed(&name, &mesh);
                    }
                }
            }
        }
    }
}

#[test]
fn unripe_fruit_is_smaller_than_ripe_fruit() {
    // Each maturity is a pool of its own, so fix the size and measure from
    // the attachment, which the item's random turn leaves alone.
    let mut options = bare_single(fruit(ClusterKind::Single));
    options.size_variance = 0.0;
    let reach = |mesh: FruitMesh| {
        mesh.positions
            .iter()
            .map(|p| Vec3::from_array(*p).length())
            .fold(0.0, f32::max)
    };
    options.maturity = 0.0;
    let unripe = reach(mesh(&options, &ClusterDetail::FULL));
    options.maturity = 1.0;
    let ripe = reach(mesh(&options, &ClusterDetail::FULL));
    // At maturity zero, every fruit dimension is 30% of its ripe value.
    assert!((unripe / ripe - 0.3).abs() < 1e-3, "{unripe} vs {ripe}");
}

#[test]
fn fruit_colour_ripens_from_unripe_to_ripe() {
    let mut options = bare_single(fruit(ClusterKind::Single));
    let shape = fruit_shape(&mut options);
    shape.unripe_color = 0x00ff00;
    shape.color = 0xff0000;
    shape.blush_amount = 0.0;
    // Leave out the stem and calyx to measure only the skin.
    shape.stem_length = 0.0;
    shape.calyx = false;
    for (maturity, expected) in [(0.0, [0.0, 1.0, 0.0, 1.0]), (1.0, [1.0, 0.0, 0.0, 1.0])] {
        options.maturity = maturity;
        let mesh = mesh(&options, &ClusterDetail::FULL);
        assert!(!mesh.colors.is_empty());
        assert!(mesh.colors.iter().all(|c| *c == expected), "{maturity}");
    }
}

/// The blush colours the side of the fruit that faces up and away from its
/// host: for a hanging fruit, local −Y and +Z.
#[test]
fn blush_colours_the_upper_side_of_ripe_fruit() {
    let mut options = bare_single(fruit(ClusterKind::Single));
    let shape = fruit_shape(&mut options);
    shape.color = 0x00ff00;
    shape.blush_color = 0xff0000;
    shape.blush_amount = 1.0;
    shape.stem_length = 0.0;
    shape.calyx = false;
    let mesh = mesh(&options, &ClusterDetail::FULL);

    // Compare the skin at the most upward- and downward-facing vertices.
    let up = |i: &usize| -mesh.normals[*i][1];
    let range = 0..mesh.vertex_count();
    let top = range
        .clone()
        .max_by(|a, b| up(a).total_cmp(&up(b)))
        .unwrap();
    let bottom = range.min_by(|a, b| up(a).total_cmp(&up(b))).unwrap();
    assert!(mesh.colors[top][0] > 0.2, "{:?}", mesh.colors[top]);
    assert_eq!(mesh.colors[bottom], [0.0, 1.0, 0.0, 1.0]);
}

#[test]
fn lobes_change_the_outline() {
    // The smallest and largest radius around the fruit's widest ring.
    let ring_radii = |lobes: u32| {
        let mut options = bare_single(fruit(ClusterKind::Single));
        let shape = fruit_shape(&mut options);
        shape.stem_length = 0.0;
        shape.segments = 24;
        shape.rings = 8;
        shape.lobes = lobes;
        shape.lobe_depth = 0.4;
        let fruit = grow(&options);
        let mesh = fruit.mesh(&ClusterDetail::FULL);

        let item = fruit.skeleton().items[0];
        let to_local = item.orientation.inverse();
        let ring = 4 * 25;
        let radii: Vec<f32> = mesh.positions[ring..ring + 24]
            .iter()
            .map(|p| {
                let local = to_local * (Vec3::from_array(*p) - item.origin);
                local.with_y(0.0).length()
            })
            .collect();
        let min = radii.iter().copied().fold(f32::MAX, f32::min);
        let max = radii.iter().copied().fold(0.0, f32::max);
        (min, max)
    };

    let (min, max) = ring_radii(0);
    assert!(max - min < 1e-4 * max, "smooth fruit: {min} to {max}");
    let (min, max) = ring_radii(6);
    assert!(
        (min / max - 0.6).abs() < 1e-3,
        "ribbed fruit: {min} to {max}"
    );
}

#[test]
fn calyx_adds_one_triangle_per_sepal() {
    let mut options = fruit(ClusterKind::Umbel);
    fruit_shape(&mut options).calyx = false;
    let without = mesh(&options, &ClusterDetail::FULL).triangle_count();
    fruit_shape(&mut options).calyx = true;
    let with = mesh(&options, &ClusterDetail::FULL).triangle_count();
    assert_eq!(with - without, 5 * 5);
}

#[test]
fn flutter_grows_along_the_stalks() {
    for options in [fruit(ClusterKind::Raceme), flowers(ClusterKind::Raceme)] {
        let mesh = mesh(&options, &ClusterDetail::FULL);
        // Zero at the stem base, one at the farthest point.
        assert_eq!(mesh.flutter[0], 0.0);
        assert_eq!(mesh.flutter.iter().copied().fold(0.0, f32::max), 1.0);
    }
}

#[test]
fn a_bud_is_smaller_than_an_open_flower() {
    let mut options = bare_single(flowers(ClusterKind::Single));
    options.maturity = 0.0;
    let bud = extent(&mesh(&options, &ClusterDetail::FULL));
    options.maturity = 0.5;
    let open = extent(&mesh(&options, &ClusterDetail::FULL));
    assert!(bud < 0.8 * open, "bud {bud}, open {open}");
}

#[test]
fn each_petal_is_a_separate_piece() {
    let mut options = bare_single(flowers(ClusterKind::Single));
    let shape = flower_shape(&mut options);
    shape.petal_count = 5;
    shape.rings = 2;
    shape.center_size = 0.0;
    assert_eq!(pieces(&mesh(&options, &ClusterDetail::FULL)), 10);
}

#[test]
fn flower_colour_follows_bud_open_and_faded_stages() {
    let mut options = bare_single(flowers(ClusterKind::Single));
    let shape = flower_shape(&mut options);
    shape.center_size = 0.0;
    shape.bud_color = 0xff0000;
    shape.petal_color = 0x00ff00;
    shape.base_color = 0x00ff00;
    shape.fade_color = 0x0000ff;
    for (maturity, expected) in [
        (0.0, [1.0, 0.0, 0.0, 1.0]),
        (0.5, [0.0, 1.0, 0.0, 1.0]),
        (1.0, [0.0, 0.0, 1.0, 1.0]),
    ] {
        options.maturity = maturity;
        let mesh = mesh(&options, &ClusterDetail::FULL);
        assert!(!mesh.colors.is_empty());
        assert!(mesh.colors.iter().all(|c| *c == expected), "{maturity}");
    }
}

#[test]
fn faded_petals_droop_below_open_petals() {
    // The mean petal height above the flower's base, along its facing (+Y).
    let mean_rise = |maturity: f32| {
        let mut options = bare_single(flowers(ClusterKind::Single));
        flower_shape(&mut options).center_size = 0.0;
        options.maturity = maturity;
        let mesh = mesh(&options, &ClusterDetail::FULL);
        let sum: f32 = mesh.positions.iter().map(|p| p[1]).sum();
        sum / mesh.vertex_count() as f32
    };
    assert!(mean_rise(1.0) < mean_rise(0.5));
}

#[test]
fn catkins_ignore_petal_settings() {
    let options = flowers(ClusterKind::Catkin);
    let mut changed = options.clone();
    let shape = flower_shape(&mut changed);
    shape.petal_count = 9;
    shape.rings = 3;
    shape.petal_length = 0.9;
    shape.petal_width = 0.1;
    shape.cup = 0.9;
    shape.curl = 0.7;
    shape.notch = 0.6;
    shape.petal_segments = 7;
    shape.center_size = 0.4;
    shape.center_color = 0x123456;
    shape.base_color = 0x654321;

    // Changed settings name another pool, so the items land elsewhere; but
    // they keep the same geometry and the same colours.
    let colours = |mesh: &FruitMesh| {
        let mut colours: Vec<[u32; 4]> = mesh.colors.iter().map(|c| c.map(f32::to_bits)).collect();
        colours.sort_unstable();
        colours.dedup();
        colours
    };
    let original = mesh(&options, &ClusterDetail::FULL);
    let varied = mesh(&changed, &ClusterDetail::FULL);
    assert!(original.vertex_count() > 0);
    assert_eq!(original.triangle_count(), varied.triangle_count());
    assert_eq!(colours(&original), colours(&varied));
}

#[test]
fn a_single_flower_opens_above_its_attachment() {
    let options = bare_single(flowers(ClusterKind::Single));
    let mesh = mesh(&options, &ClusterDetail::FULL);
    let lowest = mesh.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
    assert!(lowest >= -1e-4, "{lowest} below the attachment");
}

#[test]
fn lower_detail_uses_fewer_triangles() {
    for kind in KINDS {
        for options in [fruit(kind), flowers(kind)] {
            let counts = DETAILS.map(|d| mesh(&options, &detail(d, 1, 1.0)).triangle_count());
            let [full, simple, minimal] = counts;
            assert!(full > simple && simple > minimal, "{kind:?}: {counts:?}");
        }
    }
}

#[test]
fn item_stride_drops_bare_stalks_and_keeps_flutter() {
    // A stride of two keeps items 0, 2 and 4 and their own stalks.
    for options in [fruit(ClusterKind::Umbel), flowers(ClusterKind::Umbel)] {
        let all = mesh(&options, &ClusterDetail::FULL);
        let kept = mesh(&options, &detail(ItemDetail::Full, 2, 1.0));
        assert_eq!(all.vertex_count() * 3, kept.vertex_count() * 5);
    }

    // Thinning keeps the flutter values of the geometry it keeps.
    for kind in KINDS {
        for options in [fruit(kind), flowers(kind)] {
            let all = mesh(&options, &ClusterDetail::FULL);
            let kept = mesh(&options, &detail(ItemDetail::Simple, 2, 1.25));
            for f in &kept.flutter {
                assert!(all.flutter.contains(f), "{kind:?}: new flutter {f}");
            }
        }
    }
}

#[test]
fn item_scale_resizes_each_item() {
    for options in [
        bare_single(fruit(ClusterKind::Single)),
        bare_single(flowers(ClusterKind::Single)),
    ] {
        let normal = extent(&mesh(&options, &detail(ItemDetail::Full, 1, 1.0)));
        let doubled = extent(&mesh(&options, &detail(ItemDetail::Full, 1, 2.0)));
        assert!(
            (doubled / normal - 2.0).abs() < 1e-3,
            "{}",
            doubled / normal
        );
    }
}

#[test]
fn simpler_fruit_drop_the_calyx_then_the_stem() {
    let mut options = bare_single(fruit(ClusterKind::Single));
    fruit_shape(&mut options).calyx = true;
    fruit_shape(&mut options).stem_length = 0.2;
    // Full detail has a body, a stem and five calyx points; simple drops the
    // calyx, and minimal also drops the stem.
    for (item_detail, expected) in [
        (ItemDetail::Full, 7),
        (ItemDetail::Simple, 2),
        (ItemDetail::Minimal, 1),
    ] {
        let mesh = mesh(&options, &detail(item_detail, 1, 1.0));
        assert_eq!(pieces(&mesh), expected, "{item_detail:?}");
    }
}

#[test]
fn simple_flowers_keep_one_layer_of_two_segment_petals() {
    let mut options = bare_single(flowers(ClusterKind::Single));
    let shape = flower_shape(&mut options);
    shape.petal_count = 5;
    shape.rings = 3;
    shape.petal_segments = 6;
    shape.center_size = 0.3;
    let mesh = mesh(&options, &detail(ItemDetail::Simple, 1, 1.0));
    // Five petals of two divisions of four triangles each, and a six-sided centre.
    assert_eq!(mesh.triangle_count(), 5 * 2 * 4 + 6);
    assert_eq!(pieces(&mesh), 5 + 1);
}

#[test]
fn minimal_flowers_are_single_stars() {
    let mut options = bare_single(flowers(ClusterKind::Single));
    flower_shape(&mut options).petal_count = 6;
    flower_shape(&mut options).rings = 2;
    for maturity in [0.0, 0.5, 1.0] {
        options.maturity = maturity;
        let mesh = mesh(&options, &detail(ItemDetail::Minimal, 1, 1.0));
        // Each petal tip joins the centre and its two neighbouring notches.
        assert_eq!(mesh.triangle_count(), 2 * 6, "maturity {maturity}");
        assert_eq!(pieces(&mesh), 1);
    }

    // Closed buds use only the bud colour; open flowers show the centre colour.
    let shape = flower_shape(&mut options);
    shape.bud_color = 0xff0000;
    shape.center_color = 0x0000ff;
    shape.center_size = 0.3;
    let centre = [0.0, 0.0, 1.0, 1.0];
    for (maturity, shows_centre) in [(0.0, false), (0.5, true)] {
        options.maturity = maturity;
        let mesh = mesh(&options, &detail(ItemDetail::Minimal, 1, 1.0));
        assert_eq!(
            mesh.colors.contains(&centre),
            shows_centre,
            "maturity {maturity}"
        );
        if maturity == 0.0 {
            assert!(mesh.colors.iter().all(|c| *c == [1.0, 0.0, 0.0, 1.0]));
        }
    }
}

/// Minimal catkins still draw their kept items separately: with 12 items and
/// stride 3, four remain, not one body standing in for them all.
#[test]
fn minimal_catkins_draw_their_items() {
    for mut options in [fruit(ClusterKind::Catkin), flowers(ClusterKind::Catkin)] {
        options.cluster.items = 12;
        let all = mesh(&options, &detail(ItemDetail::Minimal, 1, 1.0));
        let thinned = mesh(&options, &detail(ItemDetail::Minimal, 3, 1.0));
        // The stalk stays, so the difference is the eight skipped items.
        assert_eq!(pieces(&all) - pieces(&thinned), 8);
    }
}

/// Leaving out stalks keeps the items, with fewer triangles.
#[test]
fn stalks_off_keeps_the_items_without_their_stalks() {
    for kind in [
        ClusterKind::Umbel,
        ClusterKind::Raceme,
        ClusterKind::Panicle,
    ] {
        for options in [fruit(kind), flowers(kind)] {
            let with = mesh(&options, &ClusterDetail::FULL);
            let without = mesh(
                &options,
                &ClusterDetail {
                    stalks: false,
                    ..ClusterDetail::FULL
                },
            );
            assert!(without.triangle_count() > 0, "{kind:?}");
            assert!(without.triangle_count() < with.triangle_count(), "{kind:?}");
        }
    }
}

/// The far level's settings, with `flat_item_size`.
fn flat(flat_item_size: f32) -> ClusterDetail {
    ClusterDetail {
        item_detail: ItemDetail::Minimal,
        item_stride: 2,
        item_scale: 1.25,
        flat_item_size,
        ..ClusterDetail::FULL
    }
}

/// Small items in `kind` clusters, with stalks too thin to draw so only the
/// items themselves are meshed.
fn small_items(mut options: FruitOptions) -> FruitOptions {
    options.size = 0.3;
    options.cluster.stalk_radius = 0.0;
    options
}

#[test]
fn small_items_drawn_flat_keep_every_item() {
    for kind in [
        ClusterKind::Single,
        ClusterKind::Umbel,
        ClusterKind::Raceme,
        ClusterKind::Panicle,
    ] {
        // Every fruit is a six-triangle disc and every flower one triangle,
        // whatever the item stride.
        let options = small_items(fruit(kind));
        let fruit_mesh = mesh(&options, &flat(0.75));
        assert_well_formed("fruit", &fruit_mesh);
        let items = grow(&options).skeleton().items.len();
        assert_eq!(fruit_mesh.triangle_count(), 6 * items, "{kind:?}");

        let options = small_items(flowers(kind));
        let mesh = mesh(&options, &flat(0.75));
        assert_well_formed("flowers", &mesh);
        let items = grow(&options).skeleton().items.len();
        assert_eq!(mesh.triangle_count(), items, "{kind:?}");
    }
}

/// Flat fruit face away from the host (local +Z for a hanging cluster), each
/// leaning a different way.
#[test]
fn flat_fruit_discs_face_out_at_varied_angles() {
    let options = small_items(fruit(ClusterKind::Raceme));
    let mesh = mesh(&options, &flat(0.75));
    let normals: Vec<Vec3> = mesh
        .normals
        .chunks(2 * 7)
        .map(|disc| Vec3::from_array(disc[0]))
        .collect();
    assert!(normals.len() > 2);
    for pair in normals.windows(2) {
        assert!(pair[0].dot(pair[1]) < 0.999, "{pair:?}");
    }
    // None is seen edge-on from outside.
    for n in &normals {
        assert!(n.dot(Vec3::Z).abs() >= 0.6_f32.cos() - 1e-4, "{n}");
    }
}

#[test]
fn items_at_or_above_the_flat_size_keep_their_shape() {
    for mut options in [
        small_items(fruit(ClusterKind::Single)),
        small_items(flowers(ClusterKind::Single)),
    ] {
        options.size = 0.75;
        assert_eq!(mesh(&options, &flat(0.75)), mesh(&options, &flat(0.0)));
    }
}

#[test]
fn catkins_ignore_the_flat_size() {
    for options in [
        small_items(fruit(ClusterKind::Catkin)),
        small_items(flowers(ClusterKind::Catkin)),
    ] {
        assert_eq!(mesh(&options, &flat(0.75)), mesh(&options, &flat(0.0)));
    }
}

#[test]
fn minimal_stalks_are_flat_strips() {
    // A single flower on a curved stalk, drawn as one triangle.
    let mut options = flowers(ClusterKind::Single);
    options.size = 0.3;
    let segments = grow(&options).skeleton().stalks[0].points.len() - 1;
    let mesh = mesh(&options, &flat(0.75));
    assert_eq!(mesh.triangle_count(), 2 * segments + 1);
}

#[test]
fn flat_items_still_follow_item_scale() {
    for mut options in [
        bare_single(fruit(ClusterKind::Single)),
        bare_single(flowers(ClusterKind::Single)),
    ] {
        options.size = 0.3;
        let at = |item_scale| {
            let detail = ClusterDetail {
                item_detail: ItemDetail::Minimal,
                item_scale,
                flat_item_size: 0.75,
                ..ClusterDetail::FULL
            };
            extent(&mesh(&options, &detail))
        };
        assert!((at(2.0) / at(1.0) - 2.0).abs() < 1e-3);
    }
}
