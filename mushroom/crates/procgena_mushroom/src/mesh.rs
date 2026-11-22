//! Triangle meshes of a mushroom at a chosen detail, ready for a renderer or
//! a file exporter.

use std::f32::consts::TAU;
use std::ops::Range;

use glam::{Vec2, Vec3};

use crate::colour::{Palette, mix, rgba};
use crate::detail::{BodyShape, MushroomDetail, WartDetail};
use crate::profile::Profile;
use crate::rng::smoothstep;
use crate::shape::{RingShape, Shape, Wart};

/// Which surface of the mushroom a vertex belongs to.
///
/// Renderers use it to draw gill lines only on the underside and patterns
/// only on the cap. [`id`](Self::id) gives a number to store in a vertex
/// attribute for a shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    /// The cap's top surface.
    CapTop,
    /// The narrow band at the cap's edge, between top and underside.
    CapRim,
    /// The cap's underside (the gills).
    Underside,
    /// The stem.
    Stem,
    /// The ring.
    Ring,
    /// A wart.
    Wart,
}

impl Zone {
    /// Returns a number for this zone that won't change between versions:
    /// 0 for [`CapTop`](Self::CapTop) up to 5 for [`Wart`](Self::Wart), in
    /// the order listed.
    pub fn id(self) -> u32 {
        self as u32
    }
}

/// What a group of triangles in a [`MushroomMesh`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartKind {
    /// The cap, including its rim and underside when those are built.
    Cap,
    /// The stem's surface. A flat stem is kept separately, in
    /// [`MushroomMesh::stem_ribbon`].
    Stem,
    /// The ring.
    Ring,
    /// Every wart.
    Warts,
}

/// Where one part's triangles are in [`MushroomMesh::surfaces`].
#[derive(Debug, Clone, PartialEq)]
pub struct PartRange {
    /// What the triangles are.
    pub kind: PartKind,
    /// The vertices the part uses.
    pub vertices: Range<u32>,
    /// The part's entries in the index list (three per triangle).
    pub indices: Range<u32>,
    /// True if the part is watertight: closed, with no holes, so it has an
    /// inside and an outside.
    pub closed: bool,
    /// The part's separate closed pieces, such as one per wart, as ranges of
    /// triangle numbers counted from the part's first triangle. Empty when
    /// the part is open.
    pub shells: Vec<Range<u32>>,
}

/// A triangle mesh as plain lists, in metres, with Y up and the stem base
/// at the origin.
///
/// Every list except `indices` has one entry per vertex. Each group of three
/// `indices` is a triangle, listed counter-clockwise when seen from its
/// front.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshBuffers {
    /// Vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Unit normals: the outward direction of the surface at each vertex,
    /// used for lighting.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates. `u` goes once around the mushroom's axis, from 0
    /// to 1. `v` goes from 0 to 1 along each surface: rim to top on the cap,
    /// base to top on the stem.
    pub uvs: Vec<[f32; 2]>,
    /// Direction of increasing `u` at each vertex, used to apply normal
    /// maps. The fourth value is 1 or -1 and says which way `v` runs
    /// relative to it.
    pub tangents: Vec<[f32; 4]>,
    /// Vertex colours as red, green, blue and alpha in linear light (not
    /// sRGB). Alpha is always 1.
    pub colors: Vec<[f32; 4]>,
    /// The surface each vertex belongs to.
    pub zones: Vec<Zone>,
    /// Vertex numbers, three per triangle.
    pub indices: Vec<u32>,
}

impl MeshBuffers {
    /// Returns the number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Returns the number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Returns the number the next pushed vertex will get.
    fn next_index(&self) -> u32 {
        self.positions.len() as u32
    }

    /// Appends one vertex to every per-vertex list.
    fn push(&mut self, v: Vertex) {
        self.positions.push(v.position.into());
        self.normals.push(v.normal.into());
        self.uvs.push(v.uv.into());
        self.tangents.push(v.tangent);
        self.colors.push(v.color);
        self.zones.push(v.zone);
    }
}

/// One vertex's values before they are appended to [`MeshBuffers`].
struct Vertex {
    position: Vec3,
    normal: Vec3,
    uv: Vec2,
    tangent: [f32; 4],
    color: [f32; 4],
    zone: Zone,
}

/// What a renderer needs to keep one vertex of a flat strip turned toward
/// the camera, rotating it around a fixed axis. Such camera-facing shapes
/// are often called billboards.
///
/// Each frame, place the vertex at `pivot + right * offset[0] + axis *
/// offset[1]`, where `right` is `axis` crossed with the direction from the
/// pivot to the viewer (the viewer's right), normalised. The strip then
/// shows its front face to the viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BillboardVertex {
    /// Point on the stem's centre line that the vertex turns around, in
    /// metres.
    pub pivot: [f32; 3],
    /// Unit axis to turn around: the stem's direction at the pivot.
    pub axis: [f32; 3],
    /// Distance from the pivot to the right and along the axis, in metres.
    pub offset: [f32; 2],
}

/// The flat stem of a [`BodyShape::Flat`] mesh: a strip that a renderer
/// keeps turned toward the camera, around the stem's axis, so it looks like
/// a round stem from every side.
///
/// To turn it, draw [`strip`](Self::strip) and move its vertices with
/// [`billboards`](Self::billboards), as the `procgena_mushroom_bevy` crate
/// does. If your renderer can't turn it, draw [`strip`](Self::strip) and
/// [`cull`](Self::cull) as they are: two strips crossing at right angles,
/// which are visible from every side but look flatter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StemRibbon {
    /// The strip, in a resting position facing the mushroom's +Z, with
    /// normals pointing out of its front.
    pub strip: MeshBuffers,
    /// Turning data, one entry per vertex of [`strip`](Self::strip).
    pub billboards: Vec<BillboardVertex>,
    /// A second fixed strip at right angles to the resting strip, for
    /// renderers that can't turn [`strip`](Self::strip). It has the same
    /// triangle layout.
    pub cull: MeshBuffers,
}

/// A mushroom's mesh at one detail level.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MushroomMesh {
    /// Every surface except a flat stem.
    pub surfaces: MeshBuffers,
    /// Where each part (cap, stem, ring, warts) is in
    /// [`surfaces`](Self::surfaces). Parts a level leaves out are absent.
    pub parts: Vec<PartRange>,
    /// The flat stem; present only for [`BodyShape::Flat`].
    pub stem_ribbon: Option<StemRibbon>,
    /// True when [`surfaces`](Self::surfaces) has open surfaces whose inside
    /// can be seen. Draw such meshes two-sided (without hiding back faces),
    /// or they will show holes.
    pub double_sided: bool,
}

impl MushroomMesh {
    /// Returns the number of triangles drawn: the surfaces plus a flat
    /// stem's turning strip, but not its crossing fallback strip.
    pub fn triangle_count(&self) -> usize {
        self.surfaces.triangle_count()
            + self
                .stem_ribbon
                .as_ref()
                .map_or(0, |r| r.strip.triangle_count())
    }

    /// Returns where part `kind` is, or `None` if this mesh doesn't have it.
    pub fn part(&self, kind: PartKind) -> Option<&PartRange> {
        self.parts.iter().find(|p| p.kind == kind)
    }
}

/// Builds the mesh of `shape` at `detail`, with vertex colours from
/// `palette`. `detail` must already be validated.
pub(crate) fn build(shape: &Shape, detail: &MushroomDetail, palette: &Palette) -> MushroomMesh {
    let mut mesh = MushroomMesh {
        double_sided: detail.shape != BodyShape::Solid,
        ..MushroomMesh::default()
    };
    let solid = detail.shape == BodyShape::Solid;
    let spots = if detail.warts == WartDetail::None {
        &[][..]
    } else {
        &shape.warts[..]
    };

    // Cap.
    part(&mut mesh, PartKind::Cap, solid, |out| {
        cap(out, shape, detail, palette, spots);
        Vec::new()
    });

    // Stem.
    match detail.shape {
        BodyShape::Flat => mesh.stem_ribbon = Some(ribbon(shape, detail, palette)),
        _ => part(&mut mesh, PartKind::Stem, solid, |out| {
            stem(out, shape, detail, palette, solid);
            Vec::new()
        }),
    }

    // Ring: a single sheet. Solid levels are drawn one-sided, so the sheet
    // gets a second, reversed copy to be visible from both sides.
    if let (Some(ring), true) = (&shape.ring, detail.ring) {
        part(&mut mesh, PartKind::Ring, false, |out| {
            ring_sheet(out, shape, ring, detail, palette, solid);
            Vec::new()
        });
    }

    // Warts.
    if detail.warts == WartDetail::Mesh && !shape.warts.is_empty() {
        part(&mut mesh, PartKind::Warts, true, |out| {
            shape
                .warts
                .iter()
                .map(|w| wart(out, w, detail, palette))
                .collect()
        });
    }
    mesh
}

/// Runs `build` to append one part's triangles and records where they are.
/// Nothing is recorded if `build` adds no triangles.
///
/// `build` returns the part's closed pieces as triangle ranges counted from
/// the start of the buffers, or an empty list when a closed part is one
/// piece.
fn part(
    mesh: &mut MushroomMesh,
    kind: PartKind,
    closed: bool,
    build: impl FnOnce(&mut MeshBuffers) -> Vec<Range<u32>>,
) {
    let (v0, i0) = (
        mesh.surfaces.next_index(),
        mesh.surfaces.indices.len() as u32,
    );
    let shells = build(&mut mesh.surfaces);
    let (v1, i1) = (
        mesh.surfaces.next_index(),
        mesh.surfaces.indices.len() as u32,
    );
    if i1 > i0 {
        // Pieces arrive counted from the start of the buffers; store them
        // counted from the part's first triangle.
        let t0 = i0 / 3;
        let shells = if closed && shells.is_empty() {
            // One piece covering every triangle of the part.
            let whole: Range<u32> = 0..(i1 - i0) / 3;
            vec![whole]
        } else {
            shells
                .into_iter()
                .map(|r| r.start - t0..r.end - t0)
                .collect()
        };
        mesh.parts.push(PartRange {
            kind,
            vertices: v0..v1,
            indices: i0..i1,
            closed,
            shells,
        });
    }
}

// --------------------------------------------------------------------------
// Surfaces spun around an axis

/// A surface spun around an axis, meshed as rings of vertices: one ring per
/// value in `stations`, each with `sides` segments.
///
/// The front of the surface is on the right of its outline when the outline
/// is followed in the direction of increasing `stations`, with the axis on
/// the left (for example, outward when an outline climbs a cap from rim to
/// top). Set `flip` for the other side.
struct Sheet<'a> {
    /// Position along the outline of each ring, increasing.
    stations: &'a [f32],
    /// Segments around.
    sides: u32,
    /// Surface position at (position along the outline, angle in radians).
    point: &'a dyn Fn(f32, f32) -> Vec3,
    /// Linear RGB colour at (position along the outline, angle, position).
    color: &'a dyn Fn(f32, f32, Vec3) -> [f32; 3],
    /// True if the first ring is a single point on the axis.
    pole_start: bool,
    /// True if the last ring is a single point on the axis.
    pole_end: bool,
    /// Zone stored on every vertex.
    zone: Zone,
    /// Swap front and back.
    flip: bool,
}

impl Sheet<'_> {
    /// Appends the sheet's vertices and triangles to `out`.
    ///
    /// Normals come from the shape of the surface itself rather than from
    /// the triangles, so coarse and fine levels are lit alike.
    fn emit(&self, out: &mut MeshBuffers) {
        let (s0, s1) = (self.stations[0], *self.stations.last().unwrap());
        let span = s1 - s0;
        let eps = span * 1e-3;
        let first = out.next_index();
        let ring = self.sides + 1;

        for &s in self.stations {
            // Each ring repeats its first vertex at the end, where texture
            // `u` wraps from 1 back to 0. Copy its position exactly so the
            // seam has no gap.
            let mut seam = None;
            for j in 0..ring {
                let theta = if j == self.sides {
                    0.0
                } else {
                    j as f32 / self.sides as f32 * TAU
                };
                let position = match (j == self.sides, seam) {
                    (true, Some(p)) => p,
                    _ => (self.point)(s, theta),
                };
                if j == 0 {
                    seam = Some(position);
                }

                // Normal from two small steps on the surface: along the
                // outline and around the axis. At a pole the steps are taken
                // just beside it, where "around" still has a direction.
                let sn = s.clamp(s0 + eps, s1 - eps);
                let along = (self.point)((sn + eps).min(s1), theta)
                    - (self.point)((sn - eps).max(s0), theta);
                let around = (self.point)(sn, theta + 1e-3) - (self.point)(sn, theta - 1e-3);
                let mut normal = along.cross(around).try_normalize().unwrap_or(Vec3::Y);
                if self.flip {
                    normal = -normal;
                }
                let tangent = around.try_normalize().unwrap_or(Vec3::X);
                let sign = if normal.cross(tangent).dot(along) >= 0.0 {
                    1.0
                } else {
                    -1.0
                };

                out.push(Vertex {
                    position,
                    normal,
                    uv: Vec2::new(j as f32 / self.sides as f32, (s - s0) / span),
                    tangent: [tangent.x, tangent.y, tangent.z, sign],
                    color: rgba((self.color)(s, theta, position)),
                    zone: self.zone,
                });
            }
        }

        // Two triangles per square between neighbouring rings. Next to a
        // pole one of the two would have no area, so it is left out.
        let rings = self.stations.len() as u32;
        for i in 0..rings - 1 {
            for j in 0..self.sides {
                let a = first + i * ring + j;
                let (b, c, d) = (a + ring, a + 1, a + ring + 1);
                let start_pole = i == 0 && self.pole_start;
                let end_pole = i == rings - 2 && self.pole_end;
                let mut tris: Vec<[u32; 3]> = Vec::with_capacity(2);
                if !start_pole {
                    tris.push([a, b, c]);
                }
                if !end_pole {
                    tris.push([b, d, c]);
                }
                for [x, y, z] in tris {
                    if self.flip {
                        out.indices.extend([x, z, y]);
                    } else {
                        out.indices.extend([x, y, z]);
                    }
                }
            }
        }
    }
}

/// Returns ring positions for about `segments` steps along `profile`; see
/// [`Profile::stations`].
fn stations(profile: &Profile, segments: u32) -> Vec<f32> {
    profile.stations(segments)
}

// --------------------------------------------------------------------------
// Cap

/// Appends the cap. `spots` are warts to paint into the top's colours.
///
/// Every shape gets the top, and an underside so the cap keeps its outline
/// from the side. [`BodyShape::Solid`] adds the rim band and a disc inside
/// the stem that close the cap; [`BodyShape::Shell`] keeps the rim band and
/// the underside's curve but leaves the disc out; [`BodyShape::Flat`] uses
/// one straight band from the stem to the rim.
fn cap(
    out: &mut MeshBuffers,
    shape: &Shape,
    detail: &MushroomDetail,
    palette: &Palette,
    spots: &[Wart],
) {
    let body = detail.shape;
    let top_len = shape.top.length();
    // Distance between vertices around the rim. Painted spots smaller than
    // this are spread out and faded, so coarse levels show an even tint
    // rather than smears.
    let spacing = TAU * shape.radius / detail.cap_sides as f32;
    let top_colour = |s: f32, _theta: f32, p: Vec3| {
        let toward_rim = 1.0 - s / top_len;
        let base = mix(
            palette.cap_centre,
            palette.cap_rim,
            smoothstep(0.1, 0.95, toward_rim),
        );
        mix(base, palette.wart, spot_cover(spots, p, spacing))
    };

    // Top surface, rim to top centre.
    let top_stations = stations(&shape.top, detail.cap_segments);
    let top_point = |s: f32, theta: f32| shape.cap_point(shape.top.at(s), theta);
    Sheet {
        stations: &top_stations,
        sides: detail.cap_sides,
        point: &top_point,
        color: &top_colour,
        pole_start: false,
        pole_end: true,
        zone: Zone::CapTop,
        flip: false,
    }
    .emit(out);

    let rim_bottom = shape.underside.at(shape.underside.length());
    let rim_top = shape.top.at(0.0);
    let under_colour = |_: f32, _: f32, _: Vec3| palette.underside;

    // Where the underside leaves the stem: its first corner after the
    // disc's centre.
    let neck = shape.underside.corners()[1];

    // Flat caps: one straight band from the stem to the rim.
    if body == BodyShape::Flat {
        let neck_point = shape.underside.at(neck);
        let band_point = |s: f32, theta: f32| {
            let p = if s >= 1.0 {
                rim_top
            } else {
                neck_point.lerp(rim_top, s)
            };
            shape.cap_point(p, theta)
        };
        Sheet {
            stations: &[0.0, 1.0],
            sides: detail.cap_sides,
            point: &band_point,
            color: &under_colour,
            pole_start: false,
            pole_end: false,
            zone: Zone::Underside,
            flip: false,
        }
        .emit(out);
        return;
    }

    // Rim band: a straight band from the underside's edge up to the top's.
    // Its ends use the same points as both neighbouring surfaces, so they
    // meet without gaps.
    let rim_point = |s: f32, theta: f32| {
        let p = match s {
            0.0 => rim_bottom,
            1.0 => rim_top,
            _ => rim_bottom.lerp(rim_top, s),
        };
        shape.cap_point(p, theta)
    };
    let rim_colour = |_: f32, _: f32, _: Vec3| palette.cap_rim;
    // A rim with no thickness gets no band, since the underside already
    // meets the top.
    if rim_bottom != rim_top {
        Sheet {
            stations: &[0.0, 1.0],
            sides: detail.cap_sides,
            point: &rim_point,
            color: &rim_colour,
            pole_start: false,
            pole_end: false,
            zone: Zone::CapRim,
            flip: false,
        }
        .emit(out);
    }

    // Underside out to the rim. Solid caps start from the centre of the
    // disc hidden inside the stem; open caps leave the disc out and start
    // where the underside leaves the stem.
    let solid = body == BodyShape::Solid;
    let mut under_stations = stations(&shape.underside, (detail.cap_segments / 2).max(1));
    if !solid {
        under_stations.retain(|&s| s >= neck);
    }
    let under_point = |s: f32, theta: f32| shape.cap_point(shape.underside.at(s), theta);
    Sheet {
        stations: &under_stations,
        sides: detail.cap_sides,
        point: &under_point,
        color: &under_colour,
        pole_start: solid,
        pole_end: false,
        zone: Zone::Underside,
        flip: false,
    }
    .emit(out);
}

/// Returns how much wart colour to paint at `p`, from 0 to 1.
///
/// A wart wider than `spacing` shows fully over its footprint and fades just
/// outside it. A smaller wart is spread over `spacing` and faded in
/// proportion to its area, so a coarse mesh shows about the same overall
/// amount of wart colour as a fine one.
fn spot_cover(spots: &[Wart], p: Vec3, spacing: f32) -> f32 {
    let total: f32 = spots
        .iter()
        .map(|w| {
            let reach = w.radius.max(spacing);
            let strength = (w.radius / reach).powi(2);
            strength * (1.0 - smoothstep(0.75, 1.15, w.centre.distance(p) / reach))
        })
        .sum();
    total.min(1.0)
}

// --------------------------------------------------------------------------
// Stem

/// Appends the stem's side and, with `solid`, the discs that close its two
/// ends.
fn stem(
    out: &mut MeshBuffers,
    shape: &Shape,
    detail: &MushroomDetail,
    palette: &Palette,
    solid: bool,
) {
    let colour = |s: f32| mix(palette.stem_base, palette.stem_top, smoothstep(0.0, 1.0, s));

    // Side.
    let side: Vec<f32> = (0..=detail.stem_segments)
        .map(|i| i as f32 / detail.stem_segments as f32)
        .collect();
    let side_point = |s: f32, theta: f32| shape.stem_point(s, theta, 1.0);
    let side_colour = |s: f32, _: f32, _: Vec3| colour(s);
    Sheet {
        stations: &side,
        sides: detail.stem_sides,
        point: &side_point,
        color: &side_colour,
        pole_start: false,
        pole_end: false,
        zone: Zone::Stem,
        flip: false,
    }
    .emit(out);
    if !solid {
        return;
    }

    // End discs, each meshed from its centre out to the edge. That
    // direction faces down, which suits the base; the top disc is flipped
    // to face up.
    for (s, flip) in [(0.0, false), (1.0, true)] {
        let disc_point = move |k: f32, theta: f32| shape.stem_point(s, theta, k);
        let disc_colour = |_: f32, _: f32, _: Vec3| colour(s);
        Sheet {
            stations: &[0.0, 1.0],
            sides: detail.stem_sides,
            point: &disc_point,
            color: &disc_colour,
            pole_start: true,
            pole_end: false,
            zone: Zone::Stem,
            flip,
        }
        .emit(out);
    }
}

/// Builds the flat stem: the strip to turn toward the camera, its turning
/// data, and the crossing strip for renderers that can't turn it.
fn ribbon(shape: &Shape, detail: &MushroomDetail, palette: &Palette) -> StemRibbon {
    let mut r = StemRibbon::default();
    let segments = detail.stem_segments;
    for i in 0..=segments {
        let s = i as f32 / segments as f32;
        let (centre, axis, frame) = (
            shape.stem_centre(s),
            shape.stem_tangent(s),
            shape.stem_frame(s),
        );
        let radius = shape.stem_radius(s);
        let color = rgba(mix(
            palette.stem_base,
            palette.stem_top,
            smoothstep(0.0, 1.0, s),
        ));
        for (k, (across, front)) in [(Vec3::X, Vec3::Z), (Vec3::Z, Vec3::X)]
            .into_iter()
            .enumerate()
        {
            let (across, front) = (frame * across, frame * front);
            for (side, u) in [(-1.0f32, 0.0f32), (1.0, 1.0)] {
                let offset = side * radius;
                let vertex = Vertex {
                    position: centre + across * offset,
                    normal: front,
                    uv: Vec2::new(u, s),
                    tangent: [across.x, across.y, across.z, 1.0],
                    color,
                    zone: Zone::Stem,
                };
                if k == 0 {
                    r.strip.push(vertex);
                    // At rest the strip faces the stem's +Z, where a viewer
                    // has the stem's +X on their right, so the sideways
                    // offset keeps its sign.
                    r.billboards.push(BillboardVertex {
                        pivot: centre.into(),
                        axis: axis.into(),
                        offset: [offset, 0.0],
                    });
                } else {
                    r.cull.push(vertex);
                }
            }
        }
    }

    // Two triangles per step along the stem, laid out the same way in both
    // strips.
    for buffers in [&mut r.strip, &mut r.cull] {
        for i in 0..segments {
            let a = i * 2;
            buffers
                .indices
                .extend([a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }
    r
}

// --------------------------------------------------------------------------
// Ring and warts

/// Appends the ring's sheet. With `back_faces`, also appends a reversed copy
/// so the ring is visible from both sides when drawn one-sided.
fn ring_sheet(
    out: &mut MeshBuffers,
    shape: &Shape,
    ring: &RingShape,
    detail: &MushroomDetail,
    palette: &Palette,
    back_faces: bool,
) {
    let segments = (detail.stem_segments / 3).clamp(1, 4);
    let st = stations(&ring.profile, segments);
    let point = |s: f32, theta: f32| shape.ring_point(ring, s, theta);
    let colour = |_: f32, _: f32, _: Vec3| palette.ring;
    for flip in [false, true] {
        if flip && !back_faces {
            break;
        }
        Sheet {
            stations: &st,
            sides: detail.stem_sides,
            point: &point,
            color: &colour,
            pole_start: false,
            pole_end: false,
            zone: Zone::Ring,
            flip,
        }
        .emit(out);
    }
}

/// Appends one closed wart and returns its triangles as a range counted
/// from the start of `out`.
fn wart(out: &mut MeshBuffers, w: &Wart, detail: &MushroomDetail, palette: &Palette) -> Range<u32> {
    // Side-view outline for a wart of radius 1 and height 1: a flat base,
    // then a dome.
    let outline = Profile::new(
        &[
            Vec2::ZERO,
            Vec2::new(1.0, 0.0),
            Vec2::new(0.8, 0.55),
            Vec2::new(0.0, 1.0),
        ],
        &[1],
    );
    let st = stations(&outline, 3);
    // Two directions across the wart that, with its normal as "up", are
    // arranged like the X, Y and Z axes, so the wart's faces point outward.
    let t1 = w.normal.any_orthonormal_vector();
    let t2 = t1.cross(w.normal);
    let point = |s: f32, theta: f32| {
        let p = outline.at(s);
        let (sin, cos) = theta.sin_cos();
        w.centre + (t1 * cos + t2 * sin) * (p.x * w.radius) + w.normal * (p.y * w.height)
    };
    let colour = |_: f32, _: f32, _: Vec3| palette.wart;

    let first = out.indices.len() as u32 / 3;
    Sheet {
        stations: &st,
        sides: detail.wart_sides,
        point: &point,
        color: &colour,
        pole_start: true,
        pole_end: true,
        zone: Zone::Wart,
        flip: false,
    }
    .emit(out);
    first..out.indices.len() as u32 / 3
}
