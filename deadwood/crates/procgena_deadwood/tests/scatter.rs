//! Stick scatter over flat and shaped ground.

use glam::{Vec2, Vec3};
use procgena_deadwood::{
    DeadwoodLod, FlatGround, HeightGrid, ScatterError, ScatterOptions, StickScatter,
};
use procgena_mesh::{Object, Part, SurfaceMesh, TagFilter, tags};

/// Kindling over a small 3 m square, to keep the tests quick.
fn small() -> ScatterOptions {
    ScatterOptions {
        size: [3.0, 3.0],
        ..ScatterOptions::kindling()
    }
}

#[test]
fn scatter_is_repeatable_spaced_and_in_bounds() {
    let o = small();
    let a = StickScatter::new(&o, &FlatGround, None).unwrap();
    let b = StickScatter::new(&o, &FlatGround, None).unwrap();
    assert_eq!(a, b);
    assert!(!a.instances().is_empty());
    assert_eq!(a.variants().len(), o.variants as usize);

    let sites: Vec<Vec2> = a
        .instances()
        .iter()
        .map(|i| {
            assert!((i.variant as usize) < a.variants().len());
            assert!(i.transform.translation.is_finite());
            Vec2::new(i.position.x, i.position.z)
        })
        .collect();
    for (k, s) in sites.iter().enumerate() {
        assert!(s.x.abs() <= 1.5 + 1e-4 && s.y.abs() <= 1.5 + 1e-4, "{s}");
        for t in &sites[k + 1..] {
            assert!(s.distance(*t) >= o.min_spacing - 1e-4);
        }
    }
}

#[test]
fn density_thins_the_scatter() {
    let o = small();
    let full = StickScatter::new(&o, &FlatGround, None).unwrap();
    let half_bare = |x: f32, _z: f32| if x < 0.0 { 0.0 } else { 1.0 };
    let thinned = StickScatter::new(&o, &FlatGround, Some(&half_bare)).unwrap();
    assert!(thinned.instances().len() < full.instances().len());
    for i in thinned.instances() {
        assert!(i.position.x >= 0.0, "{}", i.position);
    }
}

#[test]
fn sticks_follow_a_slope() {
    let o = small();
    let slope = |x: f32, _z: f32| 0.3 * x;
    let s = StickScatter::new(&o, &slope, None).unwrap();
    for i in s.instances() {
        let p = i.position;
        let up = i.transform.matrix3 * glam::Vec3A::Y;
        // Tilted to follow the slope, with the reported position on the
        // ground at the stick's site.
        assert!(up.x < -0.1, "up {up}");
        assert!((p.y - slope(p.x, p.z)).abs() < 1e-4, "site {p}");
        let middle = i.transform.transform_point3(glam::Vec3::ZERO);
        assert!(middle.is_finite());
    }
}

#[test]
fn no_ground_drops_sticks() {
    let o = small();
    let hole = |x: f32, _z: f32| if x < 0.0 { f32::NAN } else { 0.0 };
    let s = StickScatter::new(&o, &hole, None).unwrap();
    for i in s.instances() {
        assert!(i.position.x >= 0.0);
    }
}

#[test]
fn height_grid_from_an_object_follows_its_top() {
    // A tilted square, tagged as ground.
    let mesh = SurfaceMesh {
        positions: vec![
            [-2.0, -0.4, -2.0],
            [2.0, 0.4, -2.0],
            [2.0, 0.4, 2.0],
            [-2.0, -0.4, 2.0],
        ],
        normals: vec![[0.0, 1.0, 0.0]; 4],
        indices: vec![0, 2, 1, 0, 3, 2],
    };
    let object = Object::single(vec![Part::new([tags::GROUND], mesh)]);
    let grid =
        HeightGrid::from_object(&object, 0, &TagFilter::any_of([tags::GROUND]), 0.1).unwrap();
    use procgena_deadwood::Ground;
    assert!((grid.height(1.0, 0.5) - 0.2).abs() < 1e-3);
    let s = StickScatter::new(&small(), &grid, None).unwrap();
    assert!(!s.instances().is_empty());
}

#[test]
fn too_many_sticks_is_an_error() {
    let o = ScatterOptions {
        size: [1000.0, 1000.0],
        density: 10.0,
        ..ScatterOptions::twig_litter()
    };
    assert!(matches!(
        StickScatter::new(&o, &FlatGround, None),
        Err(ScatterError::TooMany { .. })
    ));
}

#[test]
fn baked_object_is_valid() {
    let s = StickScatter::new(&small(), &FlatGround, None).unwrap();
    let object = s.object(&DeadwoodLod::default()).unwrap();
    object.validate().unwrap();
    let part = &object.levels[0].parts[0];
    assert!(part.closed);
    let centre: Vec3 = part
        .mesh
        .positions
        .iter()
        .map(|p| Vec3::from(*p))
        .sum::<Vec3>()
        / part.mesh.positions.len() as f32;
    assert!(centre.y > -0.1);
}
