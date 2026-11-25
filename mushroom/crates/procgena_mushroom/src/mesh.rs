//! Triangle meshes of a mushroom at a chosen detail, ready for a renderer or
//! a file exporter.

use std::f32::consts::TAU;
use std::ops::Range;

use glam::{Affine3A, Vec2, Vec3};

use crate::colour::{Palette, mix, rgba};
use crate::detail::{BodyShape, MushroomDetail};
use crate::profile::Profile;
use crate::rng::smoothstep;
use crate::shape::{AnnulusShape, Shape, Wart};

/// Which surface of the mushroom a vertex belongs to.
///
/// Renderers use it to draw gill lines only on the underside, and patterns
/// and warts only on the cap. [`id`](Self::id) gives a number to store in a
/// vertex attribute for a shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    /// The cap's top surface.
    CapTop,
    /// The narrow band at the cap's edge, between top and underside.
    CapRim,
    /// The cap's underside (the gills).
    Underside,
    /// The stem, including its closed bottom.
    Stem,
    /// The annulus.
    Annulus,
}

impl Zone {
    /// Returns a number for this zone that won't change between versions:
    /// 0 for [`CapTop`](Self::CapTop) up to 4 for
    /// [`Annulus`](Self::Annulus), in the order listed.
    pub fn id(self) -> u32 {
        self as u32
    }
}

/// What a group of triangles in a [`MushroomMesh`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartKind {
    /// The cap, including its rim and underside when those are built.
    Cap,
    /// The stem's surface, including its closed bottom.
    Stem,
    /// The annulus.
    Annulus,
}

impl PartKind {
    /// Every kind, in the order parts appear in a mesh.
    pub const ALL: [PartKind; 3] = [PartKind::Cap, PartKind::Stem, PartKind::Annulus];
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
    /// The part's separate closed pieces, such as one per mushroom in a
    /// cluster, as ranges of triangle numbers counted from the part's first
    /// triangle. Empty when the part is open.
    pub shells: Vec<Range<u32>>,
}

/// A triangle mesh as plain lists, in metres, with Y up and the ground at
/// height 0.
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

    /// Appends vertices `vertices` and the triangles `indices` of `from`,
    /// moved by `transform`, which must scale equally along every axis.
    fn append_moved(
        &mut self,
        from: &MeshBuffers,
        vertices: Range<u32>,
        indices: Range<u32>,
        transform: Affine3A,
    ) {
        let base = self.next_index();
        let (v0, v1) = (vertices.start as usize, vertices.end as usize);
        let turn = |v: [f32; 3]| -> [f32; 3] {
            transform
                .transform_vector3(v.into())
                .normalize_or_zero()
                .into()
        };
        self.positions.extend(
            from.positions[v0..v1]
                .iter()
                .map(|&p| <[f32; 3]>::from(transform.transform_point3(p.into()))),
        );
        self.normals
            .extend(from.normals[v0..v1].iter().map(|&n| turn(n)));
        self.uvs.extend_from_slice(&from.uvs[v0..v1]);
        self.tangents.extend(from.tangents[v0..v1].iter().map(|t| {
            let [x, y, z] = turn([t[0], t[1], t[2]]);
            [x, y, z, t[3]]
        }));
        self.colors.extend_from_slice(&from.colors[v0..v1]);
        self.zones.extend_from_slice(&from.zones[v0..v1]);
        self.indices.extend(
            from.indices[indices.start as usize..indices.end as usize]
                .iter()
                .map(|&i| i - vertices.start + base),
        );
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

/// A mushroom's mesh at one detail.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MushroomMesh {
    /// Every surface.
    pub surfaces: MeshBuffers,
    /// Where each part (cap, stem, annulus) is in
    /// [`surfaces`](Self::surfaces). Parts a detail leaves out are absent.
    pub parts: Vec<PartRange>,
    /// True when [`surfaces`](Self::surfaces) has open surfaces whose inside
    /// can be seen. Draw such meshes two-sided (without hiding back faces),
    /// or they will show holes.
    pub double_sided: bool,
}

impl MushroomMesh {
    /// Returns the number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.surfaces.triangle_count()
    }

    /// Returns where part `kind` is, or `None` if this mesh doesn't have it.
    pub fn part(&self, kind: PartKind) -> Option<&PartRange> {
        self.parts.iter().find(|p| p.kind == kind)
    }
}

/// Combines `meshes`, each moved by its transform, into one mesh. Each
/// transform must scale equally along every axis. Parts of the same kind
/// are merged into one part, whose shells are the closed pieces of every
/// mesh; it is closed only if every mesh's part was.
pub(crate) fn combine(meshes: &[(&MushroomMesh, Affine3A)]) -> MushroomMesh {
    let mut out = MushroomMesh {
        double_sided: meshes.iter().any(|(m, _)| m.double_sided),
        ..MushroomMesh::default()
    };

    // Each kind of part in turn, so every merged part stays contiguous.
    for kind in PartKind::ALL {
        let (v0, i0) = (out.surfaces.next_index(), out.surfaces.indices.len() as u32);
        let mut closed = true;
        let mut shells = Vec::new();
        for (mesh, transform) in meshes {
            let Some(range) = mesh.part(kind) else {
                continue;
            };
            let first = (out.surfaces.indices.len() as u32 - i0) / 3;
            out.surfaces.append_moved(
                &mesh.surfaces,
                range.vertices.clone(),
                range.indices.clone(),
                *transform,
            );
            closed &= range.closed;
            shells.extend(range.shells.iter().map(|s| s.start + first..s.end + first));
        }
        let (v1, i1) = (out.surfaces.next_index(), out.surfaces.indices.len() as u32);
        if i1 > i0 {
            out.parts.push(PartRange {
                kind,
                vertices: v0..v1,
                indices: i0..i1,
                closed,
                shells: if closed { shells } else { Vec::new() },
            });
        }
    }
    out
}

/// Builds the mesh of `shape` at `detail`, with vertex colours from
/// `palette`. `detail` must already be validated.
pub(crate) fn build(shape: &Shape, detail: &MushroomDetail, palette: &Palette) -> MushroomMesh {
    let solid = detail.shape == BodyShape::Solid;
    let mut mesh = MushroomMesh {
        double_sided: !solid,
        ..MushroomMesh::default()
    };
    let spots = if detail.paint_warts {
        &shape.warts[..]
    } else {
        &[][..]
    };

    // Cap.
    part(&mut mesh, PartKind::Cap, solid, |out| {
        cap(out, shape, detail, palette, spots);
    });

    // Stem. A solid stem is closed only if its bottom is.
    let closed = solid && shape.bottom_depth.is_some();
    part(&mut mesh, PartKind::Stem, closed, |out| {
        stem(out, shape, detail, palette, solid);
    });

    // Annulus: a single sheet. Solid levels are drawn one-sided, so the
    // sheet gets a second, reversed copy to be visible from both sides.
    if let (Some(annulus), true) = (&shape.annulus, detail.annulus) {
        part(&mut mesh, PartKind::Annulus, false, |out| {
            annulus_sheet(out, shape, annulus, detail, palette, solid);
        });
    }
    mesh
}

/// Runs `build` to append one part's triangles and records where they are.
/// Nothing is recorded if `build` adds no triangles. A closed part is
/// recorded as one closed piece.
fn part(
    mesh: &mut MushroomMesh,
    kind: PartKind,
    closed: bool,
    build: impl FnOnce(&mut MeshBuffers),
) {
    let (v0, i0) = (
        mesh.surfaces.next_index(),
        mesh.surfaces.indices.len() as u32,
    );
    build(&mut mesh.surfaces);
    let (v1, i1) = (
        mesh.surfaces.next_index(),
        mesh.surfaces.indices.len() as u32,
    );
    if i1 > i0 {
        let whole: Range<u32> = 0..(i1 - i0) / 3;
        mesh.parts.push(PartRange {
            kind,
            vertices: v0..v1,
            indices: i0..i1,
            closed,
            shells: if closed { vec![whole] } else { Vec::new() },
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

/// Returns `count + 1` evenly spaced values from 0 to 1.
fn even_stations(count: u32) -> Vec<f32> {
    (0..=count).map(|i| i as f32 / count as f32).collect()
}

// --------------------------------------------------------------------------
// Cap

/// Appends the cap. `spots` are warts to paint into the top's colours.
///
/// Every shape gets the top, and an underside so the cap keeps its outline
/// from the side. [`BodyShape::Solid`] adds the rim band and a disc inside
/// the stem that close the cap; [`BodyShape::Shell`] keeps the rim band and
/// the underside's curve but leaves the disc out.
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

/// Returns the stem's colour at height `s`, from 0 (ground) to 1 (top).
fn stem_colour(palette: &Palette, s: f32) -> [f32; 3] {
    mix(palette.stem_base, palette.stem_top, smoothstep(0.0, 1.0, s))
}

/// Appends the stem's side, its rounded bottom if it has one and, with
/// `solid`, the disc that closes its top inside the cap.
fn stem(
    out: &mut MeshBuffers,
    shape: &Shape,
    detail: &MushroomDetail,
    palette: &Palette,
    solid: bool,
) {
    // Side.
    let side = even_stations(detail.stem_segments);
    let side_point = |s: f32, theta: f32| shape.stem_point(s, theta, 1.0);
    let side_colour = |s: f32, _: f32, _: Vec3| stem_colour(palette, s);
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

    // Rounded bottom, meshed from its lowest point up to the ground, where
    // its last ring matches the side's first exactly.
    if shape.bottom_depth.is_some() {
        let bottom = even_stations((detail.stem_segments / 3).max(2));
        let bottom_point = |t: f32, theta: f32| shape.bottom_point(t, theta);
        let bottom_colour = |_: f32, _: f32, _: Vec3| stem_colour(palette, 0.0);
        Sheet {
            stations: &bottom,
            sides: detail.stem_sides,
            point: &bottom_point,
            color: &bottom_colour,
            pole_start: true,
            pole_end: false,
            zone: Zone::Stem,
            flip: false,
        }
        .emit(out);
    }
    if !solid {
        return;
    }

    // Top disc, meshed from its centre out to the edge. That direction
    // faces down, so it is flipped to face up.
    let disc_point = |k: f32, theta: f32| shape.stem_point(1.0, theta, k);
    let disc_colour = |_: f32, _: f32, _: Vec3| stem_colour(palette, 1.0);
    Sheet {
        stations: &[0.0, 1.0],
        sides: detail.stem_sides,
        point: &disc_point,
        color: &disc_colour,
        pole_start: true,
        pole_end: false,
        zone: Zone::Stem,
        flip: true,
    }
    .emit(out);
}

// --------------------------------------------------------------------------
// Annulus

/// Appends the annulus's sheet. With `back_faces`, also appends a reversed
/// copy so the annulus is visible from both sides when drawn one-sided.
fn annulus_sheet(
    out: &mut MeshBuffers,
    shape: &Shape,
    annulus: &AnnulusShape,
    detail: &MushroomDetail,
    palette: &Palette,
    back_faces: bool,
) {
    let segments = (detail.stem_segments / 3).clamp(1, 4);
    let st = stations(&annulus.profile, segments);
    let point = |s: f32, theta: f32| shape.annulus_point(annulus, s, theta);
    let colour = |_: f32, _: f32, _: Vec3| palette.annulus;
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
            zone: Zone::Annulus,
            flip,
        }
        .emit(out);
    }
}
