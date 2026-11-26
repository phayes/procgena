//! Canonical chunk meshes: millimetre sampling, canonical order and
//! hashes, chunk edge contracts, and refinement to a tolerance.

use std::sync::Arc;

use glam::DVec2;
use procgena_terrain::tin::{
    ChunkEdgeKey, EdgeContract, Mm, MmHeight, ProbeAddr, TinChunk, TinTolerance, TinTriangle,
    build_chunk, sample_heights, triangulate,
};
use procgena_terrain::{
    ChunkKey, ChunkSource, Elevation, FeatureTier, RegionGraphOptions, RegionId, SurfacePoint,
    WorldTerrain, WorldTerrainOptions, fixtures,
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
        max_edge: 100.0,
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
const GOLDEN_PLANE: u64 = 10_738_285_288_521_230_516;

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

#[test]
fn a_small_budget_stops_the_same_way_every_time() {
    let ground = Analytic(hills);
    // Five centimetres: far more than 40 vertices' worth on these hills.
    let tolerance = TinTolerance {
        height: 0.05,
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
