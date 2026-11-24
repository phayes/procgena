//! Coloured triangle meshes of one cluster: its items and their stalks.

mod flower;
mod fruit;

use std::f32::consts::TAU;

use glam::{Quat, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use crate::skeleton::polyline_length;
use crate::{
    ClusterItem, ClusterKind, ClusterStalk, Facing, FruitOptions, FruitSkeleton, ItemShape,
};

/// Number of sides around a full-detail stalk.
const STALK_SIDES: usize = 6;
/// Number of sides around a simplified stalk or fruit stem.
const SIMPLE_STALK_SIDES: usize = 3;
/// Below this sine of the angle between a strip's path and its facing
/// direction, the strip keeps its previous sideways direction.
const STRIP_PARALLEL: f32 = 0.1;

/// Vertex data and triangle indices of one cluster's mesh, in the cluster
/// frame (see [`FruitSkeleton`]).
///
/// Every vertex list is in position order. Triangles are counter-clockwise
/// seen from their front; draw both sides, since stalk strips, flat items
/// and petals are single surfaces.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FruitMesh {
    /// Vertex positions in metres.
    pub positions: Vec<[f32; 3]>,
    /// Unit lighting directions.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates `(u, v)` in 0–1. Clusters use no textures; these
    /// only say where on each surface a vertex lies.
    pub uvs: Vec<[f32; 2]>,
    /// Surface colours as linear `[red, green, blue, alpha]`, ready for
    /// lighting. Multiply them by the material's base colour.
    pub colors: Vec<[f32; 4]>,
    /// How strongly wind moves each vertex on top of its host's sway, from
    /// 0 at the cluster's attachment to 1 at its farthest stalk tip or item.
    pub flutter: Vec<f32>,
    /// Groups of three indices into the vertex lists, one group per triangle.
    pub indices: Vec<u32>,
}

impl FruitMesh {
    /// The number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// The number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The index the next added vertex gets.
    fn next_index(&self) -> u32 {
        u32::try_from(self.positions.len()).expect("vertex count exceeds u32::MAX")
    }

    /// Adds one vertex.
    fn push(&mut self, position: Vec3, normal: Vec3, uv: [f32; 2], color: Vec4, flutter: f32) {
        self.positions.push(position.to_array());
        self.normals.push(normal.to_array());
        self.uvs.push(uv);
        self.colors.push(color.to_array());
        self.flutter.push(flutter);
    }
}

/// How much geometry a cluster mesh uses: one level of detail.
///
/// When deserialized, omitted fields use their defaults.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClusterDetail {
    /// Amount of shape detail for items and their stalks.
    pub item_detail: ItemDetail,
    /// Keep every Nth item, starting with the first. At least 1. Stalks
    /// that hold no kept item are left out. Flat items ignore this and keep
    /// every item.
    pub item_stride: u32,
    /// Multiplies the size of each kept item, to fill the gaps left by the
    /// others. Stalks keep their size.
    pub item_scale: f32,
    /// Whether to draw the stalks that hold the items.
    pub stalks: bool,
    /// Draw fruit and flowers flat when [`FruitOptions::size`] is below this
    /// many metres: each fruit becomes a rough disc and each flower a single
    /// triangle, facing away from the host. Catkins are not affected. 0
    /// turns this off.
    pub flat_item_size: f32,
}

impl Default for ClusterDetail {
    /// Full detail: every item and stalk, at full shape detail.
    fn default() -> Self {
        Self {
            item_detail: ItemDetail::Full,
            item_stride: 1,
            item_scale: 1.0,
            stalks: true,
            flat_item_size: 0.0,
        }
    }
}

/// How much geometry each fruit, flower and stalk uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemDetail {
    /// The full fruit and flower shape settings.
    #[default]
    Full,
    /// Fewer sides for fruit and stalks, and no calyx (the small flower
    /// remains on a fruit). Flowers have one petal layer with at most two
    /// divisions per petal.
    Simple,
    /// Coarse fruit bodies without stems or calyxes, a star-shaped surface
    /// for each non-catkin flower, and flat strips for stalks.
    Minimal,
}

/// Builds the mesh of `skeleton`, grown from `options`, at `detail`.
///
/// Pass validated options; the item shape, colours and maturity may differ
/// from the ones the skeleton was grown with.
pub(crate) fn mesh_cluster(
    skeleton: &FruitSkeleton,
    options: &FruitOptions,
    detail: &ClusterDetail,
) -> FruitMesh {
    let mut out = FruitMesh::default();
    let catkin = options.cluster.kind == ClusterKind::Catkin;
    let flat = !catkin && options.size < detail.flat_item_size;
    let stride = if flat {
        1
    } else {
        detail.item_stride.max(1) as usize
    };
    let items: Vec<ClusterItem> = skeleton
        .items
        .iter()
        .step_by(stride)
        .map(|item| ClusterItem {
            size: item.size * detail.item_scale,
            ..*item
        })
        .collect();
    let flutter = Flutter::new(skeleton.reach());
    let stalk_color = rgba(options.cluster.stalk_color);
    let out_dir = out_direction(options.facing);

    // Keep the full chain of stalks supporting the kept items.
    if detail.stalks {
        let shape = match detail.item_detail {
            ItemDetail::Full => StalkShape::Tube(STALK_SIDES),
            ItemDetail::Simple => StalkShape::Tube(SIMPLE_STALK_SIDES),
            ItemDetail::Minimal => StalkShape::Strip(out_dir),
        };
        let needed = needed_stalks(&skeleton.stalks, &items);
        for (stalk, _) in skeleton.stalks.iter().zip(needed).filter(|(_, n)| *n) {
            mesh_stalk(&mut out, stalk, shape, stalk_color, &flutter);
        }
    }

    // Then the items themselves.
    let level = detail.item_detail;
    match &options.item {
        ItemShape::Fruit(shape) => {
            let style = fruit::Style::new(shape, level, stalk_color, !catkin, options.facing);
            for (k, item) in items.iter().enumerate() {
                let at = flutter.at(item.distance);
                if flat {
                    fruit::mesh_disc(&mut out, item, k, shape, options, out_dir, at);
                } else {
                    fruit::mesh_item(&mut out, item, shape, options, &style, at);
                }
            }
        }
        ItemShape::Flower(shape) => {
            for item in &items {
                let at = flutter.at(item.distance);
                if flat {
                    flower::mesh_triangle(&mut out, item, shape, options, at);
                } else if catkin {
                    flower::mesh_catkin_item(&mut out, item, shape, options, at, level);
                } else {
                    flower::mesh_item(&mut out, item, shape, options, at, level);
                }
            }
        }
    }
    out
}

/// The unit direction away from the host in the cluster frame of `facing`.
fn out_direction(facing: Facing) -> Vec3 {
    match facing {
        Facing::Hang | Facing::Up => Vec3::Z,
        Facing::Outward => Vec3::Y,
    }
}

/// The unit direction up, away from the ground, in the cluster frame of
/// `facing`, assuming the host holds the cluster level.
fn up_direction(facing: Facing) -> Vec3 {
    match facing {
        Facing::Hang => Vec3::NEG_Y,
        Facing::Up => Vec3::Y,
        Facing::Outward => Vec3::Z,
    }
}

/// One flag per stalk, marking those that support any of `items`,
/// including the chain of parent stalks back to the cluster base.
fn needed_stalks(stalks: &[ClusterStalk], items: &[ClusterItem]) -> Vec<bool> {
    let mut needed = vec![false; stalks.len()];
    for item in items {
        let mut next = item.stalk;
        while let Some(s) = next {
            if needed[s] {
                break;
            }
            needed[s] = true;
            next = stalks[s].parent;
        }
    }
    needed
}

/// Flutter along a cluster's stalks: 0 at the base, rising to 1 at its
/// farthest point.
struct Flutter {
    reach: f32,
}

impl Flutter {
    fn new(reach: f32) -> Self {
        Self { reach }
    }

    /// The flutter at `distance` along the stalks, within 0–1; zero for a
    /// cluster with no length.
    fn at(&self, distance: f32) -> f32 {
        if self.reach > 0.0 {
            (distance / self.reach).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// How a stalk's surface is built.
#[derive(Clone, Copy)]
enum StalkShape {
    /// An open-ended tube with this many sides (at least one).
    Tube(usize),
    /// A flat strip as wide as the stalk, turned to face this unit direction.
    Strip(Vec3),
}

/// Appends a stalk following its points with the given shape. Adds nothing
/// for a nonpositive radius, fewer than two points, or zero total length.
fn mesh_stalk(
    out: &mut FruitMesh,
    stalk: &ClusterStalk,
    shape: StalkShape,
    color: Vec4,
    flutter: &Flutter,
) {
    let mut distance = stalk.base_distance;
    let mut rows = Vec::with_capacity(stalk.points.len());
    for (k, point) in stalk.points.iter().enumerate() {
        if k > 0 {
            distance += point.distance(stalk.points[k - 1]);
        }
        rows.push(flutter.at(distance));
    }
    let (grid, fallback) = match shape {
        StalkShape::Tube(sides) => (tube(&stalk.points, stalk.radius, sides, color), Vec3::Y),
        StalkShape::Strip(facing) => (strip(&stalk.points, stalk.radius, facing, color), facing),
    };
    if let Some(grid) = grid {
        grid.emit(out, fallback, |row| rows[row]);
    }
}

/// Creates a flat strip along `points`, `2 × radius` wide, with its flat
/// side turned as far toward the unit direction `facing` as the path
/// allows. Where the path runs along `facing`, the strip keeps its previous
/// sideways direction, so it never flips over. Returns `None` for a
/// nonpositive radius, fewer than two points, or zero total length.
fn strip(points: &[Vec3], radius: f32, facing: Vec3, color: Vec4) -> Option<Grid> {
    if radius <= 0.0 || points.len() < 2 || polyline_length(points) <= 0.0 {
        return None;
    }
    let n = points.len();
    let mut grid = Grid::new(2, false);
    let mut previous: Option<Vec3> = None;
    for k in 0..n {
        let tangent =
            (points[(k + 1).min(n - 1)] - points[k.saturating_sub(1)]).normalize_or(Vec3::Y);

        // Choose a sideways direction across the strip, keeping it steady along the path.
        let across = tangent.cross(facing);
        let across = match previous {
            _ if across.length() > STRIP_PARALLEL => across.normalize(),
            Some(p) => {
                (p - tangent * tangent.dot(p)).normalize_or(tangent.any_orthonormal_vector())
            }
            None => tangent.any_orthonormal_vector(),
        };
        let across = match previous {
            Some(p) if across.dot(p) < 0.0 => -across,
            _ => across,
        };
        previous = Some(across);

        // Listing the `across` edge first makes the front face `facing` until any sign flip above.
        let v = k as f32 / (n - 1) as f32;
        grid.push(points[k] + across * radius, [0.0, v], color);
        grid.push(points[k] - across * radius, [1.0, v], color);
    }
    Some(grid)
}

/// Creates an open-ended tube along `points`, with a radius in metres, a
/// positive side count and a linear RGBA colour. Returns `None` for a
/// nonpositive radius, fewer than two points, or zero total length.
fn tube(points: &[Vec3], radius: f32, sides: usize, color: Vec4) -> Option<Grid> {
    if radius <= 0.0 {
        return None;
    }
    sweep(points, &vec![radius; points.len()], sides, color)
}

/// Creates a surface along `points` with `sides` sides and a radius for
/// each point. Returns `None` for fewer than two points or zero total
/// length. Otherwise panics if `sides` is zero or `radii` is too short.
fn sweep(points: &[Vec3], radii: &[f32], sides: usize, color: Vec4) -> Option<Grid> {
    if points.len() < 2 || polyline_length(points) <= 0.0 {
        return None;
    }

    // Align each ring with the path through its neighbouring points.
    let n = points.len();
    let tangents: Vec<Vec3> = (0..n)
        .map(|k| {
            let before = points[k.saturating_sub(1)];
            let after = points[(k + 1).min(n - 1)];
            (after - before).normalize_or(Vec3::Y)
        })
        .collect();

    // Rotate successive rings as the path bends.
    let mut grid = Grid::new(sides + 1, true);
    let mut rotation = Quat::from_rotation_arc(Vec3::Y, tangents[0]);
    for k in 0..n {
        if k > 0 {
            rotation = Quat::from_rotation_arc(tangents[k - 1], tangents[k]) * rotation;
        }
        let v = k as f32 / (n - 1) as f32;
        for j in 0..=sides {
            let angle = loop_angle(j, sides);
            let around = rotation * Vec3::new(angle.cos(), 0.0, angle.sin());
            let u = j as f32 / sides as f32;
            grid.push(points[k] + around * radii[k], [u, v], color);
        }
    }
    Some(grid)
}

/// A surface made from equal-length rows of points. Neighbouring points form
/// triangles, with the front facing the cross product of the direction to the
/// next row and the direction to the next column.
struct Grid {
    columns: usize,
    /// Whether each row ends by repeating its first position to close a seam.
    wrap: bool,
    points: Vec<Vec3>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<Vec4>,
}

impl Grid {
    /// Starts an empty surface with `columns` points per row (at least two).
    /// Set `wrap` when the last point in each row repeats the first, so
    /// lighting stays smooth across that seam.
    fn new(columns: usize, wrap: bool) -> Self {
        Self {
            columns,
            wrap,
            points: Vec::new(),
            uvs: Vec::new(),
            colors: Vec::new(),
        }
    }

    /// Adds a position, texture coordinates and linear RGBA colour. Fill one
    /// row before starting the next.
    fn push(&mut self, point: Vec3, uv: [f32; 2], color: Vec4) {
        self.points.push(point);
        self.uvs.push(uv);
        self.colors.push(color);
    }

    /// Appends the surface, calling `flutter` with each vertex's row number.
    /// `fallback` is the unit lighting direction for points with no
    /// surrounding area. Triangles with zero area are left out.
    fn emit(&self, out: &mut FruitMesh, fallback: Vec3, flutter: impl Fn(usize) -> f32) {
        let rows = self.points.len() / self.columns;
        let at = |row: usize, column: usize| row * self.columns + column;

        // Join neighbouring rows and columns, then discard triangles with zero area.
        let mut triangles = Vec::new();
        for i in 0..rows.saturating_sub(1) {
            for j in 0..self.columns - 1 {
                let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j), at(i + 1, j + 1));
                triangles.push([a, c, b]);
                triangles.push([b, c, d]);
            }
        }
        triangles.retain(|t| self.face(t).length_squared() > 0.0);

        // Blend neighbouring triangle directions for smooth lighting; larger triangles count more.
        let mut normals = vec![Vec3::ZERO; self.points.len()];
        for t in &triangles {
            let face = self.face(t);
            for &v in t {
                normals[v] += face;
            }
        }

        // Give coincident points at a tip or seam the same lighting direction.
        for i in 0..rows {
            let row = at(i, 0)..at(i + 1, 0);
            let first = self.points[row.start];
            if self.points[row.clone()].iter().all(|&p| p == first) {
                let sum: Vec3 = normals[row.clone()].iter().sum();
                normals[row].fill(sum);
            }
        }
        if self.wrap {
            for i in 0..rows {
                let sum = normals[at(i, 0)] + normals[at(i, self.columns - 1)];
                normals[at(i, 0)] = sum;
                normals[at(i, self.columns - 1)] = sum;
            }
        }

        // Offset triangle indices by the vertices already in the output.
        let base = out.next_index();
        for (k, &point) in self.points.iter().enumerate() {
            let normal = normals[k].normalize_or(fallback);
            out.push(
                point,
                normal,
                self.uvs[k],
                self.colors[k],
                flutter(k / self.columns),
            );
        }
        for t in triangles {
            out.indices.extend(t.map(|v| base + v as u32));
        }
    }

    /// A vector perpendicular to the triangle at these point indices, twice
    /// its area long, pointing out of its front.
    fn face(&self, [a, b, c]: &[usize; 3]) -> Vec3 {
        let (a, b, c) = (self.points[*a], self.points[*b], self.points[*c]);
        (b - a).cross(c - a)
    }
}

/// Appends a coloured triangle with its front facing `normal`, swapping
/// corners if needed. A zero normal uses the triangle's own direction.
/// Collinear or coincident corners add nothing.
fn push_triangle(out: &mut FruitMesh, corners: [Vec3; 3], normal: Vec3, color: Vec4, flutter: f32) {
    let [a, mut b, mut c] = corners;
    let face = (b - a).cross(c - a);
    if face.length_squared() == 0.0 {
        return;
    }
    if face.dot(normal) < 0.0 {
        std::mem::swap(&mut b, &mut c);
    }
    let normal = normal.normalize_or(face.normalize());
    let base = out.next_index();
    for (point, uv) in [a, b, c]
        .into_iter()
        .zip([[0.5, 0.0], [0.0, 1.0], [1.0, 1.0]])
    {
        out.push(point, normal, uv, color, flutter);
    }
    out.indices.extend([base, base + 1, base + 2]);
}

/// Converts a `0xRRGGBB` display colour to linear RGBA for lighting, with
/// alpha 1. Bits above the lowest 24 are ignored.
pub(crate) fn rgba(hex: u32) -> Vec4 {
    let channel = |shift: u32| {
        let c = ((hex >> shift) & 0xff) as f32 / 255.0;
        if c < 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Vec4::new(channel(16), channel(8), channel(0), 1.0)
}

/// Turns a linear RGBA colour around the colour wheel by `turns` of a
/// whole turn, keeping its brightness and saturation. Alpha is kept.
fn turn_hue(color: Vec4, turns: f32) -> Vec4 {
    if turns == 0.0 {
        return color;
    }
    let (r, g, b) = (color.x, color.y, color.z);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = max - min;
    if chroma <= 0.0 {
        return color;
    }

    // Hue in sixths of a turn, then turned and wrapped.
    let hue = if max == r {
        ((g - b) / chroma).rem_euclid(6.0)
    } else if max == g {
        (b - r) / chroma + 2.0
    } else {
        (r - g) / chroma + 4.0
    };
    let hue = (hue + turns * 6.0).rem_euclid(6.0);

    // Back to red, green and blue at the same value and chroma.
    let x = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = max - chroma;
    Vec4::new(r + m, g + m, b + m, color.w)
}

/// An angle in radians for position `j` of `count` equal steps around a
/// loop; `j == count` gives exactly zero, to close a seam. Panics if
/// `count` is zero.
fn loop_angle(j: usize, count: usize) -> f32 {
    TAU * (j % count) as f32 / count as f32
}

/// Blends smoothly from 0 at `edge0` to 1 at `edge1`, clamping outside
/// that interval. Pass finite values with `edge0 < edge1`.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Blends from `a` at `t = 0` to `b` at `t = 1`, continuing beyond them.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tube_faces_outward_and_closes_its_seam() {
        let points = [Vec3::ZERO, Vec3::Y, Vec3::new(0.0, 2.0, 0.5)];
        let grid = tube(&points, 0.1, STALK_SIDES, Vec4::ONE).unwrap();
        let mut out = FruitMesh::default();
        grid.emit(&mut out, Vec3::Y, |row| row as f32);

        assert_eq!(out.vertex_count(), 3 * (STALK_SIDES + 1));
        assert_eq!(out.colors.len(), out.vertex_count());
        // The two edges of the seam must meet with matching lighting.
        let last = STALK_SIDES;
        assert_eq!(out.positions[0], out.positions[last]);
        assert_eq!(out.normals[0], out.normals[last]);
        // Lighting directions must point outward from the stalk.
        for (k, n) in out.normals.iter().enumerate().take(STALK_SIDES) {
            let p = Vec3::from_array(out.positions[k]);
            assert!(Vec3::from_array(*n).dot(p) > 0.0, "vertex {k}");
        }
        assert_eq!(out.flutter[out.vertex_count() - 1], 2.0);
    }

    #[test]
    fn tube_without_area_is_skipped() {
        assert!(tube(&[Vec3::ZERO, Vec3::Y], 0.0, 3, Vec4::ONE).is_none());
        assert!(tube(&[Vec3::ONE, Vec3::ONE], 0.1, 3, Vec4::ONE).is_none());
    }

    #[test]
    fn grid_pole_rows_share_one_normal() {
        // A cone with a pointed bottom and a triangular rim.
        let mut grid = Grid::new(4, true);
        for _ in 0..4 {
            grid.push(Vec3::NEG_Y, [0.0; 2], Vec4::ONE);
        }
        for j in 0..4 {
            let angle = TAU * j as f32 / 3.0;
            grid.push(
                Vec3::new(angle.cos(), 0.0, angle.sin()),
                [0.0; 2],
                Vec4::ONE,
            );
        }
        let mut out = FruitMesh::default();
        grid.emit(&mut out, Vec3::Y, |_| 0.0);
        // Only the three sides remain; triangles with two corners at the tip are skipped.
        assert_eq!(out.triangle_count(), 3);
        let pole = Vec3::from_array(out.normals[0]);
        assert!(pole.abs_diff_eq(Vec3::NEG_Y, 1e-5), "{pole}");
        assert!(out.normals[..4].iter().all(|n| *n == out.normals[0]));
    }

    #[test]
    fn push_triangle_turns_to_face_the_normal() {
        let mut out = FruitMesh::default();
        let corners = [Vec3::ZERO, Vec3::X, Vec3::Z];
        push_triangle(&mut out, corners, Vec3::Y, Vec4::ONE, 0.0);
        let p = |i: u32| Vec3::from_array(out.positions[i as usize]);
        let [a, b, c] = [out.indices[0], out.indices[1], out.indices[2]];
        assert!((p(b) - p(a)).cross(p(c) - p(a)).dot(Vec3::Y) > 0.0);
    }

    /// Keeping an item also keeps every stalk supporting it.
    #[test]
    fn needed_stalks_follow_kept_items_to_the_cluster_base() {
        let stalk = |parent| ClusterStalk {
            points: vec![Vec3::ZERO, Vec3::Y],
            radius: 0.1,
            base_distance: 0.0,
            parent,
        };
        // The kept item needs the main stem and first side stem, but not the second.
        let stalks = [stalk(None), stalk(Some(0)), stalk(Some(0)), stalk(Some(1))];
        let item = |stalk| ClusterItem {
            origin: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            size: 1.0,
            stalk,
            distance: 0.0,
            variation: crate::ItemVariation::NONE,
        };
        assert_eq!(
            needed_stalks(&stalks, &[item(Some(3))]),
            [true, true, false, true]
        );
        assert_eq!(needed_stalks(&stalks, &[item(None)]), [false; 4]);
    }

    #[test]
    fn strip_is_flat_and_as_wide_as_the_stalk() {
        // A path that rises, then runs along the facing direction, then turns down.
        let points = [
            Vec3::ZERO,
            Vec3::Y,
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
        ];
        let grid = strip(&points, 0.1, Vec3::X, Vec4::ONE).unwrap();
        let mut out = FruitMesh::default();
        grid.emit(&mut out, Vec3::X, |_| 0.0);

        // Two triangles per segment, two edge points per path point, each 0.2 apart.
        assert_eq!(out.triangle_count(), 2 * 3);
        for (k, pair) in out.positions.chunks(2).enumerate() {
            let (a, b) = (Vec3::from_array(pair[0]), Vec3::from_array(pair[1]));
            assert!((a.distance(b) - 0.2).abs() < 1e-5, "point {k}");
            assert!(a.midpoint(b).abs_diff_eq(points[k], 1e-5), "point {k}");
        }
        // The strip's width always runs along Z, so it never twists.
        for (k, pair) in out.positions.chunks(2).enumerate() {
            let across = Vec3::from_array(pair[0]) - Vec3::from_array(pair[1]);
            assert!(
                across.normalize().abs().abs_diff_eq(Vec3::Z, 1e-5),
                "point {k}"
            );
        }
        let first = Vec3::from_array(out.normals[0]);
        assert!(first.abs().abs_diff_eq(Vec3::X, 1e-4), "{first}");
        assert!(strip(&points, 0.0, Vec3::X, Vec4::ONE).is_none());
    }

    #[test]
    fn flutter_grows_to_one_at_the_farthest_point() {
        let flutter = Flutter::new(2.0);
        assert_eq!(flutter.at(0.0), 0.0);
        assert_eq!(flutter.at(1.0), 0.5);
        assert_eq!(flutter.at(5.0), 1.0);
        assert_eq!(Flutter::new(0.0).at(1.0), 0.0);
    }

    #[test]
    fn turning_the_hue_keeps_value_and_wraps() {
        let red = Vec4::new(0.8, 0.1, 0.1, 1.0);
        let green = turn_hue(red, 1.0 / 3.0);
        assert!(
            green.abs_diff_eq(Vec4::new(0.1, 0.8, 0.1, 1.0), 1e-5),
            "{green}"
        );
        assert!(turn_hue(red, 1.0).abs_diff_eq(red, 1e-5));
        let grey = Vec4::new(0.5, 0.5, 0.5, 1.0);
        assert_eq!(turn_hue(grey, 0.2), grey);
    }

    #[test]
    fn srgb_conversion_matches_three() {
        // An sRGB channel of 128 becomes about 0.21586 in linear RGB.
        let c = rgba(0x80_00_ff);
        assert!((c.x - 0.215_860_5).abs() < 1e-6, "{c}");
        assert_eq!(c.y, 0.0);
        assert!((c.z - 1.0).abs() < 1e-6);
    }
}
