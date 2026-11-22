//! Separate rounded clumps, such as moss cushions or burls, scattered over
//! the working surface. Each clump is its own closed blob, partly sunk into
//! the input.

use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::bvh::triangles_cross;
use crate::field::{ClumpField, SurfacePoint};
use crate::hash::Rng;
use crate::mesh::{BuildStats, ClumpMesh};
use crate::options::ClumpOptions;
use crate::shell::{face_normals, remove_unused_vertices};

/// Seed attempts per requested clump before giving up on placing more.
const ATTEMPTS_PER_CLUMP: u32 = 64;

/// Smallest size factor a clump may get from size variation, so no clump
/// shrinks to nothing.
const MIN_SIZE: f32 = 0.05;

/// Extra depth of a blob's buried bottom below the sink depth, as a
/// fraction of its height, like a layer clump's underside.
const BOTTOM_GAP: f32 = 0.25;

/// Fraction of the input's thickness a blob's buried bottom may reach.
const GAP_FILL: f32 = 0.5;

/// Widest a blob may be, as a fraction of the input's thickness under its
/// centre, so it doesn't wrap around a thin branch.
const ROOM_WIDTH: f32 = 0.5;

/// Fewest segments around a blob, and fewest rings from top to bottom.
const MIN_SEGMENTS: u32 = 4;
const MIN_RINGS: u32 = 3;

/// One clump to build: where it sits and how big it is.
#[derive(Debug, Clone, Copy)]
struct Seed {
    /// Point on the input surface.
    point: Vec3,
    /// Unit normal of the input there.
    normal: Vec3,
    /// Distance from the centre to the widest part of the blob.
    radius: f32,
    /// Height of the blob's top above the surface.
    height: f32,
    /// Depth of the blob's buried bottom below the surface.
    depth: f32,
}

/// A blob already kept in strict mode: its bounds and triangles.
struct Built {
    lo: Vec3,
    hi: Vec3,
    tris: Vec<[Vec3; 3]>,
}

/// Builds the scattered clumps for `options` on `field`'s input surface.
pub(crate) fn scatter(field: &ClumpField, options: &ClumpOptions) -> (ClumpMesh, BuildStats) {
    let edge_length = options.mesh.edge_length * options.scale;
    let seeds = place_seeds(field, options);

    // Build each blob; in strict mode, leave out any that would cross itself
    // or a blob already built.
    let mut mesh = ClumpMesh::default();
    let mut built: Vec<Built> = Vec::new();
    for seed in &seeds {
        let blob = build_blob(
            field,
            seed,
            options.shape.neck,
            edge_length,
            options.mesh.closed,
        );
        if options.mesh.strict {
            let tris = triangles(&blob);
            let (lo, hi) = bounds(&tris);
            let near = |other: &&Built| lo.cmple(other.hi).all() && hi.cmpge(other.lo).all();
            if crosses_itself(&tris)
                || built
                    .iter()
                    .filter(near)
                    .any(|other| crosses(&tris, &other.tris))
            {
                continue;
            }
            built.push(Built { lo, hi, tris });
        }
        append(&mut mesh, &blob);
    }
    let clumps = if options.mesh.strict {
        built.len()
    } else {
        seeds.len()
    };
    (
        mesh,
        BuildStats {
            clumps,
            ..BuildStats::default()
        },
    )
}

/// Picks up to `placement.count` clump centres on the input surface, spread
/// in proportion to area times mask, keeping centres at least
/// `placement.min_spacing` apart, and skipping spots where the input can't
/// carry a clump.
///
/// Candidates are drawn by area times the largest facing term at a
/// triangle's corners, which needs no rays, and each is kept with
/// probability mask ÷ that weight. Rays are cast only for candidates that
/// pass the spacing check.
fn place_seeds(field: &ClumpField, options: &ClumpOptions) -> Vec<Seed> {
    let placement = &options.placement;
    let scale = options.scale;
    let surface = field.surface();

    // Weight each input triangle by its area times its largest corner facing.
    let mut cumulative = Vec::with_capacity(surface.triangle_count());
    let mut facing = Vec::with_capacity(surface.triangle_count());
    let mut total = 0.0;
    for t in 0..surface.triangle_count() {
        let [a, b, c] = surface.triangle_positions(t);
        let area = 0.5 * (b - a).cross(c - a).length();
        let most = surface
            .triangle(t)
            .map(|v| field.facing(surface.normal(v).normalize_or_zero()))
            .into_iter()
            .fold(0.0, f32::max);
        total += area * most;
        cumulative.push(total);
        facing.push(most);
    }
    if total <= 0.0 {
        return Vec::new();
    }

    let mut rng = Rng::new(field.seed() ^ 0x0b10_b5ed);
    let mut seeds: Vec<Seed> = Vec::new();
    let spacing = placement.min_spacing * scale;
    for _ in 0..placement.count.saturating_mul(ATTEMPTS_PER_CLUMP) {
        if seeds.len() as u32 >= placement.count {
            break;
        }

        // Pick a triangle by weight, then a point on it.
        let pick = rng.next_f32() * total;
        let t = cumulative
            .partition_point(|&c| c <= pick)
            .min(cumulative.len() - 1);
        let (r1, r2) = (rng.next_f32().sqrt(), rng.next_f32());
        let variation = placement.size_variation;
        let size = rng.range(1.0 - variation, 1.0 + variation).max(MIN_SIZE);
        let accept = rng.next_f32();
        let corners = surface.triangle(t);
        let weights = [1.0 - r1, r1 * (1.0 - r2), r1 * r2];
        let point: Vec3 = (0..3)
            .map(|k| surface.position(corners[k]) * weights[k])
            .sum();
        let [a, b, c] = surface.triangle_positions(t);
        let normal = (0..3)
            .map(|k| surface.normal(corners[k]) * weights[k])
            .sum::<Vec3>()
            .normalize_or((b - a).cross(c - a).normalize_or(Vec3::Y));
        if seeds.iter().any(|s| s.point.distance(point) < spacing) {
            continue;
        }

        // Measure the spot, then keep it in proportion to its mask.
        let here = field.measure(SurfacePoint::new(point, normal));
        let full = field.mask(&here);
        if full <= 0.0 || accept * facing[t] >= full {
            continue;
        }
        let mask = full.min(1.0);
        let height = field.thickness() * size * mask;
        seeds.push(Seed {
            point,
            normal,
            radius: (placement.radius * scale * size).min(ROOM_WIDTH * here.room),
            height,
            depth: (field.sink() * size + BOTTOM_GAP * height).min(GAP_FILL * here.room),
        });
    }
    seeds
}

/// Builds one blob: a sphere-like mesh of rings, shaped so it is `seed.radius`
/// wide at its widest, rises `seed.height` above the surface and sinks
/// `seed.depth` below it. `neck` narrows the base to `1 - neck` of the width,
/// so the blob bulges out above it. Each column of the blob is moved to
/// follow the input's curvature, and lumps are added on its visible part.
/// Triangle edges are about `edge_length` long. A blob that isn't `closed`
/// leaves out triangles that are entirely buried.
fn build_blob(
    field: &ClumpField,
    seed: &Seed,
    neck: f32,
    edge_length: f32,
    closed: bool,
) -> ClumpMesh {
    let Seed {
        point,
        normal,
        radius,
        height,
        depth,
    } = *seed;
    let tangent = normal.any_orthonormal_vector();
    let bitangent = tangent.cross(normal);

    // The upper part is an ellipsoid with its widest ring at height `centre`
    // and its top at `height`; where it crosses the surface it is
    // `1 - neck` as wide. Below the surface it is squashed to `depth`.
    let k = (1.0 - (1.0 - neck).powi(2)).sqrt();
    let semi = height / (1.0 + k);
    let centre = k * semi;
    let below = semi - centre;

    // Ring counts from the blob's size, so edges are about `edge_length`.
    let segments = ((TAU * radius / edge_length).ceil() as u32).max(MIN_SEGMENTS);
    let meridian = 0.5 * PI * (radius + 0.5 * (height + depth));
    let rings = ((meridian / edge_length).ceil() as u32).max(MIN_RINGS);

    // Place every vertex: the top pole, the rings, then the bottom pole.
    let mut positions = Vec::new();
    let mut fade = Vec::new();
    let mut buried = Vec::new();
    let mut place = |dir: Vec3| {
        let y = centre + dir.y * semi;
        let y = if y < 0.0 {
            y * depth / below.max(f32::MIN_POSITIVE)
        } else {
            y
        };
        let offset = (tangent * dir.x + bitangent * dir.z) * radius;
        let drape = (field.bvh().closest_point(point + offset).point - point).dot(normal);
        let mut p = point + offset + normal * (y + drape);
        let visible = field.fade(y);
        if y > 0.0 {
            let outward =
                (tangent * dir.x + normal * dir.y + bitangent * dir.z).normalize_or(normal);
            p += outward * field.lump_height(p) * visible;
        }
        positions.push(p.to_array());
        fade.push(visible);
        buried.push(y <= 0.0);
    };
    place(Vec3::Y);
    for i in 1..rings {
        let theta = PI * i as f32 / rings as f32;
        for j in 0..segments {
            let phi = TAU * j as f32 / segments as f32;
            place(Vec3::new(
                theta.sin() * phi.cos(),
                theta.cos(),
                theta.sin() * phi.sin(),
            ));
        }
    }
    place(Vec3::NEG_Y);

    // Join them, counter-clockwise seen from outside.
    let top = 0u32;
    let bottom = 1 + (rings - 1) * segments;
    let at = |ring: u32, j: u32| 1 + (ring - 1) * segments + j % segments;
    let mut indices = Vec::new();
    for j in 0..segments {
        indices.extend_from_slice(&[top, at(1, j + 1), at(1, j)]);
        for ring in 1..rings - 1 {
            let (a, b, c, d) = (
                at(ring, j),
                at(ring, j + 1),
                at(ring + 1, j),
                at(ring + 1, j + 1),
            );
            indices.extend_from_slice(&[a, b, c, b, d, c]);
        }
        indices.extend_from_slice(&[at(rings - 1, j), at(rings - 1, j + 1), bottom]);
    }

    // An open blob keeps only triangles with a visible corner.
    if !closed {
        let mut kept = Vec::with_capacity(indices.len());
        for tri in indices.as_chunks::<3>().0 {
            if tri.iter().any(|&v| !buried[v as usize]) {
                kept.extend_from_slice(tri);
            }
        }
        indices = kept;
    }
    let mut mesh = ClumpMesh {
        positions,
        normals: Vec::new(),
        fade,
        indices,
    };
    remove_unused_vertices(&mut mesh);
    mesh.normals = face_normals(&mesh.positions, &mesh.indices);
    mesh
}

/// Appends `blob`'s vertices and triangles to `mesh`.
fn append(mesh: &mut ClumpMesh, blob: &ClumpMesh) {
    let base = mesh.positions.len() as u32;
    mesh.positions.extend_from_slice(&blob.positions);
    mesh.normals.extend_from_slice(&blob.normals);
    mesh.fade.extend_from_slice(&blob.fade);
    mesh.indices.extend(blob.indices.iter().map(|i| i + base));
}

/// Returns the corner positions of every triangle in `mesh`.
fn triangles(mesh: &ClumpMesh) -> Vec<[Vec3; 3]> {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| t.map(|i| Vec3::from(mesh.positions[i as usize])))
        .collect()
}

/// Returns whether any triangle of `a` passes through any triangle of `b`.
fn crosses(a: &[[Vec3; 3]], b: &[[Vec3; 3]]) -> bool {
    let (lo, hi) = bounds(b);
    a.iter()
        .filter(|t| {
            let (tl, th) = bounds(std::slice::from_ref(t));
            tl.cmple(hi).all() && th.cmpge(lo).all()
        })
        .any(|t| b.iter().any(|u| triangles_cross(*t, *u)))
}

/// Returns whether any two triangles of `tris` that share no corner pass
/// through each other.
fn crosses_itself(tris: &[[Vec3; 3]]) -> bool {
    tris.iter().enumerate().any(|(i, t)| {
        tris[i + 1..].iter().any(|u| {
            let shared = t.iter().any(|p| u.contains(p));
            !shared && triangles_cross(*t, *u)
        })
    })
}

/// Returns the corner-wise bounds of `tris`.
fn bounds(tris: &[[Vec3; 3]]) -> (Vec3, Vec3) {
    tris.iter().flatten().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), &p| (lo.min(p), hi.max(p)),
    )
}
