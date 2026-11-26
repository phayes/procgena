//! What is made from canonical chunk meshes: triangle lookup, ground
//! queries on loaded chunks, meshes to draw, the fit of regular
//! neighbours, and collision geometry.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use glam::{DVec2, DVec3, Vec3};
use procgena_terrain::tin::{
    ChunkEdgeKey, Cm, CmHeight, ContractCache, EdgeContract, GroundSurface, ResidentTins, TinChunk,
    TinIndex, TinTolerance, TinVertex, build_chunk, build_chunk_with, triangulate,
};
use procgena_terrain::{
    ChunkKey, ChunkMesh, ChunkSamples, ChunkSide, ChunkSource, FeatureLevel, SurfacePoint,
    Transitions,
};

/// Ground given by a function, at the finest level.
struct Analytic<F>(F);

impl<F: Fn(DVec2) -> f64 + Sync> ChunkSource for Analytic<F> {
    fn surface(&self, p: DVec2, _: FeatureLevel) -> SurfacePoint {
        SurfacePoint {
            height: (self.0)(p) as f32,
            water: None,
        }
    }
    fn level(&self, _: FeatureLevel) -> FeatureLevel {
        FeatureLevel::FINEST
    }
    fn level_difference(&self, _: DVec2, _: DVec2, _: FeatureLevel) -> f32 {
        0.0
    }
}

/// Smooth hills, up to about 30 m.
fn hills(p: DVec2) -> f64 {
    10.0 + 12.0 * (p.x / 37.0).sin() * (p.y / 53.0).cos() + 8.0 * ((p.x + p.y) / 23.0).sin()
}

/// The canonical mesh of level-0 chunk `(x, z)` over the hills.
fn hill_chunk(x: i64, z: i64) -> Arc<TinChunk> {
    Arc::new(
        build_chunk(
            &Analytic(hills),
            ChunkKey::new(x, z, 0),
            &TinTolerance::GENERAL,
        )
        .unwrap(),
    )
}

/// Twice the signed area of `a`, `b`, `c` in (x, z): negative when they
/// face up.
fn orient(a: (i64, i64), b: (i64, i64), c: (i64, i64)) -> i64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// The lowest-numbered triangle of `chunk` whose closed area holds the
/// centimetre point `p`, by checking every triangle.
fn brute_locate(chunk: &TinChunk, p: (i64, i64)) -> Option<u32> {
    chunk
        .triangles
        .iter()
        .position(|t| {
            let [a, b, c] = t.0.map(|i| {
                let v = chunk.vertices[i as usize];
                (v.x.0, v.z.0)
            });
            orient(a, b, p) <= 0 && orient(b, c, p) <= 0 && orient(c, a, p) <= 0
        })
        .map(|t| t as u32)
}

/// A chunk with vertices off the whole-metre lattice, on its border and
/// inside it, as feature lines will add.
fn off_lattice_chunk() -> Arc<TinChunk> {
    let key = ChunkKey::new(2, -1, 0);
    let height = |x: i64, z: i64| CmHeight(((x / 7 + z / 11) % 9000) as i32);
    let contracts = ChunkEdgeKey::of_chunk(key).map(|edge| {
        let (x0, z0) = edge.start();
        // Each edge gets one vertex 12.34 m along, besides its corners.
        let along = |t: i64| match edge.axis {
            procgena_terrain::tin::EdgeAxis::X => (x0.0 + t, z0.0),
            procgena_terrain::tin::EdgeAxis::Z => (x0.0, z0.0 + t),
        };
        let vertices = [0, 1_234, 6_400]
            .map(|t| {
                let (x, z) = along(t);
                TinVertex {
                    x: Cm(x),
                    z: Cm(z),
                    h: height(x, z),
                }
            })
            .to_vec();
        EdgeContract {
            key: edge,
            normals: vec![glam::Vec3::Y; vertices.len()],
            vertices,
            crossings: Vec::new(),
        }
    });
    let (x0, z0) = (12_800, -6_400);
    let interior: Vec<TinVertex> = [(3_123, 2_701), (555, 5_051), (4_817, 999)]
        .into_iter()
        .map(|(u, w)| TinVertex {
            x: Cm(x0 + u),
            z: Cm(z0 + w),
            h: height(x0 + u, z0 + w),
        })
        .collect();
    Arc::new(triangulate(
        key,
        [&contracts[0], &contracts[1], &contracts[2], &contracts[3]],
        &interior,
    ))
}

/// Millimetre points over chunk `key`'s closed square: a grid with an odd
/// spacing, every vertex, and points along every triangle edge.
fn probe_points(chunk: &TinChunk) -> Vec<(i64, i64)> {
    let (x0, z0) = (chunk.key.x * 6_400, chunk.key.z * 6_400);
    let mut points = Vec::new();
    for j in 0..=173 {
        for i in 0..=173 {
            points.push((x0 + i * 6_400 / 173, z0 + j * 6_400 / 173));
        }
    }
    points.extend(chunk.vertices.iter().map(|v| (v.x.0, v.z.0)));
    for t in &chunk.triangles {
        for k in 0..3 {
            let (a, b) = (
                chunk.vertices[t.0[k] as usize],
                chunk.vertices[t.0[(k + 1) % 3] as usize],
            );
            // The edge's midpoint, when it is a whole centimetre.
            if (a.x.0 + b.x.0) % 2 == 0 && (a.z.0 + b.z.0) % 2 == 0 {
                points.push(((a.x.0 + b.x.0) / 2, (a.z.0 + b.z.0) / 2));
            }
        }
    }
    points
}

/// The centimetre point as metres.
fn metres(p: (i64, i64)) -> DVec2 {
    DVec2::new(p.0 as f64 / 100.0, p.1 as f64 / 100.0)
}

#[test]
fn lookup_finds_the_first_triangle_holding_each_point() {
    for chunk in [hill_chunk(0, 0), hill_chunk(-1, 2), off_lattice_chunk()] {
        let index = TinIndex::new(chunk.clone());
        for p in probe_points(&chunk) {
            let found = index.locate(metres(p));
            assert!(found.is_some(), "{p:?} in {:?} has no triangle", chunk.key);
            assert_eq!(found, brute_locate(&chunk, p), "at {p:?}");
        }
        // Outside the chunk's square there is nothing.
        let (x0, z0) = (chunk.key.x * 6_400, chunk.key.z * 6_400);
        assert_eq!(index.locate(metres((x0 - 1, z0 + 5))), None);
        assert_eq!(index.locate(metres((x0 + 64_001, z0 + 5))), None);
    }
}

#[test]
fn heights_meet_the_vertices_and_agree_across_edges() {
    for chunk in [hill_chunk(0, 0), off_lattice_chunk()] {
        let index = TinIndex::new(chunk.clone());
        for v in &chunk.vertices {
            let s = index.sample(metres((v.x.0, v.z.0))).unwrap();
            assert_eq!(s.height, v.h.to_m(), "at vertex {v:?}");
            assert!(s.normal.y > 0.0 && (s.normal.length() - 1.0).abs() < 1e-12);
        }
        // Each triangle's plane, evaluated at its neighbours' shared edge
        // points, gives the same height as the triangle the lookup picks.
        for p in probe_points(&chunk) {
            let s = index.sample(metres(p)).unwrap();
            for (t, tri) in chunk.triangles.iter().enumerate() {
                let corners = tri.0.map(|i| chunk.vertices[i as usize]);
                let xz = corners.map(|v| (v.x.0, v.z.0));
                let w = [
                    orient(xz[1], xz[2], p),
                    orient(xz[2], xz[0], p),
                    orient(xz[0], xz[1], p),
                ];
                if w.iter().any(|&w| w > 0) {
                    continue;
                }
                let area = orient(xz[0], xz[1], xz[2]) as f64;
                let h: f64 = (0..3)
                    .map(|k| w[k] as f64 * f64::from(corners[k].h.0))
                    .sum::<f64>()
                    / area
                    / 100.0;
                assert!(
                    (h - s.height).abs() < 1e-9,
                    "triangle {t} gives {h} at {p:?}, the lookup {}",
                    s.height
                );
            }
        }
    }
}

#[test]
fn neighbouring_chunks_agree_on_their_shared_side() {
    let (a, b) = (
        TinIndex::new(hill_chunk(0, 0)),
        TinIndex::new(hill_chunk(1, 0)),
    );
    for z in (0..=6_400).step_by(25) {
        let p = metres((6_400, z));
        let (ha, hb) = (a.sample(p).unwrap().height, b.sample(p).unwrap().height);
        assert!((ha - hb).abs() < 1e-9, "at z = {z} cm: {ha} against {hb}");
    }
}

#[test]
fn loaded_chunks_answer_only_for_points_they_own() {
    let mut tins = ResidentTins::new();
    tins.insert(Arc::new(TinIndex::new(hill_chunk(-1, 0))));
    tins.insert(Arc::new(TinIndex::new(hill_chunk(0, 0))));
    assert_eq!(tins.len(), 2);

    // Inside each, the answer is that chunk's.
    let s = tins.sample(DVec2::new(-10.0, 30.0)).unwrap();
    assert_eq!(s.chunk, ChunkKey::new(-1, 0, 0));
    let s = tins.sample(DVec2::new(10.0, 30.0)).unwrap();
    assert_eq!(s.chunk, ChunkKey::new(0, 0, 0));
    // x = 0 belongs to chunk 0; z = 64 belongs to the unloaded chunk above.
    assert_eq!(tins.sample(DVec2::new(0.0, 30.0)).unwrap().chunk.x, 0);
    assert_eq!(tins.sample(DVec2::new(10.0, 64.0)), None);
    assert_eq!(tins.sample(DVec2::new(64.0, 10.0)), None);
    assert_eq!(tins.sample(DVec2::new(f64::NAN, 10.0)), None);

    // Unloading chunk 0 leaves its points unanswered, even on the side it
    // shares with chunk -1.
    tins.remove(ChunkKey::new(0, 0, 0));
    assert_eq!(tins.sample(DVec2::new(0.0, 30.0)), None);
    assert!(tins.sample(DVec2::new(-0.01, 30.0)).is_some());
}

#[test]
fn loaded_chunks_of_several_levels_answer_finest_first() {
    let coarse = |x: i64, z: i64| {
        let key = ChunkKey::new(x, z, 1);
        Arc::new(build_chunk(&Analytic(hills), key, &TinTolerance::GENERAL).unwrap())
    };
    let mut tins = ResidentTins::new();
    // Level-1 chunks (128 m) at x from -128 to 128 m, and a level-0 chunk
    // over the first quarter of the eastern one.
    tins.insert(Arc::new(TinIndex::new(coarse(-1, 0))));
    tins.insert(Arc::new(TinIndex::new(coarse(0, 0))));
    tins.insert(Arc::new(TinIndex::new(hill_chunk(0, 0))));
    assert_eq!(tins.len(), 3);

    // Each point gets the finest loaded chunk holding it.
    let chunk_of =
        |tins: &ResidentTins, x: f64, z: f64| tins.sample(DVec2::new(x, z)).map(|s| s.chunk);
    let chunk = |x: f64, z: f64| chunk_of(&tins, x, z);
    assert_eq!(chunk(10.0, 30.0), Some(ChunkKey::new(0, 0, 0)));
    assert_eq!(chunk(100.0, 30.0), Some(ChunkKey::new(0, 0, 1)));
    assert_eq!(chunk(10.0, 100.0), Some(ChunkKey::new(0, 0, 1)));
    assert_eq!(chunk(-10.0, 30.0), Some(ChunkKey::new(-1, 0, 1)));
    // x = 128 m belongs to the unloaded level-1 chunk east of them.
    assert_eq!(chunk(128.0, 30.0), None);
    // A level-1 chunk's height matches its own index's answer.
    let p = DVec2::new(100.0, 30.0);
    let own = TinIndex::new(coarse(0, 0)).sample(p).unwrap();
    assert_eq!(tins.sample(p), Some(own));

    // The batch agrees with single queries.
    let points: Vec<DVec2> = (0..500)
        .map(|i| DVec2::new(f64::from(i) * 0.6 - 140.0, f64::from(i % 37) * 3.4))
        .collect();
    let batch = tins.sample_batch(&points);
    for (p, b) in points.iter().zip(&batch) {
        assert_eq!(*b, tins.sample(*p), "at {p}");
    }

    // Unloading the level-0 chunk hands its points to the level-1 one.
    tins.remove(ChunkKey::new(0, 0, 0));
    assert_eq!(chunk_of(&tins, 10.0, 30.0), Some(ChunkKey::new(0, 0, 1)));
}

#[test]
fn a_batch_answers_as_single_queries_do() {
    let mut tins = ResidentTins::new();
    for (x, z) in [(0, 0), (1, 0), (0, 1)] {
        tins.insert(Arc::new(TinIndex::new(hill_chunk(x, z))));
    }
    let points: Vec<DVec2> = (0..2000)
        .map(|i| {
            let t = f64::from(i);
            DVec2::new((t * 0.173) % 150.0 - 10.0, (t * 0.071) % 140.0 - 5.0)
        })
        .collect();
    let batch = tins.sample_batch(&points);
    for (p, b) in points.iter().zip(&batch) {
        assert_eq!(*b, tins.sample(*p), "at {p}");
    }
    assert!(batch.iter().any(Option::is_none) && batch.iter().any(Option::is_some));
}

/// The world position of vertex `i` of `mesh`.
fn world(mesh: &ChunkMesh, i: usize) -> DVec3 {
    mesh.translation() + mesh.positions[i].as_dvec3()
}

/// The area the mesh's triangles cover, seen from above, after checking
/// that none faces down. Upright triangles (skirts) cover none.
fn upward_area(mesh: &ChunkMesh) -> f64 {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|k| mesh.positions[k as usize].as_dvec3());
            let n = (b - a).cross(c - a);
            if n.y.abs() < 1e-9 {
                // A skirt quad half, standing upright.
                return 0.0;
            }
            assert!(n.y > 0.0, "triangle {t:?} faces down");
            n.y / 2.0
        })
        .sum()
}

#[test]
fn the_drawn_mesh_is_the_canonical_one_with_split_sides() {
    let source = Analytic(hills);
    for chunk in [hill_chunk(0, 0), off_lattice_chunk()] {
        let normals = chunk.shading_normals(&source).unwrap();
        assert_eq!(normals.len(), chunk.vertices.len());

        // Unsplit and without skirts: the canonical triangles as they are.
        let plain = chunk.render_mesh(&normals, &[], 0.0);
        assert_eq!(plain.positions.len(), chunk.vertices.len());
        assert_eq!(plain.indices.len(), chunk.triangles.len() * 3);
        assert!((upward_area(&plain) - 64.0 * 64.0).abs() < 1e-6);
        for (i, v) in chunk.vertices.iter().enumerate() {
            // Local positions are f32, so exact to well within a
            // centimetre.
            let p = world(&plain, i);
            assert_eq!((p.x * 100.0).round() as i64, v.x.0);
            assert_eq!((p.z * 100.0).round() as i64, v.z.0);
            assert_eq!(plain.positions[i].y, v.h.to_m() as f32);
        }

        // Every side split: a vertex every metre, the same area, and every
        // new vertex on the canonical border.
        let split = chunk.render_mesh(&normals, &ChunkSide::ALL, 0.0);
        assert!((upward_area(&split) - 64.0 * 64.0).abs() < 1e-6);
        let index = TinIndex::new(chunk.clone());
        let mut border: BTreeSet<(i64, i64)> = BTreeSet::new();
        for i in 0..split.positions.len() {
            let p = world(&split, i);
            let cm = ((p.x * 100.0).round() as i64, (p.z * 100.0).round() as i64);
            let height = chunk.border_height(Cm(cm.0), Cm(cm.1));
            if let Some(h) = height {
                border.insert(cm);
                assert_eq!(split.positions[i].y, h as f32);
            }
            // Every vertex, old or new, lies on the canonical surface.
            let canonical = index.sample(DVec2::new(p.x, p.z)).unwrap().height;
            assert!((canonical - p.y).abs() < 1e-3, "{p} off the surface");
        }
        let (x0, z0) = (chunk.key.x * 6_400, chunk.key.z * 6_400);
        for t in (0..=6_400).step_by(100) {
            for p in [
                (x0 + t, z0),
                (x0 + t, z0 + 6_400),
                (x0, z0 + t),
                (x0 + 6_400, z0 + t),
            ] {
                assert!(border.contains(&p), "no vertex at {p:?}");
            }
        }

        // Skirts hang below every side.
        let skirted = chunk.render_mesh(&normals, &[ChunkSide::MinX], 8.0);
        assert!(skirted.positions.len() > plain.positions.len());
        assert!(skirted.heights.0 <= plain.heights.0 - 8.0 + 1e-4);
        assert!((upward_area(&skirted) - 64.0 * 64.0).abs() < 1e-6);
    }
}

/// Whether `v` is on the border of `chunk`'s square.
fn on_border(chunk: &TinChunk, v: &TinVertex) -> bool {
    let width = 6_400 << chunk.key.level;
    let (x0, z0) = (chunk.key.x * width, chunk.key.z * width);
    v.x.0 == x0 || v.x.0 == x0 + width || v.z.0 == z0 || v.z.0 == z0 + width
}

#[test]
fn border_shading_matches_regular_chunks_where_they_share_vertices() {
    // On the border, where a regular neighbour may meet it, a canonical
    // chunk shades its vertices as regular chunks do.
    let source = Analytic(hills);
    let chunk = hill_chunk(0, 0);
    let normals = chunk.shading_normals(&source).unwrap();
    let regular = ChunkSamples::sample(&source, chunk.key, FeatureLevel::FINEST)
        .unwrap()
        .mesh(Transitions::NONE, 0.0);
    let mut checked = 0;
    for (v, n) in chunk.vertices.iter().zip(&normals) {
        if !on_border(&chunk, v) {
            continue;
        }
        // Lattice vertex (i, j) is regular vertex j × 65 + i.
        let (i, j) = (v.x.0 / 100, v.z.0 / 100);
        let r = regular.normals[(j * 65 + i) as usize];
        assert!((*n - r).length() < 1e-5, "{n} against {r} at {v:?}");
        checked += 1;
    }
    assert!(checked >= 4);
}

#[test]
fn neighbours_shade_their_shared_border_alike() {
    // The chunks either side of an edge give its vertices the same normals,
    // bit for bit, so the lighting has no seam there.
    let source = Analytic(hills);
    let (a, b) = (hill_chunk(0, 0), hill_chunk(1, 0));
    let (na, nb) = (
        a.shading_normals(&source).unwrap(),
        b.shading_normals(&source).unwrap(),
    );
    let shared = |chunk: &TinChunk, normals: &[Vec3]| -> BTreeMap<(i64, i64), Vec3> {
        chunk
            .vertices
            .iter()
            .zip(normals)
            .filter(|(v, _)| v.x.0 == 6_400)
            .map(|(v, &n)| ((v.x.0, v.z.0), n))
            .collect()
    };
    let (sa, sb) = (shared(&a, &na), shared(&b, &nb));
    assert!(sa.len() >= 2);
    assert_eq!(sa, sb);
}

#[test]
fn coarse_chunks_shade_their_shared_border_alike_from_their_own_step() {
    // A 2 × 2 block at level 2 (256 m chunks, 4 m lattice).
    let source = Analytic(hills);
    let tolerance = TinTolerance::GENERAL;
    let cache = ContractCache::new(&tolerance);
    let mut shaded: BTreeMap<(Cm, Cm), Vec<Vec3>> = BTreeMap::new();
    for (x, z) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
        let chunk = build_chunk_with(&source, ChunkKey::new(x, z, 2), &tolerance, &cache).unwrap();
        let normals = chunk
            .shading_normals_with(&source, &tolerance, &cache)
            .unwrap();
        for (v, n) in chunk.vertices.iter().zip(normals) {
            if on_border(&chunk, v) {
                shaded.entry((v.x, v.z)).or_default().push(n);
            }
        }
    }
    // Every border vertex two or four chunks hold is shaded the same by
    // each, with the ground's slope 4 m either side.
    let mut shared = 0;
    for (&(x, z), normals) in &shaded {
        assert!(normals.iter().all(|n| *n == normals[0]), "at {x:?}, {z:?}");
        shared += usize::from(normals.len() > 1);
        let at = DVec2::new(x.to_m(), z.to_m());
        let h = |dx: f64, dz: f64| hills(at + DVec2::new(dx, dz)) as f32;
        let slope = Vec3::new(h(-4.0, 0.0) - h(4.0, 0.0), 8.0, h(0.0, -4.0) - h(0.0, 4.0));
        assert!((normals[0] - slope.normalize()).length() < 1e-5, "at {at}");
    }
    assert!(shared > 4, "neighbours share border vertices");
}

#[test]
fn shading_reads_no_ground_with_the_contracts_the_chunk_was_built_with() {
    // Border vertices take the normals their edge contracts carry, and a
    // vertex inside takes the area-weighted normal of its own triangles,
    // so with the build's contract cache nothing reads the ground.
    let source = Counted(Analytic(hills), AtomicUsize::new(0));
    let tolerance = TinTolerance::GENERAL;
    let cache = ContractCache::new(&tolerance);
    let chunk = build_chunk_with(&source, ChunkKey::new(0, 0, 0), &tolerance, &cache).unwrap();
    source.1.store(0, Ordering::Relaxed);
    let normals = chunk
        .shading_normals_with(&source, &tolerance, &cache)
        .unwrap();
    assert_eq!(source.1.load(Ordering::Relaxed), 0);
    assert_eq!(normals, chunk.shading_normals(&Analytic(hills)).unwrap());
    let border = chunk
        .vertices
        .iter()
        .filter(|v| on_border(&chunk, v))
        .count();
    assert!(
        border < chunk.vertices.len(),
        "the chunk has inside vertices"
    );
    for (i, (v, n)) in chunk.vertices.iter().zip(&normals).enumerate() {
        assert!((n.length() - 1.0).abs() < 1e-5 && n.y > 0.0, "{v:?}");
        if on_border(&chunk, v) {
            continue;
        }
        let mut sum = Vec3::ZERO;
        for t in chunk.triangles.iter().filter(|t| t.0.contains(&(i as u32))) {
            let [p, q, r] = t.0.map(|k| {
                let w = chunk.vertices[k as usize];
                Vec3::new(w.x.0 as f32, w.h.0 as f32, w.z.0 as f32) / 100.0
            });
            let face = (q - p).cross(r - p);
            sum += if face.y < 0.0 { -face } else { face };
        }
        assert!((*n - sum.normalize()).length() < 1e-4, "{v:?}");
    }
}

/// Ground from another source, counting the heights read from it.
struct Counted<S>(S, AtomicUsize);

impl<S: ChunkSource> ChunkSource for Counted<S> {
    fn surface(&self, p: DVec2, level: FeatureLevel) -> SurfacePoint {
        self.1.fetch_add(1, Ordering::Relaxed);
        self.0.surface(p, level)
    }
    fn level(&self, level: FeatureLevel) -> FeatureLevel {
        self.0.level(level)
    }
    fn level_difference(&self, min: DVec2, max: DVec2, from: FeatureLevel) -> f32 {
        self.0.level_difference(min, max, from)
    }
}

/// The regular mesh of `key` over the hills, with `transitions`.
fn regular(key: ChunkKey, transitions: Transitions, skirt: f32) -> ChunkMesh {
    ChunkSamples::sample(&Analytic(hills), key, FeatureLevel::FINEST)
        .unwrap()
        .mesh(transitions, skirt)
}

/// The world positions, in centimetres, and heights of `mesh`'s vertices
/// on the line `x = line_cm` between `z0` and `z1` centimetres, the highest
/// at each position.
fn on_side(mesh: &ChunkMesh, line_cm: i64, (z0, z1): (i64, i64)) -> BTreeMap<i64, f32> {
    let mut found = BTreeMap::new();
    for i in 0..mesh.positions.len() {
        let p = world(mesh, i);
        let (x, z) = ((p.x * 100.0).round() as i64, (p.z * 100.0).round() as i64);
        if x == line_cm && (z0..=z1).contains(&z) {
            let y = found.entry(z).or_insert(f32::NEG_INFINITY);
            *y = y.max(mesh.positions[i].y);
        }
    }
    found
}

#[test]
fn regular_neighbours_fit_the_canonical_border_exactly() {
    let source = Analytic(hills);
    let chunk = hill_chunk(1, 0);
    let normals = chunk.shading_normals(&source).unwrap();
    let tin = chunk.render_mesh(&normals, &ChunkSide::ALL, 0.0);

    // A level-0 neighbour across its low-x side, and a level-1 neighbour
    // across its high-x side, meshed to join a finer chunk there.
    let cases = [
        (
            regular(ChunkKey::new(0, 0, 0), Transitions::NONE, 8.0),
            6_400,
        ),
        (
            regular(
                ChunkKey::new(1, 0, 1),
                Transitions::NONE.with_side(ChunkSide::MinX),
                8.0,
            ),
            12_800,
        ),
    ];
    let tolerance = TinTolerance::GENERAL.general.height as f32;
    for (mut neighbour, line) in cases {
        let ours = on_side(&tin, line, (0, 6_400));
        let before = on_side(&neighbour, line, (0, 6_400));
        assert_eq!(
            ours.keys().collect::<Vec<_>>(),
            before.keys().collect::<Vec<_>>(),
            "both have a vertex every metre"
        );
        // Unfitted, they stand apart by at most the tolerance.
        let gap = ours
            .iter()
            .map(|(z, y)| (y - before[z]).abs())
            .fold(0.0f32, f32::max);
        assert!(gap <= tolerance + 1e-3, "unfitted gap {gap} m");

        // Fitted, they share every vertex exactly, and the skirts keep
        // their depth.
        let moved = chunk.fit_neighbour(&mut neighbour);
        assert_eq!(moved, 65);
        assert_eq!(on_side(&neighbour, line, (0, 6_400)), ours);
        let lowest: f32 = (0..neighbour.positions.len())
            .filter(|&i| ((world(&neighbour, i).x * 100.0).round() as i64) == line)
            .map(|i| neighbour.positions[i].y)
            .fold(f32::INFINITY, f32::min);
        let edge_lowest = ours.values().copied().fold(f32::INFINITY, f32::min);
        assert!((edge_lowest - lowest - 8.0).abs() < 1e-3);
    }
}

#[test]
fn the_collider_is_the_canonical_mesh() {
    for chunk in [hill_chunk(0, 0), off_lattice_chunk()] {
        let collider = chunk.collider();
        assert_eq!(collider.key, chunk.key);
        assert_eq!(collider.origin, chunk.key.min_corner());
        assert_eq!(collider.vertices.len(), chunk.vertices.len());
        assert_eq!(
            collider.indices,
            chunk.triangles.iter().map(|t| t.0).collect::<Vec<_>>()
        );
        for (p, v) in collider.vertices.iter().zip(&chunk.vertices) {
            let expected = Vec3::new(
                ((v.x.0 - chunk.key.x * 6_400) as f64 / 100.0) as f32,
                v.h.to_m() as f32,
                ((v.z.0 - chunk.key.z * 6_400) as f64 / 100.0) as f32,
            );
            assert_eq!(*p, expected);
        }
        // Products never change the canonical mesh or its hash.
        let hash = chunk.content_hash();
        let _ = chunk.render_mesh(&vec![Vec3::Y; chunk.vertices.len()], &ChunkSide::ALL, 4.0);
        assert_eq!(chunk.content_hash(), hash);
    }
}
