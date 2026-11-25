//! Triangle meshes of a piece at a chosen detail, ready for a renderer or a
//! file exporter.
//!
//! Every round stem is one closed, outward-facing shell: its bark wall, its
//! two ends and, for a hollow log, the inside of the hollow. Thin twigs can
//! instead become flat strips that a renderer turns toward the camera.

use std::f32::consts::{PI, TAU};
use std::ops::Range;

use glam::{Vec2, Vec3};

use crate::colour::{Palette, mix, rgba};
use crate::curve::perpendicular;
use crate::decay::Decay;
use crate::detail::DeadwoodDetail;
use crate::limb::{HollowEnd, Limb, Role, SizeClass};
use crate::options::EndKind;
use crate::rng::{smoothstep, value_noise};

/// Shortest edge around a stem, in metres; thin stems get fewer sides.
const MIN_EDGE: f32 = 0.002;

/// Which surface of a stem a vertex lies on. It is stored as a number in
/// the first value of [`MeshBuffers::wood`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    /// The outside of a stem: bark, or bare wood where bark has peeled off.
    /// A natural, bark-covered tip ([`crate::EndKind::Taper`]) also counts
    /// as side.
    Side = 0,
    /// A broken or axe-cut end, showing growth rings, with a rim of bark.
    End = 1,
    /// The inside of a hollow.
    Hollow = 2,
    /// A flat saw or chainsaw cut, showing clean growth rings, with a rim
    /// of bark. An end-grain photo fits here.
    Cut = 3,
}

/// Vertex and triangle data for one mesh.
///
/// Every vertex list has one entry per vertex. Triangles list three vertex
/// numbers each, counter-clockwise when seen from the side the normal
/// points to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshBuffers {
    /// Positions in metres.
    pub positions: Vec<[f32; 3]>,
    /// Unit surface directions, for lighting.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates. On a stem's side, `u` runs from 0 to 1 once
    /// around it and `v` runs along it, one unit per circumference at the
    /// stem's base, so a square bark texture keeps its shape. On ends, the
    /// coordinates lie flat across the end, with 0.5 at the middle and 0
    /// and 1 at the stem's smooth outline (before bumps and bark plates),
    /// so a square photo of a tree end fits the end.
    pub uvs: Vec<[f32; 2]>,
    /// Tangents for normal maps, which add fine surface relief in the
    /// renderer. The first three values are the direction in which `u`
    /// increases. The fourth is +1 or −1: the sign to apply to
    /// `normal × tangent` to get the direction in which `v` increases.
    pub tangents: Vec<[f32; 4]>,
    /// Linear RGBA vertex colours, to multiply with any texture.
    pub colors: Vec<[f32; 4]>,
    /// Extra values a wood material can use, per vertex:
    ///
    /// 0. the [`Zone`] as a number;
    /// 1. distance from the stem's centre line in metres, for drawing growth
    ///    rings on ends;
    /// 2. how much bark covers the vertex, from 0 (bare wood) to 1 (bark);
    /// 3. how rotten the wood looks, from 0 to 1.
    pub wood: Vec<[f32; 4]>,
    /// Three vertex numbers per triangle.
    pub indices: Vec<u32>,
}

impl MeshBuffers {
    /// Number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Returns the number the next pushed vertex will get.
    fn next_index(&self) -> u32 {
        u32::try_from(self.positions.len()).expect("too many vertices for 32-bit indices")
    }

    /// Appends one vertex.
    fn push(&mut self, v: Vertex) {
        self.positions.push(v.position.into());
        self.normals.push(v.normal.into());
        self.uvs.push(v.uv.into());
        self.tangents.push(v.tangent);
        self.colors.push(v.color);
        self.wood.push(v.wood);
    }
}

/// One vertex's values before they are appended to [`MeshBuffers`].
struct Vertex {
    position: Vec3,
    normal: Vec3,
    uv: Vec2,
    tangent: [f32; 4],
    color: [f32; 4],
    wood: [f32; 4],
}

/// Where one part of a piece lies in [`DeadwoodMesh::surfaces`].
///
/// A part holds all the round stems of one [`SizeClass`], so a renderer can
/// draw or tag trunks, limbs and branches separately.
#[derive(Debug, Clone, PartialEq)]
pub struct PartRange {
    /// Which size of stems the part holds.
    pub size: SizeClass,
    /// The part's vertices, as positions in the vertex lists.
    pub vertices: Range<u32>,
    /// The part's entries in [`MeshBuffers::indices`]. These count single
    /// vertex numbers, three per triangle, and refer to vertices in the
    /// whole mesh, not from the start of the part.
    pub indices: Range<u32>,
    /// One closed shell per stem, as ranges of triangle numbers counted from
    /// the part's first triangle.
    pub shells: Vec<Range<u32>>,
    /// Whether any of the part still has bark on it.
    pub bark: bool,
}

/// What a renderer needs to keep one vertex of a flat strip turned toward
/// the camera, rotating it around a fixed axis. Shapes that turn to face
/// the camera like this are often called billboards.
///
/// Each frame, place the vertex at `pivot + right * offset[0] + axis *
/// offset[1]`, where `right` is `axis` crossed with the direction from the
/// pivot to the viewer (the viewer's right), normalised. The strip then
/// shows its front face to the viewer. The normals stored in
/// [`Ribbons::strip`] are for its resting position, so turn them with the
/// strip, or point them at the viewer.
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

/// Thin twigs and branches drawn as flat strips instead of round tubes.
///
/// A strip has far fewer triangles than a tube, and from a distance looks
/// much the same if it keeps facing the camera. There are two ways to draw
/// them:
///
/// - Turn them toward the camera: draw [`strip`](Self::strip) and move its
///   vertices each frame using [`billboards`](Self::billboards).
/// - If your renderer can't turn them, draw [`strip`](Self::strip) and
///   [`cull`](Self::cull) as they are. They form two strips crossing at
///   right angles, which are visible from every side but look flatter.
///
/// Either way, turn off back-face culling so both sides of the strips are
/// drawn.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ribbons {
    /// The strips, in a fixed resting position.
    pub strip: MeshBuffers,
    /// Turning data, one entry per vertex of [`strip`](Self::strip).
    pub billboards: Vec<BillboardVertex>,
    /// A second set of fixed strips, at right angles to
    /// [`strip`](Self::strip) and with the same triangle layout. Draw it
    /// only if your renderer can't turn the strips.
    pub cull: MeshBuffers,
}

/// A piece's mesh at one level of detail.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeadwoodMesh {
    /// Every round stem, as closed shells.
    pub surfaces: MeshBuffers,
    /// Where each size class of stems is in [`surfaces`](Self::surfaces),
    /// in the order trunk, limb, branch. A size class with no round stems
    /// at this detail is left out.
    pub parts: Vec<PartRange>,
    /// Stems drawn as flat strips; `None` when there are none.
    pub ribbons: Option<Ribbons>,
    /// Leaf cards ([`crate::LeafOptions`]); `None` when there are none at
    /// this detail. Each card is a square showing the leaf texture, with
    /// `v` running from 0 at the base, where it grows from the wood, to 1
    /// at its tip. Draw them two-sided with alpha testing, using
    /// [`crate::LeafMap::Color`]'s alpha to cut out the leaves. The vertex
    /// colour is [`crate::LeafOptions::tint`], and the wood values are
    /// unused (zero).
    pub leaves: Option<MeshBuffers>,
}

impl DeadwoodMesh {
    /// Number of triangles, counting leaf cards and both [`Ribbons::strip`]
    /// and [`Ribbons::cull`]. A renderer that turns the strips draws only one of
    /// these, so it draws fewer triangles than this.
    pub fn triangle_count(&self) -> usize {
        self.surfaces.triangle_count()
            + self
                .ribbons
                .as_ref()
                .map_or(0, |r| r.strip.triangle_count() + r.cull.triangle_count())
            + self.leaves.as_ref().map_or(0, MeshBuffers::triangle_count)
    }

    /// A sphere around everything the mesh draws, as a centre and a radius
    /// in metres: the sphere through the corners of the smallest box around
    /// the round stems, the leaf cards, and every position a strip can turn
    /// to. A mesh with nothing in it gives a point at the origin.
    ///
    /// Its diameter is the piece's size on screen for choosing what to
    /// draw, and an impostor of the piece covers it.
    pub fn bounding_sphere(&self) -> ([f32; 3], f32) {
        let mut min = Vec3::INFINITY;
        let mut max = Vec3::NEG_INFINITY;
        let mut add = |lo: Vec3, hi: Vec3| {
            min = min.min(lo);
            max = max.max(hi);
        };
        let buffers = std::iter::once(&self.surfaces).chain(self.leaves.as_ref());
        for p in buffers.flat_map(|b| &b.positions) {
            add(Vec3::from(*p), Vec3::from(*p));
        }
        for b in self.ribbons.iter().flat_map(|r| &r.billboards) {
            let pivot = Vec3::from(b.pivot);
            let reach = b.offset[0].hypot(b.offset[1]);
            add(pivot - reach, pivot + reach);
        }
        if min.x > max.x {
            return ([0.0; 3], 0.0);
        }
        let centre = (min + max) * 0.5;
        (centre.into(), (max - centre).length())
    }
}

/// Colours and rot for a piece.
pub(crate) struct Shade {
    /// The piece's colours, in linear RGB.
    pub palette: Palette,
    /// The decay amounts, for weathering and rot.
    pub decay: Decay,
}

impl Shade {
    /// Returns how rotten the wood looks at `p`, 0 to 1.
    fn rot(&self, p: Vec3) -> f32 {
        self.decay.rot * (0.6 + 0.4 * value_noise(p * 3.0, 0x0f07))
    }

    /// Returns the colour of a stem's side with bark coverage `bark` (0 bare
    /// to 1 covered) and rot `rot`.
    fn side(&self, bark: f32, rot: f32) -> [f32; 3] {
        let p = &self.palette;
        let w = self.decay.weathering;
        let bark_colour = mix(p.bark, p.bark_weathered, w);
        let bare = mix(p.sapwood, p.bark_weathered, 0.5 * w);
        mix(mix(bare, bark_colour, bark), p.rot, 0.6 * rot)
    }

    /// Returns the colour of end grain at fraction `f` of the radius from
    /// the centre, with rot `rot`: darker heartwood in the core, paler
    /// sapwood toward the rim.
    fn end(&self, f: f32, rot: f32) -> [f32; 3] {
        let p = &self.palette;
        let wood = mix(p.heartwood, p.sapwood, smoothstep(0.55, 0.8, f));
        mix(wood, p.rot, 0.6 * rot)
    }

    /// Returns the colour inside a hollow.
    fn hollow(&self) -> [f32; 3] {
        mix(self.palette.rot, self.palette.heartwood, 0.3)
    }
}

/// Builds the mesh of `limbs` at `detail`, coloured by `shade`. The main
/// stem must be first in `limbs`. Side counts and ring spacing of every
/// stem are measured against the main stem's base radius.
pub(crate) fn build_mesh(limbs: &[Limb], detail: &DeadwoodDetail, shade: &Shade) -> DeadwoodMesh {
    let main_radius = limbs[0].base_radius();
    let mut mesh = DeadwoodMesh::default();
    let mut ribbons = Ribbons::default();

    // Emit round stems grouped by size class, each its own closed shell.
    for size in [SizeClass::Trunk, SizeClass::Limb, SizeClass::Branch] {
        let out = &mut mesh.surfaces;
        let (v0, i0) = (out.next_index(), out.indices.len() as u32);
        let mut shells = Vec::new();
        for limb in limbs.iter().filter(|l| l.size == size) {
            let r = limb.base_radius();
            if limb.role != Role::Main && r < detail.min_radius {
                continue;
            }
            if limb.role != Role::Main && r < detail.ribbon_radius {
                ribbon(&mut ribbons, limb, detail, main_radius, shade);
                continue;
            }
            let t0 = (out.indices.len() as u32 - i0) / 3;
            solid(out, limb, detail, main_radius, shade);
            let t1 = (out.indices.len() as u32 - i0) / 3;
            shells.push(t0..t1);
        }
        if !shells.is_empty() {
            let (v1, i1) = (out.next_index(), out.indices.len() as u32);
            let bark = out.wood[v0 as usize..v1 as usize]
                .iter()
                .any(|w| w[2] > 0.5);
            mesh.parts.push(PartRange {
                size,
                vertices: v0..v1,
                indices: i0..i1,
                shells,
                bark,
            });
        }
    }
    if !ribbons.billboards.is_empty() {
        mesh.ribbons = Some(ribbons);
    }
    mesh
}

/// Returns how many sides, wall rings and end rings `limb` gets at
/// `detail`.
fn resolution(limb: &Limb, detail: &DeadwoodDetail, main_radius: f32) -> (u32, u32, u32) {
    // Fewer sides on thinner stems, and none narrower than about 2 mm.
    let ratio = (limb.base_radius() / main_radius).min(1.0);
    let by_size = (TAU * limb.base_radius() / MIN_EDGE).ceil() as u32;
    let sides = ((detail.sides as f32 * ratio.powf(0.6)).round() as u32)
        .min(by_size.max(6))
        .max(3);
    // Space rings by the stem's diameter, but give one stem no more rings
    // than 16 diameters' worth, so long, thin sticks don't get hundreds.
    let spacing = (2.0 * limb.base_radius()).max(0.4 * main_radius) / detail.segments_per_diameter;
    let most = (16.0 * detail.segments_per_diameter).ceil() as u32;
    let rings = ((limb.length() / spacing).ceil() as u32).clamp(1, most.max(1));
    let caps = ((detail.cap_rings as f32 * ratio.sqrt()).round() as u32).max(1);
    (sides, rings, caps)
}

/// Returns the angle of column `j` of `sides`, wrapping the seam column
/// back to exactly 0 so its positions repeat bit for bit.
fn column_angle(j: u32, sides: u32) -> f32 {
    (j % sides) as f32 / sides as f32 * TAU
}

/// Returns a unit tangent along `along` across `normal`, with the sign that
/// makes `normal × tangent` point along `bitangent`.
fn tangent(normal: Vec3, along: Vec3, bitangent: Vec3) -> [f32; 4] {
    let t = perpendicular(along, normal);
    let w = if normal.cross(t).dot(bitangent) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    [t.x, t.y, t.z, w]
}

/// Appends quads between rings `a` and `b` (lists of vertex numbers, one
/// per column, the same length). `outward` picks the winding for walls
/// seen from outside the stem.
fn stitch_wall(out: &mut MeshBuffers, a: &[u32], b: &[u32], outward: bool) {
    for j in 0..a.len() - 1 {
        let (p, q, r, s) = (a[j], a[j + 1], b[j], b[j + 1]);
        if outward {
            out.indices.extend([p, q, r, q, s, r]);
        } else {
            out.indices.extend([p, r, q, q, r, s]);
        }
    }
}

/// Appends a closed round stem: wall, ends and any hollow.
fn solid(
    out: &mut MeshBuffers,
    limb: &Limb,
    detail: &DeadwoodDetail,
    main_radius: f32,
    shade: &Shade,
) {
    let (sides, rings, caps) = resolution(limb, detail, main_radius);
    let circumference = TAU * limb.base_radius();
    let length = limb.length();
    // Normals are measured across half a mesh cell, so detail finer than
    // the mesh can show doesn't tilt them.
    let (eps_u, eps_t) = (0.5 / rings as f32, PI / sides as f32);

    // The wall: columns around the stem, rings along it.
    let mut wall: Vec<Vec<u32>> = Vec::with_capacity(rings as usize + 1);
    for i in 0..=rings {
        let t = i as f32 / rings as f32;
        let mut ring = Vec::with_capacity(sides as usize + 1);
        for j in 0..=sides {
            let theta = column_angle(j, sides);
            let u = limb.wall_u(t, theta);
            let p = limb.point(u, theta);
            let d_theta = limb.point(u, theta + eps_t) - limb.point(u, theta - eps_t);
            let d_u = limb.point(u + eps_u, theta) - limb.point(u - eps_u, theta);
            let frame = limb.frame(u);
            let normal = d_theta
                .cross(d_u)
                .try_normalize()
                .unwrap_or_else(|| frame.radial(theta));
            let bark = 1.0 - limb.bare(u, theta);
            let rot = shade.rot(p);
            ring.push(out.next_index());
            out.push(Vertex {
                position: p,
                normal,
                uv: Vec2::new(j as f32 / sides as f32, u * length / circumference),
                tangent: tangent(normal, d_theta, d_u),
                color: rgba(shade.side(bark, rot)),
                wood: [
                    Zone::Side as u32 as f32,
                    (p - frame.position).length(),
                    bark,
                    rot,
                ],
            });
        }
        wall.push(ring);
    }
    for i in 0..rings as usize {
        stitch_wall(out, &wall[i], &wall[i + 1], true);
    }

    // The ends, and the hollow's inside where it opens through them.
    let span = limb.hollow_span();
    for e in 0..2 {
        let open = matches!(span, Some((HollowEnd::Open(a), _)) if a == e)
            || matches!(span, Some((_, HollowEnd::Open(b))) if b == e);
        end_cap(out, limb, e, sides, caps, open, shade);
    }
    if let Some((start, end)) = span {
        hollow(out, limb, sides, rings, caps, start, end, shade);
    }
}

/// Appends end `e` of `limb`: a solid cap, or a ring around the hollow when
/// `open`.
fn end_cap(
    out: &mut MeshBuffers,
    limb: &Limb,
    e: usize,
    sides: u32,
    caps: u32,
    open: bool,
    shade: &Shade,
) {
    let end = &limb.ends[e];
    let sign = Limb::end_sign(e);
    let plane = limb.frame(limb.end_plane(e));
    let wood = end.shows_wood();
    let step = (0.5 / caps.max(1) as f32, PI / sides as f32);

    // Sawn faces get their own zone, for end-grain photos. Texture
    // coordinates span the end's smooth outline, averaged around it.
    let end_zone = match end.kind {
        EndKind::Saw { .. } | EndKind::Chainsaw { .. } => Zone::Cut,
        _ => Zone::End,
    };
    let u_end = limb.end_plane(e);
    let outline = (0..8)
        .map(|i| limb.nominal_radius(u_end, i as f32 * TAU / 8.0))
        .sum::<f32>()
        / 8.0;
    let uv_scale = 0.5 / outline.max(1e-4);

    // Ring fractions for each angle: the rim, the inner edge of the bark,
    // then evenly inward to the middle or to the hollow.
    let fractions = |theta: f32| -> Vec<f32> {
        let mut rho = vec![1.0];
        if wood {
            let bark = limb.bark_rho(e, theta);
            rho.push(bark);
            let inner = if open { limb.hollow_rho(e, theta) } else { 0.0 };
            let steps = caps.max(1);
            for k in 1..=steps {
                if !open && k == steps {
                    break;
                }
                let f = k as f32 / steps as f32;
                rho.push(if open && k == steps {
                    inner
                } else {
                    bark + (inner - bark) * f
                });
            }
        } else {
            for k in 1..caps {
                rho.push(1.0 - k as f32 / caps as f32);
            }
        }
        rho
    };

    // Vertices: one row per ring fraction, plus a second copy of the bark
    // edge row so bark and wood meet with a sharp colour change.
    let ring_count = fractions(0.0).len();
    let mut rows: Vec<Vec<u32>> = vec![Vec::with_capacity(sides as usize); ring_count + 1];
    for j in 0..sides {
        let theta = column_angle(j, sides);
        let rho = fractions(theta);
        for (k, &f) in rho.iter().enumerate() {
            let position = if k == 0 {
                limb.point(limb.wall_end(e, theta), theta)
            } else {
                limb.end_point(e, f, theta)
            };
            let normal = end_normal(limb, e, f, theta, step);
            let local = position - plane.position;
            let uv = Vec2::new(
                0.5 + local.dot(plane.normal) * uv_scale,
                0.5 + local.dot(plane.binormal) * uv_scale,
            );
            let tan = tangent(normal, plane.normal, plane.binormal);
            let radius = (local - plane.tangent * local.dot(plane.tangent)).length();
            let rot = shade.rot(position);
            let bark = 1.0 - limb.bare(limb.wall_end(e, theta), theta);
            let mut emit = |row: usize, bark_side: bool| {
                let (zone, colour, cover) = if !wood {
                    (Zone::Side, shade.side(bark, rot), bark)
                } else if bark_side {
                    (end_zone, shade.side(bark, rot), bark)
                } else {
                    (end_zone, shade.end(f, rot), 0.0)
                };
                rows[row].push(out.next_index());
                out.push(Vertex {
                    position,
                    normal,
                    uv,
                    tangent: tan,
                    color: rgba(colour),
                    wood: [zone as u32 as f32, radius, cover, rot],
                });
            };
            match k {
                0 => emit(0, true),
                1 if wood => {
                    emit(1, true);
                    emit(2, false);
                }
                _ => emit(if wood { k + 1 } else { k }, false),
            }
        }
    }
    if !wood {
        rows.pop();
    }

    // Triangles between rows: the bark band, then the wood rings.
    let tip = sign > 0.0;
    let pairs: Vec<(usize, usize)> = if wood {
        std::iter::once((0, 1))
            .chain((2..rows.len() - 1).map(|k| (k, k + 1)))
            .collect()
    } else {
        (0..rows.len() - 1).map(|k| (k, k + 1)).collect()
    };
    for (a, b) in pairs {
        stitch_ring(out, &rows[a], &rows[b], tip);
    }

    // A solid end closes with a fan to its middle point.
    if !open {
        let last = rows.last().expect("an end has at least one row");
        let position = limb.end_point(e, 0.0, 0.0);
        let normal = end_normal(limb, e, 0.0, 0.0, step);
        let rot = shade.rot(position);
        let (zone, colour, cover) = if wood {
            (end_zone, shade.end(0.0, rot), 0.0)
        } else {
            let bark = 1.0 - limb.bare(limb.end_plane(e), 0.0);
            (Zone::Side, shade.side(bark, rot), bark)
        };
        let centre = out.next_index();
        out.push(Vertex {
            position,
            normal,
            uv: Vec2::splat(0.5),
            tangent: tangent(normal, plane.normal, plane.binormal),
            color: rgba(colour),
            wood: [zone as u32 as f32, 0.0, cover, rot],
        });
        fan(out, last, centre, tip);
    }
}

/// Returns the outward unit normal of end `e`'s surface at `rho`, `theta`,
/// measured across `step` (in `rho` and in angle), about half a mesh cell.
fn end_normal(limb: &Limb, e: usize, rho: f32, theta: f32, step: (f32, f32)) -> Vec3 {
    let (eps_r, eps_t) = step;
    let rho = rho.max(eps_r);
    let (r0, r1) = ((rho - eps_r).max(0.0), (rho + eps_r).min(1.0));
    let d_rho = limb.end_point(e, r1, theta) - limb.end_point(e, r0, theta);
    let d_theta = limb.end_point(e, rho, theta + eps_t) - limb.end_point(e, rho, theta - eps_t);
    let out = limb.frame(limb.end_plane(e)).tangent * Limb::end_sign(e);
    let n = d_rho.cross(d_theta) * Limb::end_sign(e);
    n.try_normalize().unwrap_or(out)
}

/// Appends triangles between an outer ring `a` and an inner ring `b` of an
/// end (vertex numbers per column, wrapping around). `tip` picks the
/// winding for an end facing along the stem.
fn stitch_ring(out: &mut MeshBuffers, a: &[u32], b: &[u32], tip: bool) {
    let n = a.len();
    for j in 0..n {
        let k = (j + 1) % n;
        let (o0, o1, i0, i1) = (a[j], a[k], b[j], b[k]);
        if tip {
            out.indices.extend([o0, o1, i0, o1, i1, i0]);
        } else {
            out.indices.extend([o0, i0, o1, o1, i0, i1]);
        }
    }
}

/// Appends a fan from ring `ring` to vertex `centre`.
fn fan(out: &mut MeshBuffers, ring: &[u32], centre: u32, tip: bool) {
    let n = ring.len();
    for j in 0..n {
        let (a, b) = (ring[j], ring[(j + 1) % n]);
        if tip {
            out.indices.extend([a, b, centre]);
        } else {
            out.indices.extend([a, centre, b]);
        }
    }
}

/// Appends the inside of a hollow between `start` (low `u`) and `end`
/// (high `u`): its wall, facing inward, and a rounded bottom at a closed
/// end.
#[allow(clippy::too_many_arguments)]
fn hollow(
    out: &mut MeshBuffers,
    limb: &Limb,
    sides: u32,
    rings: u32,
    caps: u32,
    start: HollowEnd,
    end: HollowEnd,
    shade: &Shade,
) {
    let colour = rgba(shade.hollow());
    let circumference = TAU * limb.base_radius();
    let length = limb.length();

    // Where each column starts and ends, and the exact positions at the
    // openings, shared with the end rings.
    let limit = |h: HollowEnd, theta: f32| -> (f32, Option<Vec3>) {
        match h {
            HollowEnd::Open(e) => {
                let rho = limb.hollow_rho(e, theta);
                (
                    limb.inner_end(e, rho, theta),
                    Some(limb.end_point(e, rho, theta)),
                )
            }
            HollowEnd::Closed(u) => (u, None),
        }
    };
    let (ua, ub) = (limit(start, 0.0).0, limit(end, 0.0).0);
    let n = ((rings as f32 * (ub - ua).abs()).ceil() as u32).max(1);
    let (eps_u, eps_t) = (0.5 * (ub - ua).abs() / n as f32, PI / sides as f32);

    let mut wall: Vec<Vec<u32>> = Vec::with_capacity(n as usize + 1);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let mut ring = Vec::with_capacity(sides as usize + 1);
        for j in 0..=sides {
            let theta = column_angle(j, sides);
            let (a, pa) = limit(start, theta);
            let (b, pb) = limit(end, theta);
            let u = a + (b - a) * t;
            let position = match (i, pa, pb) {
                (0, Some(p), _) => p,
                (i, _, Some(p)) if i == n => p,
                _ => limb.inner_point(u, theta),
            };
            let d_theta = limb.inner_point(u, theta + eps_t) - limb.inner_point(u, theta - eps_t);
            let d_u = limb.inner_point(u + eps_u, theta) - limb.inner_point(u - eps_u, theta);
            let frame = limb.frame(u);
            let normal = -d_theta
                .cross(d_u)
                .try_normalize()
                .unwrap_or_else(|| frame.radial(theta));
            let rot = shade.rot(position).max(shade.decay.rot);
            ring.push(out.next_index());
            out.push(Vertex {
                position,
                normal,
                uv: Vec2::new(j as f32 / sides as f32, u * length / circumference),
                tangent: tangent(normal, d_theta, d_u),
                color: colour,
                wood: [
                    Zone::Hollow as u32 as f32,
                    (position - frame.position).length(),
                    0.0,
                    rot,
                ],
            });
        }
        wall.push(ring);
    }
    for i in 0..n as usize {
        stitch_wall(out, &wall[i], &wall[i + 1], false);
    }

    // Close a blind end with a rounded bottom bulging away from the opening.
    for (h, ring, away) in [(start, &wall[0], -1.0f32), (end, &wall[n as usize], 1.0)] {
        let HollowEnd::Closed(u) = h else { continue };
        let plane = limb.frame(u);
        let dir = plane.tangent * away;
        let depth = 0.5 * limb.inner_radius(u, 0.0);
        let steps = (caps / 2).max(1);
        let rim: Vec<u32> = ring[..sides as usize].to_vec();
        let mut previous = rim;
        let point = |rho: f32, theta: f32| {
            let r = limb.inner_point(u, theta) - plane.position;
            plane.position + r * rho + dir * depth * (1.0 - rho * rho).max(0.0).sqrt()
        };
        // A bottom bulging toward +u is wound like a base end, and the
        // other way like a tip end, so it faces into the hollow.
        let tip = away < 0.0;
        for k in 1..=steps {
            let rho = 1.0 - k as f32 / steps as f32;
            let mut row = Vec::with_capacity(sides as usize);
            let count = if k == steps { 1 } else { sides };
            for j in 0..count {
                let theta = column_angle(j, sides);
                let position = point(rho, theta);
                // The bottom is half an ellipsoid; face into the hollow,
                // against the ellipsoid's outward direction.
                let local = position - plane.position;
                let along = local.dot(dir);
                let across = local - dir * along;
                let radius = limb.inner_radius(u, theta).max(1e-6);
                let outward = across / (radius * radius) + dir * along / (depth * depth).max(1e-12);
                let normal = (-outward).try_normalize().unwrap_or(-dir);
                let rot = shade.decay.rot;
                row.push(out.next_index());
                out.push(Vertex {
                    position,
                    normal,
                    uv: Vec2::new(j as f32 / sides as f32, 0.0),
                    tangent: tangent(normal, plane.normal, plane.binormal),
                    color: colour,
                    wood: [Zone::Hollow as u32 as f32, 0.0, 0.0, rot],
                });
            }
            if k == steps {
                fan(out, &previous, row[0], tip);
            } else {
                stitch_ring(out, &previous, &row, tip);
                previous = row;
            }
        }
    }
}

/// Appends `limb` as a flat strip with turning data, plus the crossing
/// strip for renderers that can't turn it.
fn ribbon(r: &mut Ribbons, limb: &Limb, detail: &DeadwoodDetail, main_radius: f32, shade: &Shade) {
    let (_, segments, _) = resolution(limb, detail, main_radius);
    let base = r.strip.next_index();
    let circumference = TAU * limb.base_radius();
    for i in 0..=segments {
        let u = i as f32 / segments as f32;
        let frame = limb.frame(u);
        let radius = limb.nominal_radius(u, 0.0);
        let bark = 1.0 - limb.bare(u, 0.0);
        let colour = rgba(shade.side(bark, shade.rot(frame.position)));
        for (k, (across, front)) in [
            (frame.normal, frame.binormal),
            (frame.binormal, frame.normal),
        ]
        .into_iter()
        .enumerate()
        {
            for (side, uv_u) in [(-1.0f32, 0.0f32), (1.0, 1.0)] {
                let offset = side * radius;
                let vertex = Vertex {
                    position: frame.position + across * offset,
                    normal: front,
                    uv: Vec2::new(uv_u, u * limb.length() / circumference),
                    tangent: [across.x, across.y, across.z, 1.0],
                    color: colour,
                    wood: [Zone::Side as u32 as f32, radius, bark, 0.0],
                };
                if k == 0 {
                    r.strip.push(vertex);
                    r.billboards.push(BillboardVertex {
                        pivot: frame.position.into(),
                        axis: frame.tangent.into(),
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
            let a = base + i * 2;
            buffers
                .indices
                .extend([a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }
}
