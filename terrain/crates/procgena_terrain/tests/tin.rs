//! Canonical chunk meshes: millimetre sampling, canonical order and
//! hashes, chunk edge contracts, refinement to a tolerance, and river and
//! pond lines kept as edges.

use std::sync::Arc;

use glam::DVec2;
use procgena_terrain::tin::{
    ChunkEdgeKey, ConstraintKind, DomainTolerance, EdgeAxis, EdgeContract, Mm, MmHeight, ProbeAddr,
    TinChunk, TinTolerance, TinTriangle, build_chunk, sample_heights, triangulate,
};
use procgena_terrain::{
    ChunkFeature, ChunkFeatureShape, ChunkKey, ChunkSource, Elevation, FeatureId, FeatureKind,
    FeatureLine, FeatureTier, Pond, RegionGraphOptions, RegionId, SurfacePoint, WorldTerrain,
    WorldTerrainOptions, fixtures,
};

/// Ground given by a function, at the fine tier.
struct Analytic<F>(F);

impl<F: Fn(DVec2) -> f64 + Sync> ChunkSource for Analytic<F> {
    fn surface(&self, p: DVec2, _: FeatureTier) -> SurfacePoint {
        SurfacePoint {
            height: (self.0)(p) as f32,
            water: None,
        }
    }
    fn tier(&self, _: FeatureTier) -> FeatureTier {
        FeatureTier::Fine
    }
    fn tier_difference(&self, _: DVec2, _: DVec2, _: FeatureTier) -> f32 {
        0.0
    }
}

/// A world's ground read one point at a time, through the trait's
/// default `surfaces`.
struct Pointwise<'a>(&'a WorldTerrain);

impl ChunkSource for Pointwise<'_> {
    fn surface(&self, p: DVec2, tier: FeatureTier) -> SurfacePoint {
        self.0.surface(p, tier)
    }
    fn tier(&self, tier: FeatureTier) -> FeatureTier {
        self.0.tier(tier)
    }
    fn tier_difference(&self, min: DVec2, max: DVec2, from: FeatureTier) -> f32 {
        self.0.tier_difference(min, max, from)
    }
}

/// Smooth hills, up to about 30 m.
fn hills(p: DVec2) -> f64 {
    10.0 + 12.0 * (p.x / 37.0).sin() * (p.y / 53.0).cos() + 8.0 * ((p.x + p.y) / 23.0).sin()
}

/// The chunks of the 2 × 2 block round the origin.
const BLOCK: [(i64, i64); 4] = [(-1, -1), (0, -1), (-1, 0), (0, 0)];

/// The height of `mesh` at lattice point `p` (millimetres), if a triangle
/// covers it, from the plane through that triangle's corners.
fn mesh_height(mesh: &TinChunk, p: (i64, i64)) -> Option<f64> {
    mesh.triangles.iter().find_map(|&TinTriangle([a, b, c])| {
        let [a, b, c] = [a, b, c].map(|i| mesh.vertices[i as usize]);
        let orient = |q: (i64, i64), r: (i64, i64), s: (i64, i64)| {
            ((r.0 - q.0) * (s.1 - q.1) - (r.1 - q.1) * (s.0 - q.0)) as f64
        };
        let (pa, pb, pc) = ((a.x.0, a.z.0), (b.x.0, b.z.0), (c.x.0, c.z.0));
        // Triangles face up, so they wind clockwise in (x, z).
        let area = -orient(pa, pb, pc);
        let (wa, wb, wc) = (-orient(pb, pc, p), -orient(pc, pa, p), -orient(pa, pb, p));
        (wa >= 0.0 && wb >= 0.0 && wc >= 0.0)
            .then(|| (wa * f64::from(a.h.0) + wb * f64::from(b.h.0) + wc * f64::from(c.h.0)) / area)
    })
}

/// Checks that `mesh` is a whole, up-facing tiling of its chunk.
fn assert_tiles_its_chunk(mesh: &TinChunk) {
    let mut area = 0.0;
    for &TinTriangle([a, b, c]) in &mesh.triangles {
        let [a, b, c] = [a, b, c].map(|i| mesh.vertices[i as usize]);
        let twice = (b.x.0 - a.x.0) as f64 * (c.z.0 - a.z.0) as f64
            - (b.z.0 - a.z.0) as f64 * (c.x.0 - a.x.0) as f64;
        assert!(twice < 0.0, "triangle {a:?} {b:?} {c:?} faces down");
        area -= twice / 2.0;
    }
    assert_eq!(area, 64_000.0 * 64_000.0, "triangles cover the chunk once");
}

#[test]
fn probes_sample_the_ground_to_the_millimetre() {
    let plane = Analytic(|p: DVec2| 5.0 + 0.25 * p.x - 0.5 * p.y);
    let probes = [
        ProbeAddr::new(Mm(0), Mm(0)),
        ProbeAddr::new(Mm(-64_000), Mm(12_000)),
        ProbeAddr::new(Mm(3_000), Mm(-7_000)),
    ];
    let heights = sample_heights(&plane, &probes).unwrap();
    assert_eq!(
        heights,
        [MmHeight(5_000), MmHeight(-17_000), MmHeight(9_250)]
    );

    // No ground is an error, not a height.
    let none = Analytic(|_: DVec2| f64::NAN);
    assert!(sample_heights(&none, &probes).is_err());
}

#[test]
fn a_plane_needs_only_two_triangles() {
    let plane = Analytic(|p: DVec2| 5.0 + 0.01 * p.x + 0.02 * p.y);
    let tolerance = TinTolerance {
        general: DomainTolerance {
            max_edge: 100.0,
            ..TinTolerance::GENERAL.general
        },
        ..TinTolerance::GENERAL
    };
    for (x, z) in BLOCK {
        let mesh = build_chunk(&plane, ChunkKey::new(x, z, 0), &tolerance).unwrap();
        assert_eq!(mesh.vertices.len(), 4, "chunk ({x}, {z})");
        assert_eq!(mesh.triangles.len(), 2);
        assert_eq!(mesh.constraints.len(), 4);
        assert!(mesh.unmet.is_empty());
        assert_tiles_its_chunk(&mesh);
    }
}

#[test]
fn a_plane_chunk_has_a_fixed_hash() {
    // Ground from plain sums and products, which every platform rounds the
    // same, so this hash is the same everywhere. A change to it changes
    // every canonical mesh: bump the numeric profile when it does.
    let plane = Analytic(|p: DVec2| 5.0 + 0.01 * p.x + 0.02 * p.y);
    let mesh = build_chunk(&plane, ChunkKey::new(-1, 2, 0), &TinTolerance::GENERAL).unwrap();
    assert_eq!(mesh.content_hash(), GOLDEN_PLANE, "{mesh:?}");
}

/// [`a_plane_chunk_has_a_fixed_hash`]'s hash.
const GOLDEN_PLANE: u64 = 2_351_256_048_787_654_546;

#[test]
fn hashes_ignore_the_order_of_vertices() {
    let mesh = build_chunk(
        &Analytic(hills),
        ChunkKey::new(0, 0, 0),
        &TinTolerance::GENERAL,
    )
    .unwrap();

    // The same mesh with its vertices reversed, and its triangles and
    // constraints renumbered and shuffled to match.
    let n = mesh.vertices.len() as u32;
    let mut shuffled = mesh.clone();
    shuffled.vertices.reverse();
    for t in &mut shuffled.triangles {
        t.0 = [t.0[1], t.0[2], t.0[0]].map(|i| n - 1 - i);
    }
    shuffled.triangles.reverse();
    for c in &mut shuffled.constraints {
        c.v = [c.v[1], c.v[0]].map(|i| n - 1 - i);
    }
    assert_ne!(shuffled, mesh);
    assert_eq!(shuffled.normalized(), mesh);
    assert_eq!(shuffled.content_hash(), mesh.content_hash());

    // A changed height changes the hash.
    let mut moved = mesh.clone();
    moved.vertices[0].h.0 += 1;
    assert_ne!(moved.content_hash(), mesh.content_hash());
}

#[test]
fn neighbours_built_alone_in_any_order_share_their_edges() {
    let ground = Analytic(hills);
    let tolerance = TinTolerance::GENERAL;
    let build =
        |(x, z): (i64, i64)| build_chunk(&ground, ChunkKey::new(x, z, 0), &tolerance).unwrap();

    // Every order of building the block gives the same meshes.
    let first: Vec<u64> = BLOCK.iter().map(|&k| build(k).content_hash()).collect();
    let mut orders = vec![[0, 1, 2, 3]];
    for _ in 0..23 {
        // The next permutation, lexicographically.
        let mut p = *orders.last().unwrap();
        let i = (0..3).rev().find(|&i| p[i] < p[i + 1]).unwrap();
        let j = (i + 1..4).rev().find(|&j| p[j] > p[i]).unwrap();
        p.swap(i, j);
        p[i + 1..].reverse();
        orders.push(p);
    }
    for order in orders.iter().step_by(5) {
        let mut hashes = [0u64; 4];
        for &k in order {
            hashes[k] = build(BLOCK[k]).content_hash();
        }
        assert_eq!(hashes.to_vec(), first, "order {order:?}");
    }

    // Neighbours have the same vertices along their shared edges, and
    // corners agree.
    let meshes: Vec<TinChunk> = BLOCK.iter().map(|&k| build(k)).collect();
    let on = |mesh: &TinChunk, edge: ChunkEdgeKey| {
        let (x0, z0) = edge.start();
        let mut v: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| match edge.axis {
                procgena_terrain::tin::EdgeAxis::X => {
                    v.z == z0 && (x0.0..=x0.0 + 64_000).contains(&v.x.0)
                }
                procgena_terrain::tin::EdgeAxis::Z => {
                    v.x == x0 && (z0.0..=z0.0 + 64_000).contains(&v.z.0)
                }
            })
            .copied()
            .collect();
        v.sort();
        v
    };
    for (i, &a) in BLOCK.iter().enumerate() {
        for (j, &b) in BLOCK.iter().enumerate() {
            let shared = ChunkEdgeKey::of_chunk(ChunkKey::new(a.0, a.1, 0))
                .into_iter()
                .find(|e| ChunkEdgeKey::of_chunk(ChunkKey::new(b.0, b.1, 0)).contains(e));
            if let (true, Some(edge)) = (i != j, shared) {
                let (va, vb) = (on(&meshes[i], edge), on(&meshes[j], edge));
                assert!(va.len() >= 2, "an edge has its corners");
                assert_eq!(va, vb, "chunks {a:?} and {b:?} along {edge:?}");
                let contract = EdgeContract::build(&ground, edge, &tolerance).unwrap();
                let mut cv = contract.vertices.clone();
                cv.sort();
                assert_eq!(va, cv, "the edge follows its contract");
            }
        }
    }
}

#[test]
fn smooth_hills_converge_within_the_tolerance() {
    let ground = Analytic(hills);
    let tolerance = TinTolerance::GENERAL;
    for (x, z) in BLOCK {
        let key = ChunkKey::new(x, z, 0);
        let mesh = build_chunk(&ground, key, &tolerance).unwrap();
        assert!(mesh.unmet.is_empty(), "chunk {key:?}");
        assert!(
            mesh.vertices.len() < 4096 / 4,
            "chunk {key:?} has {} vertices",
            mesh.vertices.len()
        );
        assert_tiles_its_chunk(&mesh);

        // Every 1 m sample is within the tolerance (plus rounding).
        for row in 0..=64 {
            for col in 0..=64 {
                let p = (x * 64_000 + col * 1000, z * 64_000 + row * 1000);
                let at = DVec2::new(p.0 as f64, p.1 as f64) / 1000.0;
                let sample = (hills(at) as f32 as f64 * 1000.0).round();
                let height = mesh_height(&mesh, p).expect("the mesh covers the chunk");
                assert!(
                    (height - sample).abs() <= 501.0,
                    "chunk {key:?} at {p:?}: mesh {height} mm, ground {sample} mm"
                );
            }
        }
    }
}

/// Ground from another source, counting the heights read from it.
struct Counted<S> {
    ground: S,
    reads: std::sync::atomic::AtomicUsize,
}

impl<S: ChunkSource> ChunkSource for Counted<S> {
    fn surface(&self, p: DVec2, tier: FeatureTier) -> SurfacePoint {
        self.reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.ground.surface(p, tier)
    }
    fn tier(&self, tier: FeatureTier) -> FeatureTier {
        self.ground.tier(tier)
    }
    fn tier_difference(&self, min: DVec2, max: DVec2, from: FeatureTier) -> f32 {
        self.ground.tier_difference(min, max, from)
    }
    fn lines_within(&self, min: DVec2, max: DVec2, tier: FeatureTier) -> Vec<ChunkFeature> {
        self.ground.lines_within(min, max, tier)
    }
}

/// Gentle swells, up to about 16 m.
fn swells(p: DVec2) -> f64 {
    10.0 + 6.0 * (p.x / 61.0).sin() * (p.y / 47.0).cos()
}

#[test]
fn gentle_ground_is_sampled_sparsely_and_stays_within_the_tolerance() {
    // The four edges are read every metre (65 heights each), and the
    // inside only as finely as the swells bend: far fewer than the 65 × 65
    // lattice plus the edges. Every 1 m point, read or not, is still within
    // the tolerance (plus rounding).
    let ground = Counted {
        ground: Analytic(swells),
        reads: Default::default(),
    };
    let dense = 65 * 65 + 4 * 65;
    for (x, z) in BLOCK {
        ground.reads.store(0, std::sync::atomic::Ordering::Relaxed);
        let mesh = build_chunk(&ground, ChunkKey::new(x, z, 0), &TinTolerance::GENERAL).unwrap();
        let reads = ground.reads.load(std::sync::atomic::Ordering::Relaxed);
        assert!(reads * 4 < dense, "chunk ({x}, {z}) read {reads} heights");
        assert!(mesh.unmet.is_empty());
        for row in 0..=64 {
            for col in 0..=64 {
                let p = (x * 64_000 + col * 1000, z * 64_000 + row * 1000);
                let at = DVec2::new(p.0 as f64, p.1 as f64) / 1000.0;
                let sample = (swells(at) as f32 as f64 * 1000.0).round();
                let height = mesh_height(&mesh, p).expect("the mesh covers the chunk");
                assert!(
                    (height - sample).abs() <= 501.0,
                    "chunk ({x}, {z}) at {p:?}: mesh {height} mm, ground {sample} mm"
                );
            }
        }
    }
}

#[test]
fn a_small_budget_stops_the_same_way_every_time() {
    let ground = Analytic(hills);
    // Five centimetres: far more than 40 vertices' worth on these hills.
    let tolerance = TinTolerance {
        general: DomainTolerance {
            height: 0.05,
            ..TinTolerance::GENERAL.general
        },
        vertex_budget: 40,
        ..TinTolerance::GENERAL
    };
    let key = ChunkKey::new(0, -1, 0);
    let a = build_chunk(&ground, key, &tolerance).unwrap();
    let b = build_chunk(&ground, key, &tolerance).unwrap();
    assert_eq!(a, b);
    // The border (one constraint per border vertex) is always kept; the
    // budget stops what comes after it.
    assert_eq!(
        a.vertices.len(),
        a.constraints.len().max(40),
        "{} constraints, {} unmet",
        a.constraints.len(),
        a.unmet.len()
    );
    assert!(!a.unmet.is_empty(), "40 vertices can't follow these hills");
    assert!(a.unmet.iter().all(|u| u.error.0 > 50));
    assert_tiles_its_chunk(&a);
}

#[test]
fn triangulating_given_vertices_keeps_the_border() {
    let ground = Analytic(hills);
    let tolerance = TinTolerance::GENERAL;
    let key = ChunkKey::new(-1, 0, 0);
    let contracts: Vec<EdgeContract> = ChunkEdgeKey::of_chunk(key)
        .into_iter()
        .map(|e| EdgeContract::build(&ground, e, &tolerance).unwrap())
        .collect();
    let interior: Vec<_> = [(10, 10), (40, 20), (32, 50)]
        .into_iter()
        .map(|(col, row)| procgena_terrain::tin::TinVertex {
            x: Mm(-64_000 + col * 1000),
            z: Mm(row * 1000),
            h: MmHeight(0),
        })
        .collect();
    let mesh = triangulate(
        key,
        [&contracts[0], &contracts[1], &contracts[2], &contracts[3]],
        &interior,
    );
    let border: usize = contracts.iter().map(|c| c.vertices.len() - 1).sum();
    assert_eq!(mesh.vertices.len(), border + interior.len());
    assert_eq!(mesh.constraints.len(), border);
    assert_tiles_its_chunk(&mesh);
}

#[test]
fn a_world_chunk_across_a_region_border_tiles_and_ignores_batching() {
    // A small world of medium-scale plains, and a chunk whose corners lie
    // in different regions.
    let options = WorldTerrainOptions {
        regions: RegionGraphOptions {
            size: 2000.0,
            min_edge: procgena_terrain::MIN_EDGE,
            ..fixtures::regions_medium()
        },
        ..fixtures::world_medium()
    };
    let source = |_: RegionId, _: DVec2, _: &Elevation| fixtures::plains_medium().terrain.clone();
    let world = WorldTerrain::new(options, Arc::new(source)).unwrap();
    let key = (0..64)
        .map(|i| ChunkKey::new(i, 0, 0))
        .find(|k| {
            let (lo, hi) = (k.min_corner(), k.max_corner());
            world.region_at(lo) != world.region_at(DVec2::new(hi.x, lo.y))
        })
        .expect("a region border within 4 km of the origin");
    let tolerance = TinTolerance::GENERAL;

    let mesh = build_chunk(&world, key, &tolerance).unwrap();
    assert_tiles_its_chunk(&mesh);
    assert!(mesh.unmet.is_empty());
    let pointwise = build_chunk(&Pointwise(&world), key, &tolerance).unwrap();
    assert_eq!(pointwise.content_hash(), mesh.content_hash());

    // Its neighbour across the border shares the edge.
    let east = ChunkKey::new(key.x + 1, key.z, 0);
    let neighbour = build_chunk(&world, east, &tolerance).unwrap();
    let edge = ChunkEdgeKey::of_chunk(key)[1];
    let x = edge.start().0;
    let mut a: Vec<_> = mesh.vertices.iter().filter(|v| v.x == x).copied().collect();
    let mut b: Vec<_> = neighbour
        .vertices
        .iter()
        .filter(|v| v.x == x)
        .copied()
        .collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
}

/// Ground given by a function, with rivers and ponds lying on it, each
/// listed for boxes its water's box widened by `margin` metres meets.
struct Featured {
    features: Vec<ChunkFeature>,
    margin: f64,
}

impl ChunkSource for Featured {
    fn surface(&self, p: DVec2, _: FeatureTier) -> SurfacePoint {
        SurfacePoint {
            height: hills(p) as f32,
            water: None,
        }
    }
    fn tier(&self, _: FeatureTier) -> FeatureTier {
        FeatureTier::Fine
    }
    fn tier_difference(&self, _: DVec2, _: DVec2, _: FeatureTier) -> f32 {
        0.0
    }
    fn lines_within(&self, min: DVec2, max: DVec2, _: FeatureTier) -> Vec<ChunkFeature> {
        self.features
            .iter()
            .filter(|f| {
                let (lo, hi) = bounds(f);
                let m = DVec2::splat(self.margin);
                (lo - m).cmple(max).all() && (hi + m).cmpge(min).all()
            })
            .cloned()
            .collect()
    }
}

/// The corners of the box round a feature's water.
fn bounds(feature: &ChunkFeature) -> (DVec2, DVec2) {
    match &feature.shape {
        ChunkFeatureShape::Line(line) => {
            let r = DVec2::splat(f64::from(line.widths[0]) / 2.0);
            let lo = line.points.iter().fold(DVec2::INFINITY, |a, &p| a.min(p));
            let hi = line
                .points
                .iter()
                .fold(DVec2::NEG_INFINITY, |a, &p| a.max(p));
            (lo - r, hi + r)
        }
        ChunkFeatureShape::Pond(pond) => {
            let r = DVec2::splat(pond.radius);
            (pond.centre - r, pond.centre + r)
        }
    }
}

/// A river `width` metres wide along `points` (metres), falling 1 m along
/// its length, with key `key`.
fn river(key: u64, points: &[(f64, f64)], width: f32) -> ChunkFeature {
    let n = points.len();
    ChunkFeature {
        id: FeatureId {
            kind: FeatureKind::River,
            key,
        },
        shape: ChunkFeatureShape::Line(FeatureLine {
            kind: FeatureKind::River,
            points: points.iter().map(|&(x, z)| DVec2::new(x, z)).collect(),
            levels: (0..n).map(|i| 20.0 - i as f32 / n as f32).collect(),
            widths: vec![width; n],
            depth: 1.0,
        }),
    }
}

/// A pond of `radius` metres round `centre`, with key `key`.
fn pond(key: u64, centre: (f64, f64), radius: f64) -> ChunkFeature {
    ChunkFeature {
        id: FeatureId {
            kind: FeatureKind::Pond,
            key,
        },
        shape: ChunkFeatureShape::Pond(Pond {
            centre: DVec2::new(centre.0, centre.1),
            level: 18.0,
            radius,
            depth: 1.5,
        }),
    }
}

/// The vertices of `mesh` on chunk edge `edge`, sorted.
fn on_edge(mesh: &TinChunk, edge: ChunkEdgeKey) -> Vec<procgena_terrain::tin::TinVertex> {
    let (x0, z0) = edge.start();
    let mut v: Vec<_> = mesh
        .vertices
        .iter()
        .filter(|v| match edge.axis {
            EdgeAxis::X => v.z == z0 && (x0.0..=x0.0 + 64_000).contains(&v.x.0),
            EdgeAxis::Z => v.x == x0 && (z0.0..=z0.0 + 64_000).contains(&v.z.0),
        })
        .copied()
        .collect();
    v.sort();
    v
}

/// Builds the 2 × 2 `block` of chunks over `source` in several orders,
/// checks every order gives the same meshes, that neighbours share their
/// edges (crossings included), and that every mesh tiles its chunk and
/// keeps every feature line piece, and returns the meshes.
fn build_block(
    source: &dyn ChunkSource,
    block: [(i64, i64); 4],
    tolerance: &TinTolerance,
) -> Vec<TinChunk> {
    let build =
        |(x, z): (i64, i64)| build_chunk(source, ChunkKey::new(x, z, 0), tolerance).unwrap();
    let meshes: Vec<TinChunk> = block.iter().map(|&k| build(k)).collect();
    for order in [[3, 1, 2, 0], [2, 0, 3, 1], [1, 3, 0, 2]] {
        let mut again = vec![None; 4];
        for k in order {
            again[k] = Some(build(block[k]));
        }
        for (k, mesh) in again.into_iter().enumerate() {
            assert_eq!(
                mesh.unwrap().content_hash(),
                meshes[k].content_hash(),
                "chunk {:?}, order {order:?}",
                block[k]
            );
        }
    }
    for (i, mesh) in meshes.iter().enumerate() {
        assert_tiles_its_chunk(mesh);
        assert!(
            mesh.dropped.is_empty(),
            "chunk {:?}: {:?}",
            block[i],
            mesh.dropped
        );
        for (j, other) in meshes.iter().enumerate() {
            let shared = ChunkEdgeKey::of_chunk(mesh.key)
                .into_iter()
                .find(|e| ChunkEdgeKey::of_chunk(other.key).contains(e));
            if let (true, Some(edge)) = (i != j, shared) {
                assert_eq!(on_edge(mesh, edge), on_edge(other, edge), "{edge:?}");
                // Each crossing of the lowest line has the line's edge on
                // each side the line goes into.
                let contract = EdgeContract::build(source, edge, tolerance).unwrap();
                for crossing in &contract.crossings {
                    assert!(
                        on_edge(mesh, edge)
                            .iter()
                            .any(|v| (v.x, v.z) == crossing.at)
                    );
                    if crossing.kind == ConstraintKind::RiverThalweg {
                        let at = DVec2::new(crossing.at.0.to_m(), crossing.at.1.to_m());
                        let lines = source.lines_within(at, at, FeatureTier::Fine);
                        for m in [mesh, other] {
                            if !enters(&lines, crossing.feature, at, m.key) {
                                continue;
                            }
                            let touches = m.constraints.iter().any(|c| {
                                c.kind == ConstraintKind::RiverThalweg
                                    && c.v.iter().any(|&v| {
                                        let v = m.vertices[v as usize];
                                        (v.x, v.z) == crossing.at
                                    })
                            });
                            assert!(touches, "{crossing:?} in chunk {:?}", m.key);
                        }
                    }
                }
            }
        }
    }
    meshes
}

/// Whether the lowest line of river `feature` among `lines` goes into
/// chunk `key` (more than a millimetre inside) within half a metre of `at`.
fn enters(lines: &[ChunkFeature], feature: FeatureId, at: DVec2, key: ChunkKey) -> bool {
    let (lo, hi) = (key.min_corner() + 0.001, key.max_corner() - 0.001);
    lines
        .iter()
        .filter(|f| f.id == feature)
        .filter_map(|f| match &f.shape {
            ChunkFeatureShape::Line(line) => Some(line),
            ChunkFeatureShape::Pond(_) => None,
        })
        .flat_map(|line| {
            line.points
                .windows(2)
                .map(|w| (w[0], w[1]))
                .collect::<Vec<_>>()
        })
        .any(|(a, b)| {
            (0..=1000).any(|s| {
                let p = a.lerp(b, f64::from(s) / 1000.0);
                p.distance(at) <= 0.5 && p.cmpgt(lo).all() && p.cmplt(hi).all()
            })
        })
}

/// Distance from `p` to segment `a`–`b`, all in metres.
fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-18)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// How far `p` is from the water's edge of `features` (negative inside),
/// leaving out feature `except`.
fn water_gap(features: &[ChunkFeature], p: DVec2, except: Option<usize>) -> f64 {
    let mut gap = f64::INFINITY;
    for (i, f) in features.iter().enumerate() {
        if Some(i) == except {
            continue;
        }
        match &f.shape {
            ChunkFeatureShape::Line(line) => {
                for w in line.points.windows(2) {
                    let hw = f64::from(line.widths[0]) / 2.0;
                    gap = gap.min(segment_distance(p, w[0], w[1]) - hw);
                }
            }
            ChunkFeatureShape::Pond(pond) => gap = gap.min(p.distance(pond.centre) - pond.radius),
        }
    }
    gap
}

/// Checks that `p` (metres) lies within `within` metres of an edge of
/// `mesh` kept for one of `kinds`, if it is inside the chunk away from its
/// border.
fn assert_kept_at(mesh: &TinChunk, p: DVec2, kinds: &[ConstraintKind], within: f64) {
    let (lo, hi) = (mesh.key.min_corner(), mesh.key.max_corner());
    let margin = 0.01;
    if p.x <= lo.x + margin || p.x >= hi.x - margin || p.y <= lo.y + margin || p.y >= hi.y - margin
    {
        return;
    }
    let m = |i: u32| {
        let v = mesh.vertices[i as usize];
        DVec2::new(v.x.to_m(), v.z.to_m())
    };
    let nearest = mesh
        .constraints
        .iter()
        .filter(|c| kinds.contains(&c.kind))
        .map(|c| segment_distance(p, m(c.v[0]), m(c.v[1])))
        .fold(f64::INFINITY, f64::min);
    assert!(
        nearest <= within,
        "{kinds:?} at {p} is {nearest} m from the nearest kept edge in chunk {:?}",
        mesh.key
    );
}

/// Checks that every river's lowest line, and the outline of all the
/// water together, are kept as edges in `meshes`.
fn assert_lines_kept(features: &[ChunkFeature], meshes: &[TinChunk], tolerance: &TinTolerance) {
    let chord = tolerance.chord + 0.002;
    for (i, feature) in features.iter().enumerate() {
        match &feature.shape {
            ChunkFeatureShape::Line(line) => {
                let hw = f64::from(line.widths[0]) / 2.0;
                for w in line.points.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let n = (b - a).normalize().perp();
                    let steps = ((b - a).length() / 0.5).ceil() as usize;
                    for s in 0..=steps {
                        let p = a.lerp(b, s as f64 / steps as f64);
                        for mesh in meshes {
                            assert_kept_at(mesh, p, &[ConstraintKind::RiverThalweg], 0.0015);
                            // The water's edge, where no other water hides it.
                            for side in [1.0, -1.0] {
                                let q = p + n * (side * hw);
                                let others = water_gap(features, q, Some(i));
                                let own = water_gap(&features[i..=i], q, None);
                                if others > 0.2 && own > -0.001 {
                                    assert_kept_at(
                                        mesh,
                                        q,
                                        &[ConstraintKind::RiverEdge, ConstraintKind::PondShore],
                                        chord,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            ChunkFeatureShape::Pond(pond) => {
                for s in 0..720 {
                    let angle = std::f64::consts::TAU * f64::from(s) / 720.0;
                    let q = pond.centre + DVec2::new(angle.cos(), angle.sin()) * pond.radius;
                    if water_gap(features, q, Some(i)) > 0.2 {
                        for mesh in meshes {
                            assert_kept_at(mesh, q, &[ConstraintKind::PondShore], chord);
                        }
                    }
                }
            }
        }
    }
}

/// Builds [`BLOCK`] round `features` (each listed for boxes within 20 m of
/// its water) and checks it (see [`build_block`] and
/// [`assert_lines_kept`]).
fn check_features(features: Vec<ChunkFeature>) -> Vec<TinChunk> {
    check_listed(features, 20.0)
}

/// [`check_features`], with each feature listed for boxes within `margin`
/// metres of its water.
fn check_listed(features: Vec<ChunkFeature>, margin: f64) -> Vec<TinChunk> {
    let tolerance = TinTolerance::GENERAL;
    let source = Featured { features, margin };
    let meshes = build_block(&source, BLOCK, &tolerance);
    assert_lines_kept(&source.features, &meshes, &tolerance);
    meshes
}

#[test]
fn a_bending_river_keeps_its_lines_across_chunks() {
    // A river winding across all four chunks, its points 3 m apart.
    let points: Vec<(f64, f64)> = (0..=40)
        .map(|i| {
            let x = -60.0 + 3.0 * f64::from(i);
            (x, 12.0 * (x / 17.0).sin() + 5.0)
        })
        .collect();
    let meshes = check_features(vec![river(1, &points, 8.0)]);
    // Its water reaches every chunk; its lowest line all but one.
    for mesh in &meshes {
        let kinds: Vec<_> = mesh.constraints.iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&ConstraintKind::RiverEdge), "{:?}", mesh.key);
    }
    let with_thalweg = meshes
        .iter()
        .filter(|m| {
            m.constraints
                .iter()
                .any(|c| c.kind == ConstraintKind::RiverThalweg)
        })
        .count();
    assert_eq!(with_thalweg, 3);
}

#[test]
fn a_river_through_a_chunk_corner() {
    check_features(vec![river(
        1,
        &[(-50.0, -50.0), (0.0, 0.0), (50.0, 50.0)],
        6.0,
    )]);
}

#[test]
fn a_river_along_a_chunk_edge() {
    check_features(vec![river(
        1,
        &[(0.0, -60.0), (0.0, 0.0), (0.0, 60.0)],
        6.0,
    )]);
}

#[test]
fn rivers_crossing_on_a_chunk_edge() {
    // Both lowest lines pass through (0, 20), on the edge x = 0.
    check_features(vec![
        river(1, &[(-30.0, 10.0), (30.0, 30.0)], 4.0),
        river(2, &[(-30.0, 30.0), (30.0, 10.0)], 4.0),
    ]);
}

#[test]
fn a_tributary_ending_a_millimetre_from_its_trunk() {
    // One stops a millimetre short of the trunk's lowest line, one a
    // millimetre past it.
    check_features(vec![
        river(1, &[(-60.0, 20.0), (60.0, 20.0)], 8.0),
        river(2, &[(10.0, -50.0), (10.0, 19.999)], 4.0),
        river(3, &[(-20.0, 60.0), (-20.0, 19.999)], 4.0),
        river(4, &[(-40.0, -50.0), (-40.0, 20.001)], 4.0),
    ]);
}

#[test]
fn a_crossing_a_millimetre_from_an_edge_sample() {
    // The lowest line crosses x = 0 at z = 10.0008 m, a millimetre from the
    // edge's sample at 10 m; the edges of its water likewise.
    check_features(vec![river(1, &[(-40.0, 10.0008), (40.0, 10.0008)], 6.0)]);
}

#[test]
fn rivers_carrying_on_into_each_other_near_an_edge() {
    // One river ends where the next starts, bending there, near the edge
    // x = 0. Each is listed only where its own water reaches, so the edge
    // x = 0 sees the second river alone while the chunks see both.
    for bend in [-0.6, -0.2, 0.3, 0.8] {
        let joint = (-4.5, 10.0);
        let end = (
            joint.0 + 60.0 * f64::cos(bend),
            joint.1 + 60.0 * f64::sin(bend),
        );
        check_listed(
            vec![
                river(1, &[(-60.0, 10.0), joint], 6.0),
                river(2, &[joint, end], 6.0),
            ],
            0.0,
        );
    }
}

#[test]
fn a_river_ending_in_a_pond_on_a_corner() {
    check_features(vec![
        river(1, &[(-50.0, 40.0), (-20.0, 20.0), (-3.0, 2.0)], 5.0),
        pond(2, (1.0, -1.0), 9.0),
    ]);
}

/// Ground with ripples about 12 m long along x, and a straight river
/// across them.
struct Rippled(ChunkFeature);

impl ChunkSource for Rippled {
    fn surface(&self, p: DVec2, _: FeatureTier) -> SurfacePoint {
        SurfacePoint {
            height: (10.0 + 0.4 * (p.x / 2.0).sin()) as f32,
            water: None,
        }
    }
    fn tier(&self, _: FeatureTier) -> FeatureTier {
        FeatureTier::Fine
    }
    fn tier_difference(&self, _: DVec2, _: DVec2, _: FeatureTier) -> f32 {
        0.0
    }
    fn lines_within(&self, _: DVec2, _: DVec2, _: FeatureTier) -> Vec<ChunkFeature> {
        vec![self.0.clone()]
    }
}

#[test]
fn river_lines_follow_the_ground_along_their_length() {
    let source = Rippled(river(1, &[(-60.0, 7.3), (60.0, 7.3)], 6.0));
    let tolerance = TinTolerance::GENERAL;
    let meshes = build_block(&source, BLOCK, &tolerance);
    let mut checked = 0;
    for mesh in &meshes {
        for c in mesh.constraints.iter().filter(|c| {
            matches!(
                c.kind,
                ConstraintKind::RiverThalweg | ConstraintKind::RiverEdge
            )
        }) {
            // Halfway along each kept edge, the mesh is within the water's
            // tolerance of the ground, plus what the ripples bend between
            // samples a metre apart.
            let [a, b] = c.v.map(|i| mesh.vertices[i as usize]);
            let mid = ((a.x.0 + b.x.0) / 2, (a.z.0 + b.z.0) / 2);
            let ground = source
                .surface(
                    DVec2::new(mid.0 as f64, mid.1 as f64) / 1000.0,
                    FeatureTier::Fine,
                )
                .height;
            let height = mesh_height(mesh, mid).unwrap() / 1000.0;
            assert!(
                (height - f64::from(ground)).abs() <= tolerance.water.height + 0.02,
                "{:?} at {mid:?}: mesh {height} m, ground {ground} m",
                c.kind
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "{checked} kept edges");
}

#[test]
fn rippled_banks_stay_within_the_water_tolerance_everywhere() {
    // Ripples a few metres long beside a river: every 1 m point within
    // the water band is within the water's tolerance (plus rounding), and
    // every other one within the open land's, though the open land is
    // sampled sparsely.
    let source = Rippled(river(1, &[(-60.0, 7.3), (60.0, 7.3)], 6.0));
    let tolerance = TinTolerance::GENERAL;
    let features = [source.0.clone()];
    let meshes = build_block(&source, BLOCK, &tolerance);
    let mut near = 0;
    for (mesh, (x, z)) in meshes.iter().zip(BLOCK) {
        assert!(mesh.unmet.is_empty(), "chunk ({x}, {z})");
        for row in 0..=64 {
            for col in 0..=64 {
                let p = (x * 64_000 + col * 1000, z * 64_000 + row * 1000);
                let at = DVec2::new(p.0 as f64, p.1 as f64) / 1000.0;
                let ground = f64::from(source.surface(at, FeatureTier::Fine).height);
                let height = mesh_height(mesh, p).expect("the mesh covers its chunk");
                let limit = if water_gap(&features, at, None) < tolerance.water_band {
                    near += 1;
                    tolerance.water.height
                } else {
                    tolerance.general.height
                };
                assert!(
                    (height - (ground * 1000.0).round()).abs() <= limit * 1000.0 + 1.0,
                    "chunk ({x}, {z}) at {at}: mesh {height} mm, ground {ground} m"
                );
            }
        }
    }
    assert!(near > 1000, "{near} points near the water");
}

/// A world of medium-scale rolling hills, built once and shared.
fn river_world() -> &'static WorldTerrain {
    static WORLD: std::sync::OnceLock<WorldTerrain> = std::sync::OnceLock::new();
    WORLD.get_or_init(|| {
        let source =
            |_: RegionId, _: DVec2, _: &Elevation| fixtures::rolling_hills_medium().terrain.clone();
        WorldTerrain::new(fixtures::world_medium(), Arc::new(source)).unwrap()
    })
}

#[test]
fn a_world_lists_each_feature_in_a_box_once() {
    let world = river_world();
    let terrain = world.terrain(world.region_at(DVec2::ZERO)).unwrap();
    for (min, max) in [
        (DVec2::new(-300.0, -300.0), DVec2::new(300.0, 300.0)),
        (DVec2::new(-2000.0, -2000.0), DVec2::new(2000.0, 2000.0)),
    ] {
        let lines = world.lines_within(min, max, FeatureTier::Fine);
        // Sorted by id, each once.
        assert!(lines.windows(2).all(|w| w[0].id <= w[1].id && w[0] != w[1]));
        // Every feature of the region at the centre is there.
        for feature in terrain.lines_within(min, max, FeatureTier::Fine) {
            assert!(lines.contains(&feature), "{:?} is missing", feature.id);
        }
    }
}

#[test]
fn world_rivers_are_lines_in_the_ground() {
    let world = river_world();
    let terrain = world.terrain(world.region_at(DVec2::ZERO)).unwrap();
    let river = terrain
        .rivers()
        .iter()
        .max_by_key(|r| r.points.len())
        .expect("the region at the origin has a river");

    // The four chunks from the one holding the river's middle point.
    let mid = river.points[river.points.len() / 2];
    let (x, z) = ((mid.x / 64.0).floor() as i64, (mid.y / 64.0).floor() as i64);
    let block = [(x, z), (x + 1, z), (x, z + 1), (x + 1, z + 1)];
    let tolerance = TinTolerance::GENERAL;
    let meshes = build_block(world, block, &tolerance);

    // The lowest line and the water's outline are kept.
    let (lo, hi) = (
        ChunkKey::new(x, z, 0).min_corner(),
        ChunkKey::new(x + 1, z + 1, 0).max_corner(),
    );
    let water: Vec<ChunkFeature> = world
        .lines_within(lo, hi, FeatureTier::Fine)
        .into_iter()
        .filter(|f| f.id.kind != FeatureKind::Road)
        .collect();
    assert!(water.iter().any(|f| f.id.kind == FeatureKind::River));
    assert_lines_kept(&water, &meshes, &tolerance);

    // The mesh's bed stays below the water, at the lowest line's vertices
    // and halfway between them.
    let mut checked = 0;
    for mesh in &meshes {
        for c in mesh
            .constraints
            .iter()
            .filter(|c| c.kind == ConstraintKind::RiverThalweg)
        {
            let [a, b] = c.v.map(|i| mesh.vertices[i as usize]);
            for (px, pz) in [
                (a.x.0, a.z.0),
                (b.x.0, b.z.0),
                ((a.x.0 + b.x.0) / 2, (a.z.0 + b.z.0) / 2),
            ] {
                let at = DVec2::new(px as f64, pz as f64) / 1000.0;
                let level = world
                    .water_level(at)
                    .unwrap_or_else(|| panic!("no water on the lowest line at {at}"));
                let height = mesh_height(mesh, (px, pz)).expect("the mesh covers its chunk");
                assert!(
                    height <= f64::from(level) * 1000.0 + 1.0,
                    "the bed at {at} is {height} mm, above the water at {level} m"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "the lowest line crosses the block");
}
