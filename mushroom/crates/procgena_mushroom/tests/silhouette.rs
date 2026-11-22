//! Coarse detail levels keep the full mesh's outline.
//!
//! Each level is drawn as a black-and-white mask, from the side and from
//! above, and compared with the full mesh's mask by "intersection over
//! union": the area both cover divided by the area either covers. 1 means
//! identical outlines.

use glam::{Vec2, Vec3};
use procgena_mushroom::{MushroomDetail, MushroomMesh, MushroomPreset};

/// Mask resolution along each side.
const GRID: usize = 96;

/// Smallest acceptable overlap with the full mesh's outline, per level.
const MIN_OVERLAP: [(&str, MushroomDetail, f32); 4] = [
    ("high", MushroomDetail::HIGH, 0.9),
    ("medium", MushroomDetail::MEDIUM, 0.8),
    ("low", MushroomDetail::LOW, 0.7),
    ("flat", MushroomDetail::FLAT, 0.6),
];

/// A viewing direction and the two axes of the picture plane.
struct View {
    name: &'static str,
    /// Direction from the mushroom toward the viewer.
    toward_viewer: Vec3,
    right: Vec3,
    up: Vec3,
}

const VIEWS: [View; 2] = [
    View {
        name: "side",
        toward_viewer: Vec3::Z,
        right: Vec3::X,
        up: Vec3::Y,
    },
    View {
        name: "top",
        toward_viewer: Vec3::Y,
        right: Vec3::X,
        up: Vec3::NEG_Z,
    },
];

/// Every triangle of `mesh` as seen in `view`, with a flat stem turned to
/// face the viewer as a renderer would.
fn triangles(mesh: &MushroomMesh, view: &View) -> Vec<[Vec2; 3]> {
    let project = |p: Vec3| Vec2::new(p.dot(view.right), p.dot(view.up));
    let mut out = Vec::new();
    let b = &mesh.surfaces;
    for t in b.indices.chunks(3) {
        out.push([t[0], t[1], t[2]].map(|i| project(Vec3::from(b.positions[i as usize]))));
    }
    if let Some(ribbon) = &mesh.stem_ribbon {
        let turned: Vec<Vec3> = ribbon
            .billboards
            .iter()
            .map(|v| {
                let axis = Vec3::from(v.axis);
                let right = axis.cross(view.toward_viewer).normalize_or_zero();
                Vec3::from(v.pivot) + right * v.offset[0] + axis * v.offset[1]
            })
            .collect();
        for t in ribbon.strip.indices.chunks(3) {
            out.push([t[0], t[1], t[2]].map(|i| project(turned[i as usize])));
        }
    }
    out
}

/// Draws `tris` into a mask covering `min..max`.
fn mask(tris: &[[Vec2; 3]], min: Vec2, max: Vec2) -> Vec<bool> {
    let mut cells = vec![false; GRID * GRID];
    let size = (max - min) / GRID as f32;
    for [a, b, c] in tris {
        let area = (b - a).perp_dot(c - a);
        if area.abs() < 1e-12 {
            continue;
        }
        // Only test cells inside the triangle's bounding box.
        let lo = ((a.min(*b).min(*c) - min) / size).floor().max(Vec2::ZERO);
        let hi = ((a.max(*b).max(*c) - min) / size)
            .ceil()
            .min(Vec2::splat(GRID as f32));
        for y in lo.y as usize..hi.y as usize {
            for x in lo.x as usize..hi.x as usize {
                let p = min + (Vec2::new(x as f32, y as f32) + 0.5) * size;
                let w0 = (b - p).perp_dot(c - p) / area;
                let w1 = (c - p).perp_dot(a - p) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    cells[y * GRID + x] = true;
                }
            }
        }
    }
    cells
}

/// Area covered by both masks divided by area covered by either.
fn overlap(a: &[bool], b: &[bool]) -> f32 {
    let both = a.iter().zip(b).filter(|(x, y)| **x && **y).count();
    let either = a.iter().zip(b).filter(|(x, y)| **x || **y).count();
    both as f32 / either.max(1) as f32
}

#[test]
fn coarse_levels_keep_the_outline() {
    let mut failures = Vec::new();
    for preset in MushroomPreset::ALL {
        let mushroom = procgena_mushroom::Mushroom::from_preset(preset);
        let full = mushroom.mesh_with_detail(&MushroomDetail::FULL).unwrap();
        for view in &VIEWS {
            // Frame the full mesh with a small margin.
            let full_tris = triangles(&full, view);
            let points = full_tris.iter().flatten();
            let (min, max) = points.fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| {
                (lo.min(*p), hi.max(*p))
            });
            let margin = (max - min) * 0.05;
            let (min, max) = (min - margin, max + margin);
            let reference = mask(&full_tris, min, max);

            for (name, detail, required) in MIN_OVERLAP {
                let mesh = mushroom.mesh_with_detail(&detail).unwrap();
                let score = overlap(&reference, &mask(&triangles(&mesh, view), min, max));
                if score < required {
                    failures.push(format!(
                        "{} {name} from the {}: {score:.3} < {required}",
                        preset.name(),
                        view.name
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "outlines differ:\n{}",
        failures.join("\n")
    );
}
