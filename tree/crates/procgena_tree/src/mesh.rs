//! Build bark, leaf, fruit and flower triangle meshes from a [`Skeleton`].
//!
//! A tree's [`game_level`](TreeOptions::game_level) is its real geometry; a
//! coarser mesh for small-on-screen trees is derived from it
//! ([`LowerDetail::lower`]). Choose any other detail with [`LodDetail`];
//! camera-based switching is up to the application.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::ops::Range;

use glam::{Quat, Vec3};
use procgena_core::chain::LowerDetail;
use serde::{Deserialize, Serialize};

use crate::compose::compose_clusters;
use crate::units::LEAF_NORMAL_REFERENCE;
use crate::wind::{BranchWind, branch_winds, wind_at};
use crate::{
    BarkOptions, BarkUvMode, Billboard, Section, Skeleton, SkeletonBranch, SkeletonLeaf,
    TreeOptions,
};

/// Settings for the amount of geometry produced by [`Skeleton::mesh`].
///
/// The default keeps all branch sections and leaves at their original size and detail.
/// It is the default [`game_level`](TreeOptions::game_level) of every tree.
/// Increase the strides or reduce `segment_factor` for a simpler mesh.
/// When deserialized, omitted fields use their defaults.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LodDetail {
    /// Keep every Nth circular slice along each branch, starting at its base.
    /// The base and tip are always kept. For example, 2 skips every other interior slice.
    /// Both 0 and 1 keep every slice.
    pub section_stride: u32,
    /// Scale the number of sides used to draw tube branches. For example, 0.5
    /// roughly halves the side count. The result is rounded, with at least three
    /// sides per branch.
    pub segment_factor: f32,
    /// Whether to close tube branches into watertight solids. When true, each
    /// branch is sealed at its base and tip, and a branch that carries on from
    /// its parent's tip joins it seamlessly instead, so the trunk and each limb
    /// form one closed, outward-facing shell. Side branches are separate shells
    /// that start inside their parent. Use closed bark for physics bodies,
    /// volume and mass, or voxelizing. When false, tubes are open at both ends.
    /// Closed bark is reliable at full detail and moderately simplified
    /// detail; the lower mesh ([`LowerDetail::lower`]) turns it off because
    /// its coarse tubes may fold over themselves at sharp bends.
    ///
    /// Each shell is closed once vertices at identical positions are merged:
    /// the texture seam and the join between a branch and its continuation repeat
    /// positions as separate vertices. Merge vertices with bitwise-equal
    /// positions before building a physics mesh that needs shared edges.
    /// Cap triangles reuse the end rings' vertices and lighting directions, so a
    /// visible end, such as a trunk base above ground, looks rounded rather than flat.
    pub closed: bool,
    /// Minimum branch radius at the base, in metres. Thinner branches are
    /// omitted, but the trunk is always kept. This does not affect leaf selection.
    /// Omitted branches still have an entry in [`TreeMesh::branch_spans`], with
    /// empty buffer ranges. Set to 0 to keep all branches.
    pub min_branch_radius: f32,
    /// Keep every Nth leaf on each branch, starting with the leaf nearest its tip
    /// and counting toward the base. For example, 2 keeps the last leaf, third-last
    /// leaf, and so on. Both 0 and 1 keep all leaves.
    pub leaf_stride: u32,
    /// Scale the stored width and height of each retained leaf card (a textured square).
    /// The card keeps the original leaf's tip edge, so enlargement extends toward
    /// the stem and the card never reaches past where the leaf ended. Use values above
    /// 1 to fill gaps between fewer leaves.
    pub leaf_scale: f32,
    /// Choose one textured square per leaf or two crossed squares.
    /// `None` uses [`LeafOptions::billboard`](crate::LeafOptions::billboard).
    pub billboard: Option<Billboard>,
    /// Keep every Nth fruit cluster and every Nth flower cluster, starting
    /// with cluster 0 in each skeleton list. Both 0 and 1 keep all clusters.
    pub cluster_stride: u32,
    /// Scale each kept cluster, stalks included. Values above 1 help fill
    /// the gaps left by skipped clusters.
    pub cluster_scale: f32,
    /// Build each kept cluster in [`TreeMesh::fruit`] and [`TreeMesh::flowers`]
    /// at its own lower mesh rather than its
    /// [`game_level`](procgena_fruit::FruitOptions::game_level). The lower
    /// mesh of a tree sets it.
    pub lower_clusters: bool,
}

impl Default for LodDetail {
    fn default() -> Self {
        Self {
            section_stride: 1,
            segment_factor: 1.0,
            closed: true,
            min_branch_radius: 0.0,
            leaf_stride: 1,
            leaf_scale: 1.0,
            billboard: None,
            cluster_stride: 1,
            cluster_scale: 1.0,
            lower_clusters: false,
        }
    }
}

impl LowerDetail for LodDetail {
    /// The tree's coarser mesh, relative to the game level:
    ///
    /// - three times the section stride and half the tube sides, with open
    ///   tube ends, since coarse tubes may fold over themselves at sharp
    ///   bends;
    /// - every second kept leaf, as a single card 1.3 times as large;
    /// - every second kept fruit and flower cluster, 1.12 times as large,
    ///   each built at its own lower mesh. Small clusters are kept too,
    ///   since on some trees, such as Spring Pussy Willow, they make the
    ///   canopy.
    fn lower(&self) -> Self {
        /// With the 1.25 times larger items of a cluster's own lower mesh,
        /// kept cluster items are 1.4 times their size.
        const CLUSTER_SCALE: f32 = 1.12;

        Self {
            section_stride: self.section_stride.max(1) * 3,
            segment_factor: self.segment_factor * 0.5,
            closed: false,
            leaf_stride: self.leaf_stride.max(1) * 2,
            leaf_scale: self.leaf_scale * 1.3,
            billboard: Some(Billboard::Single),
            cluster_stride: self.cluster_stride.max(1) * 2,
            cluster_scale: self.cluster_scale * CLUSTER_SCALE,
            lower_clusters: true,
            ..*self
        }
    }
}

/// Vertex data and triangle indices for one mesh.
///
/// Meshes from [`Skeleton::mesh`] have one texture-coordinate pair and one
/// lighting direction (normal) per position.
/// When editing buffers, keep populated vertex attributes in position order and
/// ensure every triangle index refers to a valid position.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshBuffers {
    /// Vertex coordinates in the same coordinate system as the skeleton.
    pub positions: Vec<[f32; 3]>,
    /// Unit lighting directions in position order.
    pub normals: Vec<[f32; 3]>,
    /// Per-vertex texture coordinates `(u, v)`, in position order.
    /// Leaf coordinates cover 0 to 1; bark coordinates can exceed 1 for repeating textures.
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex directions that orient normal-map lighting detail on the surface.
    /// Bark and leaves have one `[x, y, z, w]` entry per position; fruit and flowers
    /// have none. `[x, y, z]` is a unit tangent perpendicular to the vertex normal.
    /// The sign `w` gives a second direction, the bitangent, as `cross(normal, xyz) * w`.
    ///
    /// On bark, the tangent points around the branch toward increasing texture `u`,
    /// and the bitangent follows increasing `v`. Use [`BarkUvMode::Continuous`] for
    /// normal maps. These directions approximate a smooth surface and may differ
    /// from triangle texture directions at sharp bends or twists.
    ///
    /// On leaves, the tangent runs across the first square toward increasing `u`,
    /// adjusted to remain perpendicular to the lighting normal. The bitangent points
    /// toward the tip, or perpendicular to it for some rounded normals. Both squares
    /// of a double billboard use the first square's reference directions, matching
    /// the shared face normal; lighting on the second square does not follow its rotation.
    pub tangents: Vec<[f32; 4]>,
    /// Animation data for each vertex, in the same order as [`positions`](Self::positions).
    /// Pass each four-float value to `wind_offset` in [`WIND_WESL`](crate::WIND_WESL).
    ///
    /// The values are `[trunk_bend, branch_weight, branch_phase, flutter]`:
    /// - `trunk_bend` controls trunk movement, from 0 at ground level to 1 at the
    ///   highest skeleton section.
    /// - `branch_weight` controls branch movement, in metres before applying
    ///   the wind settings. It is 0 on the trunk and increases toward branch tips.
    /// - `branch_phase` is a timing offset in `[0, 1)`. Each main limb shares it
    ///   with its descendants so they sway together.
    /// - `flutter` controls leaf movement: 0 for bark and leaf bases, 1 for leaf tips.
    ///
    /// Leaves inherit the first three values from their attachment points to follow
    /// their branches. Reducing mesh detail preserves the values at retained vertices.
    pub wind: Vec<[f32; 4]>,
    /// Per-vertex `[red, green, blue, alpha]` colours in linear colour space,
    /// ready for lighting. Fruit and flowers have one colour per position;
    /// multiply these by the material’s base colour when rendering. Bark and
    /// leaves have an empty buffer and use their textures instead.
    pub colors: Vec<[f32; 4]>,
    /// Groups of three vertex indices, one group per triangle.
    /// Each index addresses `positions`, `normals`, and `uvs`. Vertices are ordered
    /// counter-clockwise when viewed from the front of a triangle.
    pub indices: Vec<u32>,
}

impl MeshBuffers {
    /// Returns the number of positions in this mesh, including duplicated vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Returns the number of complete groups of three indices.
    /// Any trailing indices in a manually edited buffer are not counted.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Returns the index for the next vertex, panicking if it cannot fit in `u32`.
    pub(crate) fn next_index(&self) -> u32 {
        u32::try_from(self.positions.len()).expect("mesh has more than u32::MAX vertices")
    }

    fn push(&mut self, position: Vec3, normal: Vec3, uv: [f32; 2], wind: [f32; 4]) {
        self.positions.push(position.to_array());
        self.normals.push(normal.to_array());
        self.uvs.push(uv);
        self.wind.push(wind);
    }

    /// Appends a position, lighting normal, texture coordinates, wind values,
    /// and linear RGBA colour. Use for buffers with one colour per vertex;
    /// does not add triangle indices or tangents.
    pub(crate) fn push_colored(
        &mut self,
        position: Vec3,
        normal: Vec3,
        uv: [f32; 2],
        wind: [f32; 4],
        color: [f32; 4],
    ) {
        self.push(position, normal, uv, wind);
        self.colors.push(color);
    }
}

/// Bark, leaf, fruit, and flower meshes for a skeleton, with ranges identifying
/// the source branches, leaves, and clusters. Detail settings affect bark and
/// leaves; fruit and flowers retain their full detail.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeMesh {
    /// All branches as tubes, ready for a bark material. Tubes are closed or
    /// open according to [`LodDetail::closed`].
    pub branches: MeshBuffers,
    /// All leaves retained by the detail settings, ready for a leaf material.
    pub leaves: MeshBuffers,
    /// The kept fruit clusters, each built at its own
    /// [`game_level`](procgena_fruit::FruitOptions::game_level) (or its lower
    /// mesh with [`LodDetail::lower_clusters`]) and moved into place, with
    /// per-vertex colours. Empty if the tree grows no fruit. Render both
    /// sides, since stalk strips and flat fruit have no thickness.
    pub fruit: MeshBuffers,
    /// The kept flower clusters, as for [`fruit`](Self::fruit). Render both
    /// sides of petals, since they have no thickness.
    pub flowers: MeshBuffers,
    /// Buffer ranges for each skeleton branch, in the order of [`Skeleton::branches`].
    pub branch_spans: Vec<BranchSpan>,
    /// Buffer ranges for retained leaves, in the order of [`Skeleton::leaves`].
    pub leaf_spans: Vec<LeafSpan>,
    /// Ranges in [`fruit`](Self::fruit), one per kept cluster, in the order
    /// of [`Skeleton::fruit`].
    pub fruit_spans: Vec<ClusterSpan>,
    /// Ranges in [`flowers`](Self::flowers), one per kept cluster, in the
    /// order of [`Skeleton::flowers`].
    pub flower_spans: Vec<ClusterSpan>,
}

impl TreeMesh {
    /// Returns the total vertex count, including duplicates.
    pub fn vertex_count(&self) -> usize {
        self.branches.vertex_count()
            + self.leaves.vertex_count()
            + self.fruit.vertex_count()
            + self.flowers.vertex_count()
    }

    /// Returns the total triangle count. Incomplete trailing triangles in
    /// manually edited buffers are not counted.
    pub fn triangle_count(&self) -> usize {
        self.branches.triangle_count()
            + self.leaves.triangle_count()
            + self.fruit.triangle_count()
            + self.flowers.triangle_count()
    }

    /// The diameter, in metres, of a sphere holding every vertex: the
    /// diagonal of the box around the bark, leaves, fruit and flowers. A
    /// tree's size on screen is measured by it. 0 for an empty mesh.
    pub fn bounding_diameter(&self) -> f32 {
        let (min, max) = [&self.branches, &self.leaves, &self.fruit, &self.flowers]
            .into_iter()
            .flat_map(|buffers| &buffers.positions)
            .map(|p| Vec3::from_array(*p))
            .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)));
        if min.cmple(max).all() {
            min.distance(max)
        } else {
            0.0
        }
    }
}

/// Identifies a skeleton branch and its ranges in the bark buffers.
///
/// Vertices are grouped by retained section, from base to tip. Each group is
/// a circular ring of [`ring_vertices`](Self::ring_vertices) vertices, with a
/// repeated position at the texture seam. Adjacent rings use `segments * 2`
/// triangles.
///
/// [`LodDetail::closed`] tubes add triangles but no vertices. A tip that tapers
/// to a point uses `segments` triangles for its last band instead, all meeting at
/// the tip ring's first vertex. Each sealed end then adds `segments - 2` triangles
/// after the sides, spanning that end's ring.
///
/// Branches omitted by [`LodDetail::min_branch_radius`] have empty ranges and
/// no retained sections.
#[derive(Debug, Clone, PartialEq)]
pub struct BranchSpan {
    /// Source branch index in [`Skeleton::branches`].
    pub branch: usize,
    /// Branching depth copied from the skeleton; the trunk is level 0.
    pub level: u32,
    /// Start-inclusive, end-exclusive range in the bark position, normal, and UV buffers.
    pub vertices: Range<u32>,
    /// Start-inclusive, end-exclusive range of entries in the bark index buffer.
    /// These are index-buffer offsets, not triangle numbers or vertex indices.
    pub indices: Range<u32>,
    /// Number of sides around the tube, excluding the duplicated seam vertex.
    pub segments: u32,
    /// Retained section indices within the source branch, in ring order from base to tip.
    pub sections: Vec<usize>,
}

impl BranchSpan {
    /// Returns the vertices per section: `segments + 1`. An omitted branch
    /// reports the same layout even though its ranges are empty.
    pub fn ring_vertices(&self) -> u32 {
        self.segments + 1
    }
}

/// Buffer ranges for one retained leaf in [`TreeMesh::leaves`].
#[derive(Debug, Clone, PartialEq)]
pub struct LeafSpan {
    /// Source leaf index in [`Skeleton::leaves`], before any leaves were skipped.
    pub leaf: usize,
    /// Start-inclusive, end-exclusive range in the leaf vertex buffers.
    /// A card uses 4 vertices; crossed cards use 8.
    pub vertices: Range<u32>,
    /// Entry range in the leaf index buffer, including the start and excluding
    /// the end. Each card uses 6 entries (two triangles). These are buffer
    /// offsets, not vertex indices.
    pub indices: Range<u32>,
}

/// Locates one kept cluster's fruit or flowers and supporting stalks in the
/// corresponding [`TreeMesh`] buffers. Ranges can be empty.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterSpan {
    /// Index of the source cluster in [`Skeleton::fruit`] for fruit spans,
    /// or [`Skeleton::flowers`] for flower spans.
    pub cluster: usize,
    /// Vertex offsets shared by the position, normal, UV, wind, and colour
    /// buffers of the corresponding fruit or flower mesh. Includes the start
    /// offset and excludes the end offset.
    pub vertices: Range<u32>,
    /// Entry offsets in the corresponding fruit or flower index buffer,
    /// including the start and excluding the end. Each triangle uses three
    /// entries; these offsets are not triangle numbers or vertex indices.
    pub indices: Range<u32>,
}

impl Skeleton {
    /// Builds bark, leaf, fruit, and flower triangle meshes from this skeleton.
    ///
    /// Pass the options used to generate the skeleton and [`LodDetail::default()`]
    /// for full detail. Changing growth settings here does not reshape
    /// branches; generate a new skeleton for that. Fruit and flowers are the
    /// cluster specimens grown with the skeleton ([`Skeleton::fruit_slots`]),
    /// so their settings here are ignored; generate a new skeleton to change
    /// them.
    ///
    /// Bark texture scale and UV mode, leaf card and normal settings also
    /// affect the result. A card-count override in `detail` takes precedence.
    /// Use `detail` to reduce geometry for bark, leaves, fruit, and flowers.
    /// Each fruit or flower span identifies its original skeleton cluster,
    /// even when the detail settings skip other clusters.
    /// Repeated calls with identical inputs give identical meshes
    /// and leave the skeleton unchanged.
    ///
    /// Settings are not validated here; pass validated options. With rounded
    /// normals on double leaf cards, one corner’s lighting direction flips
    /// as the scaled leaf size passes 0.2 m; at exactly that size it uses the
    /// face normal.
    ///
    /// # Panics
    ///
    /// Panics if a branch has no sections; [`Skeleton::generate`] supplies them.
    /// Also panics if a vertex count or index-buffer length cannot fit in `u32`.
    pub fn mesh(&self, options: &TreeOptions, detail: &LodDetail) -> TreeMesh {
        let section_stride = detail.section_stride.max(1) as usize;
        let leaf_stride = detail.leaf_stride.max(1) as usize;
        let billboard = detail.billboard.unwrap_or(options.leaves.billboard);

        let winds = branch_winds(self);
        let mut mesh = TreeMesh::default();

        // Decide each branch's side count, whether it is drawn, and which tube ends to seal.
        let shapes: Vec<Shape> = self
            .branches
            .iter()
            .map(|b| Shape::new(detail, b))
            .collect();
        let kept: Vec<bool> = self
            .branches
            .iter()
            .map(|b| b.level == 0 || b.base_radius >= detail.min_branch_radius)
            .collect();
        let caps = tube_caps(self, &shapes, &kept, detail.closed);

        for (index, branch) in self.branches.iter().enumerate() {
            let shape = shapes[index];
            let span = if !kept[index] {
                culled_branch(&mesh.branches, branch, index, shape)
            } else {
                mesh_branch(
                    &mut mesh.branches,
                    branch,
                    index,
                    section_stride,
                    shape,
                    caps[index],
                    &options.bark,
                    &winds[index],
                )
            };
            mesh.branch_spans.push(span);
        }
        let style = LeafStyle {
            scale: detail.leaf_scale,
            billboard,
            rounded_normals: options.leaves.rounded_normals,
        };
        let kept = kept_leaves(&self.leaves, leaf_stride);
        for (index, leaf) in self.leaves.iter().enumerate().filter(|(i, _)| kept[*i]) {
            let span = mesh_leaf(
                &mut mesh.leaves,
                leaf,
                index,
                &style,
                wind_at(&winds, &leaf.parent),
            );
            mesh.leaf_spans.push(span);
        }
        // Fruit and flowers: each kept cluster's specimen, moved into place.
        mesh.fruit_spans = compose_clusters(
            &mut mesh.fruit,
            &self.fruit,
            &self.fruit_slots,
            &winds,
            detail,
        );
        mesh.flower_spans = compose_clusters(
            &mut mesh.flowers,
            &self.flowers,
            &self.flower_slots,
            &winds,
            detail,
        );
        mesh
    }
}

/// The number of sides of one branch's tube.
#[derive(Debug, Clone, Copy)]
struct Shape {
    /// Number of tube sides.
    segments: u32,
}

impl Shape {
    /// Scales the branch's side count by `detail`, keeping at least three
    /// sides.
    fn new(detail: &LodDetail, branch: &SkeletonBranch) -> Self {
        // Tip continuations have the same original side count as their parent,
        // so scaling both counts equally keeps their connecting rings aligned.
        Self {
            segments: (branch.segment_count as f32 * detail.segment_factor)
                .round()
                .max(3.0) as u32,
        }
    }
}

/// Which ends of a tube branch to seal with triangles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Caps {
    /// Seal the base ring.
    base: bool,
    /// Seal the tip ring. A tip with zero radius instead closes by fanning the
    /// last band into a point.
    tip: bool,
}

/// Chooses which tube ends to seal for each branch in `skeleton`. With
/// `closed` false, no end is sealed. Otherwise every end of a kept tube is
/// sealed, except where a kept child tube continues from its parent's tip
/// with an identical ring: those two ends join without caps. `shapes` and
/// `kept` hold each branch's side count and whether it is drawn.
fn tube_caps(skeleton: &Skeleton, shapes: &[Shape], kept: &[bool], closed: bool) -> Vec<Caps> {
    let tube = |i: usize| kept[i];
    let mut caps: Vec<Caps> = (0..skeleton.branches.len())
        .map(|i| Caps {
            base: closed && tube(i),
            tip: closed && tube(i),
        })
        .collect();
    if !closed {
        return caps;
    }

    // A continuation starts exactly at its parent's last section, with the same
    // side count, so both rings have identical vertex positions.
    for (child, branch) in skeleton.branches.iter().enumerate() {
        let Some(at) = branch.parent else { continue };
        let parent = &skeleton.branches[at.branch];
        let last = parent.sections.len() - 1;
        let joins = tube(child)
            && tube(at.branch)
            && at.section == last
            && at.t == 0.0
            && shapes[child].segments == shapes[at.branch].segments
            && branch.sections.first() == parent.sections.last();
        if joins {
            caps[child].base = false;
            caps[at.branch].tip = false;
        }
    }
    caps
}

/// Returns an empty span for an omitted branch, keeping its source index and
/// side count. The buffer remains unchanged.
fn culled_branch(
    out: &MeshBuffers,
    branch: &SkeletonBranch,
    index: usize,
    shape: Shape,
) -> BranchSpan {
    let vertex = out.next_index();
    BranchSpan {
        branch: index,
        level: branch.level,
        vertices: vertex..vertex,
        indices: index_range(out.indices.len(), out.indices.len()),
        segments: shape.segments,
        sections: Vec::new(),
    }
}

/// Adds a branch's tube to `out` and returns its ranges and retained section
/// indices, sealing the ends chosen in `caps`. Pass a branch with at least
/// one section, a nonzero `section_stride`, and wind data for its sections.
#[allow(clippy::too_many_arguments)]
fn mesh_branch(
    out: &mut MeshBuffers,
    branch: &SkeletonBranch,
    index: usize,
    section_stride: usize,
    shape: Shape,
    caps: Caps,
    bark: &BarkOptions,
    wind: &BranchWind,
) -> BranchSpan {
    let segments = shape.segments;

    // Repeat the bark image more often around wider branches. Whole repeats
    // keep the texture continuous across a tube seam.
    let wraps_x = (branch.base_radius * bark.texture_scale.x).round().max(1.0);

    // Include the branch tip even if it falls between stride steps.
    let last = branch.sections.len() - 1;
    let mut kept: Vec<usize> = (0..=last).step_by(section_stride).collect();
    if !last.is_multiple_of(section_stride) {
        kept.push(last);
    }

    let (first_vertex, first_index) = (out.next_index(), out.indices.len());
    for (k, &s) in kept.iter().enumerate() {
        let section = &branch.sections[s];
        let v = match bark.uv_mode {
            // Alternate texture direction between retained sections.
            BarkUvMode::Mirrored => (k % 2) as f32,
            // Preserve texture placement along the branch when sections are skipped.
            BarkUvMode::Continuous => s as f32,
        };
        // Move all vertices at this section together, without leaf flutter.
        let ring_wind = [wind.bend[s], wind.weight[s], wind.phase, 0.0];
        push_tube_ring(out, section, segments, wraps_x, v, ring_wind);
    }

    // Join neighbouring sections with two triangles per tube side.
    let span = BranchSpan {
        branch: index,
        level: branch.level,
        vertices: first_vertex..first_vertex,
        indices: 0..0,
        segments,
        sections: kept,
    };
    let ring = span.ring_vertices();
    let rings = span.sections.len() as u32;
    let point_tip = caps.tip && branch.sections[last].radius == 0.0;
    if point_tip && rings >= 2 {
        // Close a tip that tapers to a point with one triangle per side.
        connect_rings(out, first_vertex, rings - 1, ring, segments);
        let previous = first_vertex + (rings - 2) * ring;
        fan_to_point(out, previous, previous + ring, segments);
    } else {
        connect_rings(out, first_vertex, rings, ring, segments);
    }

    // Seal open ends across their rings. Bases face away from the branch.
    let tip_ring = first_vertex + (rings - 1) * ring;
    if caps.base && branch.sections[0].radius > 0.0 {
        cap_ring(out, first_vertex, segments, false);
    }
    if caps.tip && !point_tip {
        cap_ring(out, tip_ring, segments, true);
    }

    BranchSpan {
        vertices: first_vertex..out.next_index(),
        indices: index_range(first_index, out.indices.len()),
        ..span
    }
}

/// Joins successive rings with two triangles per side. `first` is the starting
/// vertex, `rings` is the ring count, and `ring` is the vertices per ring. Each
/// side joins neighbouring vertices within a ring to those in the next ring.
fn connect_rings(out: &mut MeshBuffers, first: u32, rings: u32, ring: u32, sides: u32) {
    for i in 0..rings.saturating_sub(1) {
        for j in 0..sides {
            let v1 = first + i * ring + j;
            let v2 = v1 + 1;
            let v3 = v1 + ring;
            let v4 = v2 + ring;
            out.indices.extend_from_slice(&[v1, v3, v2, v2, v3, v4]);
        }
    }
}

/// Joins the tube ring starting at vertex `ring` to the single point at vertex
/// `point`, with one outward-facing triangle per side. `ring` must have
/// `segments + 1` vertices, the last repeating the first position.
fn fan_to_point(out: &mut MeshBuffers, ring: u32, point: u32, segments: u32) {
    for j in 0..segments {
        out.indices
            .extend_from_slice(&[ring + j, point, ring + j + 1]);
    }
}

/// Covers the tube ring starting at vertex `first` with `segments - 2` triangles
/// fanned from its first vertex. With `facing_tip`, the triangles face along the
/// branch toward its tip; otherwise they face back toward its base. `segments`
/// must be at least 3. The repeated seam vertex is not used.
fn cap_ring(out: &mut MeshBuffers, first: u32, segments: u32, facing_tip: bool) {
    for j in 1..segments - 1 {
        let (a, b) = (first + j, first + j + 1);
        let triangle = if facing_tip {
            [first, b, a]
        } else {
            [first, a, b]
        };
        out.indices.extend_from_slice(&triangle);
    }
}

/// Appends a tube cross-section with `segments` sides and a repeated first vertex.
/// The repeated position has a different texture coordinate so the bark image
/// can wrap around the tube. `segments` must be nonzero.
fn push_tube_ring(
    out: &mut MeshBuffers,
    section: &Section,
    segments: u32,
    wraps_x: f32,
    v: f32,
    ring_wind: [f32; 4],
) {
    let mut seam = None;
    for j in 0..segments {
        let angle = TAU * j as f32 / segments as f32;
        let (sin, cos) = angle.sin_cos();
        let direction = section.orientation * Vec3::new(cos, 0.0, sin);
        let position = direction * section.radius + section.origin;
        let normal = direction.normalize();
        out.push(
            position,
            normal,
            [j as f32 / segments as f32 * wraps_x, v],
            ring_wind,
        );
        // The tangent points around the tube in the direction of increasing u.
        // Its -1 handedness makes the bitangent point toward the branch tip,
        // including where mirrored texture coordinates reverse v.
        let tangent = (section.orientation * Vec3::new(-sin, 0.0, cos)).normalize();
        out.tangents.push(tangent.extend(-1.0).to_array());
        seam.get_or_insert((position, normal, tangent));
    }
    let (position, normal, tangent) = seam.expect("a ring has at least 3 vertices");
    out.push(position, normal, [wraps_x, v], ring_wind);
    out.tangents.push(tangent.extend(-1.0).to_array());
}

/// Size, card count and lighting settings shared by the mesh’s leaves.
struct LeafStyle {
    /// Size multiplier for the cards.
    scale: f32,
    /// One card or a pair crossed around the leaf stem.
    billboard: Billboard,
    /// Whether cards use softened canopy lighting.
    rounded_normals: bool,
}

/// Adds one leaf card or a crossed pair to `out` and returns its buffer
/// ranges, recording `index` as the source leaf. Cards have per-corner normals
/// and tangents.
/// Pass attachment wind values as `[trunk_bend, branch_weight, branch_phase]`.
fn mesh_leaf(
    out: &mut MeshBuffers,
    leaf: &SkeletonLeaf,
    index: usize,
    style: &LeafStyle,
    wind: [f32; 3],
) -> LeafSpan {
    // Keep the leaf's tip edge where it is, so an enlarged card grows toward
    // the stem and never past the crown's edge.
    let half = leaf.size * style.scale / 2.0;
    let centre = leaf.size - half;
    let (first_vertex, first_index) = (out.next_index(), out.indices.len());

    // Cross the second card at a right angle around the leaf stem.
    let turns: &[f32] = match style.billboard {
        Billboard::Single => &[0.0],
        Billboard::Double => &[0.0, FRAC_PI_2],
    };
    for &turn in turns {
        let rotation = leaf.orientation * Quat::from_rotation_y(turn);
        let corners = [
            Vec3::new(-half, centre + half, 0.0),
            Vec3::new(-half, centre - half, 0.0),
            Vec3::new(half, centre - half, 0.0),
            Vec3::new(half, centre + half, 0.0),
        ]
        .map(|corner| rotation * corner + leaf.origin);

        // Fixed crossed cards share the first card’s lighting direction, matching
        // ez-tree. Rounded normals tilt it toward each corner for softer canopy lighting.
        let face = leaf.orientation * Vec3::Z;
        let across = leaf.orientation * Vec3::X;
        let tip = leaf.orientation * Vec3::Y;
        let uvs = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        // Apply flutter at the tip corners only; all corners still follow branch motion.
        let flutter = [1.0, 0.0, 0.0, 1.0];

        let i = out.next_index();
        for ((corner, uv), flutter) in corners.into_iter().zip(uvs).zip(flutter) {
            // Use the face direction when rounding would produce a zero-length normal.
            let normal = if style.rounded_normals {
                (face * LEAF_NORMAL_REFERENCE + corner - leaf.origin).normalize_or(face)
            } else {
                face
            };
            out.push(corner, normal, uv, [wind[0], wind[1], wind[2], flutter]);

            // Align the normal-map tangent with the first card’s width. Choose the
            // bitangent sign so its direction does not point away from the leaf tip.
            let tangent = (across - normal * normal.dot(across)).normalize();
            let w = if normal.cross(tangent).dot(tip) < 0.0 {
                -1.0
            } else {
                1.0
            };
            out.tangents.push(tangent.extend(w).to_array());
        }
        out.indices
            .extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    }

    LeafSpan {
        leaf: index,
        vertices: first_vertex..out.next_index(),
        indices: index_range(first_index, out.indices.len()),
    }
}

/// Marks retained leaves in input order. Within each branch, keep every
/// `stride`th leaf starting nearest the tip. Strides 0 and 1 keep all leaves.
fn kept_leaves(leaves: &[SkeletonLeaf], stride: usize) -> Vec<bool> {
    let mut kept = vec![stride <= 1; leaves.len()];
    if stride <= 1 {
        return kept;
    }

    // Sort each branch’s leaves from tip to base before applying the stride.
    let mut order: Vec<usize> = (0..leaves.len()).collect();
    let along = |i: usize| leaves[i].parent.section as f32 + leaves[i].parent.t;
    order.sort_by(|&a, &b| {
        let (pa, pb) = (&leaves[a].parent, &leaves[b].parent);
        pa.branch
            .cmp(&pb.branch)
            .then(along(b).total_cmp(&along(a)))
            .then(a.cmp(&b))
    });
    for group in order.chunk_by(|&a, &b| leaves[a].parent.branch == leaves[b].parent.branch) {
        for &i in group.iter().step_by(stride) {
            kept[i] = true;
        }
    }
    kept
}

/// Converts buffer offsets to a half-open range, panicking if either offset exceeds `u32::MAX`.
fn index_range(start: usize, end: usize) -> Range<u32> {
    let convert = |i: usize| u32::try_from(i).expect("index buffer longer than u32::MAX");
    convert(start)..convert(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Section;

    fn straight_branch(sections: usize, segment_count: u32) -> SkeletonBranch {
        SkeletonBranch {
            level: 0,
            parent: None,
            sections: (0..sections)
                .map(|i| Section {
                    origin: Vec3::new(0.0, i as f32, 0.0),
                    orientation: Quat::IDENTITY,
                    radius: 1.0,
                })
                .collect(),
            segment_count,
            base_radius: 1.0,
        }
    }

    /// Gives each section distinct test values so incorrect wind-data indexing is visible.
    fn wind(branch: &SkeletonBranch) -> BranchWind {
        let n = branch.sections.len();
        BranchWind {
            bend: (0..n).map(|i| i as f32 / 10.0).collect(),
            weight: (0..n).map(|i| i as f32).collect(),
            phase: 0.5,
        }
    }

    /// Builds test settings with the requested texture scale around the branch
    /// and texture direction along it. Other settings keep their defaults.
    fn bark(texture_scale_x: f32, uv_mode: BarkUvMode) -> BarkOptions {
        let mut bark = BarkOptions {
            uv_mode,
            ..BarkOptions::default()
        };
        bark.texture_scale.x = texture_scale_x;
        bark
    }

    /// Creates a tube shape for `branch` using the requested side-count multiplier.
    fn tube(branch: &SkeletonBranch, segment_factor: f32) -> Shape {
        let detail = LodDetail {
            segment_factor,
            ..LodDetail::default()
        };
        Shape::new(&detail, branch)
    }

    #[test]
    fn stride_keeps_first_and_last_sections() {
        let branch = straight_branch(8, 4);
        let mirrored = bark(1.0, BarkUvMode::Mirrored);
        let mut out = MeshBuffers::default();
        let shape = tube(&branch, 1.0);
        let span = mesh_branch(
            &mut out,
            &branch,
            0,
            3,
            shape,
            Caps::default(),
            &mirrored,
            &wind(&branch),
        );
        assert_eq!(span.sections, vec![0, 3, 6, 7]);
        assert_eq!(span.vertices, 0..4 * 5);
        assert_eq!(span.indices, 0..3 * 4 * 6);
        // All five vertices of the second retained ring, including its seam
        // duplicate, must use section 3’s wind values.
        assert_eq!(out.wind[5..10], [[0.3, 3.0, 0.5, 0.0]; 5]);

        // A tip already selected by the stride must not be duplicated.
        let short = straight_branch(7, 4);
        let shape = tube(&short, 1.0);
        let span = mesh_branch(
            &mut out,
            &short,
            1,
            3,
            shape,
            Caps::default(),
            &mirrored,
            &wind(&short),
        );
        assert_eq!(span.sections, vec![0, 3, 6]);
        assert_eq!(span.vertices, 20..35);
        assert_eq!(out.tangents.len(), out.positions.len());
        assert_eq!(out.wind.len(), out.positions.len());
    }

    #[test]
    fn segments_never_drop_below_three() {
        let mut out = MeshBuffers::default();
        let mirrored = bark(1.0, BarkUvMode::Mirrored);
        let branch = straight_branch(2, 5);
        let shape = tube(&branch, 0.1);
        let span = mesh_branch(
            &mut out,
            &branch,
            0,
            1,
            shape,
            Caps::default(),
            &mirrored,
            &wind(&branch),
        );
        assert_eq!(span.segments, 3);
    }

    /// Creates fixed leaf cards with the requested size, card count and lighting.
    fn fixed_leaves(scale: f32, billboard: Billboard, rounded_normals: bool) -> LeafStyle {
        LeafStyle {
            scale,
            billboard,
            rounded_normals,
        }
    }

    /// Checks texture coordinates along a branch when some mesh rings are skipped.
    #[test]
    fn uv_modes_set_v_per_ring() {
        let branch = straight_branch(8, 4);
        let ring_v = |mode| {
            let mut out = MeshBuffers::default();
            mesh_branch(
                &mut out,
                &branch,
                0,
                3,
                tube(&branch, 1.0),
                Caps::default(),
                &bark(1.0, mode),
                &wind(&branch),
            );
            out.uvs
                .iter()
                .step_by(5)
                .map(|uv| uv[1])
                .collect::<Vec<_>>()
        };
        // Only sections 0, 3, 6, and 7 remain. Mirrored coordinates alternate;
        // continuous coordinates keep the original section numbers.
        assert_eq!(ring_v(BarkUvMode::Mirrored), [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(ring_v(BarkUvMode::Continuous), [0.0, 3.0, 6.0, 7.0]);
    }

    #[test]
    fn ring_seam_repeats_first_vertex() {
        let mut out = MeshBuffers::default();
        let branch = straight_branch(2, 4);
        mesh_branch(
            &mut out,
            &branch,
            0,
            1,
            tube(&branch, 1.0),
            Caps::default(),
            &bark(2.6, BarkUvMode::Mirrored),
            &wind(&branch),
        );
        // A base radius of 1 and texture scale of 2.6 give three full texture repeats.
        assert_eq!(out.positions[4], out.positions[0]);
        assert_eq!(out.uvs[0], [0.0, 0.0]);
        assert_eq!(out.uvs[4], [3.0, 0.0]);
        assert_eq!(out.uvs[5], [0.0, 1.0]);
        // Check that the first triangle faces outward using its vertex order.
        let p = |i: u32| Vec3::from_array(out.positions[i as usize]);
        let [a, b, c] = [out.indices[0], out.indices[1], out.indices[2]];
        let facing = (p(b) - p(a)).cross(p(c) - p(a));
        assert!(facing.dot(Vec3::from_array(out.normals[a as usize])) > 0.0);
    }

    #[test]
    fn double_billboard_has_two_quads() {
        let leaf = SkeletonLeaf {
            parent: crate::Attachment {
                branch: 0,
                section: 0,
                t: 0.0,
            },
            origin: Vec3::new(0.1, 0.2, 0.3),
            orientation: Quat::IDENTITY,
            size: 0.2,
        };
        let mut out = MeshBuffers::default();
        let span = mesh_leaf(
            &mut out,
            &leaf,
            7,
            &fixed_leaves(0.5, Billboard::Double, false),
            [0.2, 1.5, 0.25],
        );
        assert_eq!(span.vertices, 0..8);
        // Both cards follow their branch attachment. Only the two tip corners
        // of each card receive flutter.
        assert_eq!(out.wind[0], [0.2, 1.5, 0.25, 1.0]);
        assert_eq!(out.wind[1], [0.2, 1.5, 0.25, 0.0]);
        let flutter: Vec<f32> = out.wind.iter().map(|w| w[3]).collect();
        assert_eq!(flutter, [1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0]);
        assert_eq!(span.indices, 0..12);
        // Halving the card keeps the leaf's tip edge at y = 0.4, 0.2 m above
        // its origin.
        let first = Vec3::from_array(out.positions[0]);
        assert!(
            first.abs_diff_eq(Vec3::new(0.05, 0.4, 0.3), 1e-6),
            "{first}"
        );
        // The crossed card lies at a right angle around the +Y stem direction.
        let turned = Vec3::from_array(out.positions[4]);
        assert!(
            turned.abs_diff_eq(Vec3::new(0.1, 0.4, 0.35), 1e-6),
            "{turned}"
        );
        // Both fixed cards use the same face normal, as in ez-tree.
        assert_eq!(out.normals[4], [0.0, 0.0, 1.0]);
    }

    /// Checks the directions used for leaf normal maps with single and crossed squares.
    /// Tangents must be unit length, perpendicular to the normals, and point across
    /// the first square. Bitangents must not point away from the tip; with unrounded
    /// normals, they point directly toward it.
    #[test]
    fn leaf_tangents_form_a_frame_toward_u_and_the_tip() {
        let orientation = Quat::from_euler(glam::EulerRot::XYZ, 0.3, -0.7, 1.1);
        let leaf = SkeletonLeaf {
            parent: crate::Attachment {
                branch: 0,
                section: 0,
                t: 0.0,
            },
            origin: Vec3::new(0.1, 0.2, 0.3),
            orientation,
            size: 0.15,
        };
        let across = orientation * Vec3::X;
        let tip = orientation * Vec3::Y;
        for billboard in [Billboard::Single, Billboard::Double] {
            for rounded in [false, true] {
                // Exercise rounded lighting at leaf sizes below, at, and above 0.2 m.
                // At 0.2 m, one corner needs the fallback normal and another points
                // toward the tip. Larger sizes can tilt normals past the tip.
                for scale in [1.0, 2.0 / 1.5, 2.0] {
                    let mut out = MeshBuffers::default();
                    let style = fixed_leaves(scale, billboard, rounded);
                    mesh_leaf(&mut out, &leaf, 0, &style, [0.0; 3]);
                    assert_eq!(out.tangents.len(), out.positions.len());
                    for (t, n) in out.tangents.iter().zip(&out.normals) {
                        let (tangent, n) = (Vec3::from_slice(t), Vec3::from_array(*n));
                        let case = format!("{billboard:?} rounded={rounded} scale={scale}");
                        assert!((tangent.length() - 1.0).abs() < 1e-5, "{case}: {tangent}");
                        assert!(tangent.dot(n).abs() < 1e-5, "{case}: {tangent} vs {n}");
                        assert!(t[3] == 1.0 || t[3] == -1.0, "{case}: {t:?}");
                        assert!(tangent.dot(across) > 0.0, "{case}: {tangent}");
                        let bitangent = n.cross(tangent) * t[3];
                        if rounded {
                            assert!(bitangent.dot(tip) >= -1e-6, "{case}: {bitangent}");
                        } else {
                            assert!(bitangent.abs_diff_eq(tip, 1e-5), "{case}: {bitangent}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn lod_detail_reads_partial_json() {
        let detail: LodDetail = serde_json::from_str(r#"{"leaf_stride": 2}"#).unwrap();
        assert_eq!(
            detail,
            LodDetail {
                leaf_stride: 2,
                ..LodDetail::default()
            }
        );
    }
}
