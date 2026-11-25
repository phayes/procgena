//! Compare the outline of each tree's lower mesh with its game level.
//!
//! Each preset is projected into a small black-and-white mask from eight directions
//! around the trunk, then pooled into 4 × 4 pixel blocks so thin twigs and small
//! flowers count by where they are, not by which pixel centres they happen to
//! cover. Intersection over union (IoU) measures shared area divided by
//! combined area; 1 means identical masks. Leaf cards are solid squares here, so
//! these approximate checks do not include texture transparency or lighting.

use glam::{Vec2, Vec3};
use procgena_core::chain::LowerDetail;
use procgena_tree::{LodDetail, MeshBuffers, Skeleton, TreeMesh, TreePreset};

/// Width and height of each square mask, in pixels.
const RESOLUTION: usize = 96;
/// Number of views sampled at equal angles around the trunk.
const DIRECTIONS: usize = 8;

/// Three triangle corners in tree coordinates.
type Triangle = [Vec3; 3];

/// Projection axes and square image bounds used to compare two outlines.
struct View {
    /// Horizontal and vertical projection directions in tree coordinates.
    right: Vec3,
    up: Vec3,
    /// Lower-left image bound and square image size in projected metres.
    min: Vec2,
    size: f32,
}

impl View {
    /// Fits a square image around `reference`, with 15% extra space. `forward`
    /// sets the horizontal viewing direction; the vertical image axis stays at +Y.
    fn framing(forward: Vec3, reference: &[Triangle]) -> Self {
        let right = Vec3::Y.cross(-forward).normalize();
        let up = Vec3::Y;
        let mut min = Vec2::splat(f32::MAX);
        let mut max = Vec2::splat(f32::MIN);
        for p in reference.iter().flatten() {
            let q = Vec2::new(p.dot(right), p.dot(up));
            min = min.min(q);
            max = max.max(q);
        }
        let size = (max - min).max_element() * 1.15;
        let pad = (Vec2::splat(size) - (max - min)) / 2.0;
        Self {
            right,
            up,
            min: min - pad,
            size,
        }
    }

    /// Returns a mask marking pixel centres covered by any projected triangle.
    fn mask(&self, triangles: &[Triangle]) -> Vec<bool> {
        let mut mask = vec![false; RESOLUTION * RESOLUTION];
        let scale = RESOLUTION as f32 / self.size;
        let pixel = |p: Vec3| (Vec2::new(p.dot(self.right), p.dot(self.up)) - self.min) * scale;
        let last = RESOLUTION as f32 - 1.0;
        for t in triangles {
            let [a, b, c] = t.map(pixel);
            let area = (b - a).perp_dot(c - a);
            if area.abs() < 1e-9 {
                continue;
            }
            let lo = a.min(b).min(c).floor().max(Vec2::ZERO);
            let hi = a.max(b).max(c).ceil().min(Vec2::splat(last));
            if lo.x > hi.x || lo.y > hi.y {
                continue;
            }
            for y in lo.y as usize..=hi.y as usize {
                for x in lo.x as usize..=hi.x as usize {
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let inside = (b - a).perp_dot(p - a) / area >= 0.0
                        && (c - b).perp_dot(p - b) / area >= 0.0
                        && (a - c).perp_dot(p - c) / area >= 0.0;
                    if inside {
                        mask[y * RESOLUTION + x] = true;
                    }
                }
            }
        }
        mask
    }
}

/// Pixels along each side of the square blocks a mask is pooled into before
/// comparing.
const BLOCK: usize = 4;

/// Pools a `RESOLUTION`-square mask into `BLOCK`-square blocks, marking each
/// block that has any covered pixel. Twigs and small flowers are thinner than
/// a pixel and land on scattered pixel centres, so two meshes with the same
/// outline cover different single pixels; blocks compare the outline instead
/// of that speckle.
fn pooled(mask: &[bool]) -> Vec<bool> {
    let blocks = RESOLUTION / BLOCK;
    let mut out = vec![false; blocks * blocks];
    for (i, _) in mask.iter().enumerate().filter(|(_, covered)| **covered) {
        let (x, y) = (i % RESOLUTION / BLOCK, i / RESOLUTION / BLOCK);
        out[y * blocks + x] = true;
    }
    out
}

/// Returns shared area divided by combined area for equal-sized masks.
/// Two empty masks return 0.
fn iou(a: &[bool], b: &[bool]) -> f32 {
    let both = a.iter().zip(b).filter(|(a, b)| **a && **b).count();
    let either = a.iter().zip(b).filter(|(a, b)| **a || **b).count();
    both as f32 / either.max(1) as f32
}

/// Copies complete triangles using the mesh’s fixed vertex positions.
fn stored(buffers: &MeshBuffers, out: &mut Vec<Triangle>) {
    let p = |i: u32| Vec3::from_array(buffers.positions[i as usize]);
    for t in buffers.indices.as_chunks::<3>().0 {
        out.push(t.map(p));
    }
}

/// Collects bark, leaf, fruit and flower triangles. Fruit and flowers are
/// each kept cluster's own `procgena_fruit` mesh at the matching step, moved
/// into place. Flowers count because some presets, such as Flowering
/// Dogwood, have no leaves and their flowers make the canopy.
fn mesh_triangles(mesh: &TreeMesh) -> Vec<Triangle> {
    let mut out = Vec::new();
    for buffers in [&mesh.branches, &mesh.leaves, &mesh.fruit, &mesh.flowers] {
        stored(buffers, &mut out);
    }
    out
}

/// Outline overlap of the lower mesh with the game level, averaged over the
/// sampled views, and over presets for the whole set.
#[derive(Debug, Default, Clone, Copy)]
struct Scores {
    lower: f32,
}

impl Scores {
    /// The checked scores, each with its name and the minimum the average
    /// over every preset must reach.
    fn checks(&self) -> [(&'static str, f32, f32); 1] {
        [("lower vs game", self.lower, 0.88)]
    }
}

/// The game level and lower details of a tree drawn at full detail.
fn default_details() -> [LodDetail; 2] {
    let game = LodDetail::default();
    [game, game.lower()]
}

/// Measures each preset's overlap for `levels` (the game level, then the
/// lower mesh), using the game level to frame every comparison. `change`
/// receives each level's number and mesh, and may alter the mesh before it
/// is measured.
fn measure(
    levels: [LodDetail; 2],
    change: impl Fn(usize, &mut TreeMesh),
) -> Vec<(&'static TreePreset, Scores)> {
    let directions: Vec<Vec3> = (0..DIRECTIONS)
        .map(|k| {
            let angle = std::f32::consts::TAU * k as f32 / DIRECTIONS as f32;
            Vec3::new(angle.cos(), -0.15, angle.sin()).normalize()
        })
        .collect();

    let mut all = Vec::new();
    for preset in TreePreset::all() {
        let options = preset.tree.clone();
        let skeleton = Skeleton::generate(&options).unwrap();
        let meshes: Vec<TreeMesh> = levels
            .iter()
            .enumerate()
            .map(|(level, detail)| {
                let mut mesh = skeleton.mesh(&options, detail);
                change(level, &mut mesh);
                mesh
            })
            .collect();
        let triangles: Vec<Vec<Triangle>> = meshes.iter().map(mesh_triangles).collect();
        let mut scores = Scores::default();
        for &forward in &directions {
            let view = View::framing(forward, &triangles[0]);
            let masks: Vec<Vec<bool>> = triangles.iter().map(|t| pooled(&view.mask(t))).collect();
            scores.lower += iou(&masks[0], &masks[1]);
        }
        scores.lower /= DIRECTIONS as f32;
        all.push((preset, scores));
    }
    all
}

/// The lowest score any one preset may have on any check, so a preset that
/// loses its outline can't hide in the average. A tree whose lower mesh
/// drops everything but its twigs scores about 0.5.
const PRESET_MINIMUM: f32 = 0.7;

/// Prints every preset's scores and their average, and returns each check
/// that fails: an average below its minimum, or a preset below
/// [`PRESET_MINIMUM`].
fn failures(presets: &[(&'static TreePreset, Scores)]) -> Vec<String> {
    let mut average = Scores::default();
    for (preset, scores) in presets {
        eprintln!(
            "{:<22} {}",
            preset.name,
            scores.checks().map(|(_, s, _)| format!("{s:.3}")).join(" ")
        );
        average.lower += scores.lower / presets.len() as f32;
    }
    eprintln!("{average:#?}");

    let mut failed = Vec::new();
    for (name, score, minimum) in average.checks() {
        if score < minimum {
            failed.push(format!("average {name}: {score:.3} is below {minimum}"));
        }
    }
    for (preset, scores) in presets {
        for (name, score, _) in scores.checks() {
            if score < PRESET_MINIMUM {
                let preset = &preset.name;
                failed.push(format!(
                    "{preset} {name}: {score:.3} is below {PRESET_MINIMUM}"
                ));
            }
        }
    }
    failed
}

/// Checks a minimum average outline overlap of the lower mesh with the game
/// level, and a lower minimum for every preset.
#[test]
fn the_lower_mesh_keeps_the_outline() {
    let failed = failures(&measure(default_details(), |_, _| {}));
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The outline check must catch a lower mesh that drops its fruit clusters:
/// Spring Pussy Willow then keeps only its twigs.
#[test]
fn the_outline_check_catches_dropped_catkins() {
    let [game, lower] = default_details();
    let levels = [
        game,
        LodDetail {
            cluster_stride: u32::MAX,
            ..lower
        },
    ];
    let failed = failures(&measure(levels, |_, _| {}));
    assert!(
        failed.iter().any(|f| f.starts_with("Spring Pussy Willow")),
        "{failed:#?}"
    );
}

/// The outline check must catch a lower mesh that loses every leaf.
#[test]
fn the_outline_check_catches_missing_leaves() {
    let failed = failures(&measure(default_details(), |level, mesh| {
        if level == 1 {
            mesh.leaves = MeshBuffers::default();
        }
    }));
    assert!(!failed.is_empty(), "the lower mesh without leaves passed");
}
