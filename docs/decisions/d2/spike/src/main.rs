//! GPU spike for decision gate D2. Run from this folder:
//!
//! ```text
//! cargo run --release -- all        # every check, results on stdout
//! cargo run --release -- oracle     # CPU-only checks of K_d and the slot hashes
//! cargo run --release -- parity     # CPU/GPU bit-for-bit parity
//! cargo run --release -- order      # build order and chunk size (window B on the GPU)
//! cargo run --release -- bench      # GPU and CPU timings, windows A and B
//! cargo run --release -- slots      # jump hash and rendezvous on the GPU
//! cargo run --release -- masks      # mask resolution study (CPU)
//! cargo run --release -- png        # grass clumping images in out/
//! ```

mod cpu;
mod gpu;
mod int;
mod png;
mod slots;
mod stack;

use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

use cpu::ChunkOut;
use gpu::{Dispatch, Gpu, Kernel};
use int::{Cand, FP, Grid, Rect, cell_hash, conflict, hash, hash2, lattice_pos};
use stack::{BOULDERS, GRASS, Inputs, Kind, LOGS, NAMES, Spec, TREES, Windows, stack};

/// Footprint (and available-mask) texels: 0.5 m.
const TFS: u32 = 7;

fn chunk_rect(cx: i32, cy: i32, size_m: i32) -> Rect {
    let s = size_m * FP;
    Rect::new(cx * s, cy * s, (cx + 1) * s, (cy + 1) * s)
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let what = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let t = Instant::now();
    let inp = Inputs::new(9);
    println!(
        "inputs: coverage and slope at 2 m over [-{0}, {1}) m, built in {2:.2?}",
        stack::WORLD_PAD,
        1024 + stack::WORLD_PAD,
        t.elapsed()
    );
    let needs_gpu = matches!(
        what.as_str(),
        "all" | "parity" | "order" | "bench" | "slots"
    );
    let g = needs_gpu.then(|| {
        let g = Gpu::new(&stack(5), &inp);
        println!(
            "GPU: {} via {}, timestamp queries: {}",
            g.adapter_name, g.backend, g.timestamps
        );
        g
    });
    match what.as_str() {
        "oracle" => oracle(&inp),
        "parity" => parity(g.as_ref().unwrap(), &inp),
        "order" => order(g.as_ref().unwrap(), &inp),
        "bench" => bench(g.as_ref().unwrap(), &inp),
        "slots" => slots_cmd(g.as_ref().unwrap()),
        "masks" => masks(),
        "png" => png_cmd(&inp),
        "all" => {
            oracle(&inp);
            parity(g.as_ref().unwrap(), &inp);
            order(g.as_ref().unwrap(), &inp);
            bench(g.as_ref().unwrap(), &inp);
            slots_cmd(g.as_ref().unwrap());
            masks();
            png_cmd(&inp);
        }
        other => panic!("unknown command {other}"),
    }
}

// ---------------------------------------------------------------------------
// Oracles: K_d against greedy, and the slot hashes against the prototype.

fn oracle(inp: &Inputs) {
    println!("\n== Oracle (CPU): K_d against the greedy rule, trees, 128 m square, 48 m pad");
    let seed = 1;
    let region = Rect::new(256 * FP, 256 * FP, 384 * FP, 384 * FP);
    let run = |p24_scale: f64, d_list: &[u32]| {
        let mut specs = stack(1);
        if let Kind::Lattice { p24, .. } = &mut specs[TREES].kind {
            *p24 = (*p24 as f64 * p24_scale).round() as u32;
        }
        let own = stack::cells_with_corner_in(&specs[TREES], region);
        let win = own.grow(96);
        let avail = cpu::product(
            &specs[TREES],
            TREES,
            inp,
            stack::bilinear_texels(&specs[TREES], win, TFS),
            TFS,
            &Default::default(),
        );
        let mut cand = Grid::new(win, Cand::default());
        for j in win.y0..win.y1 {
            for i in win.x0..win.x1 {
                cand.set(
                    i,
                    j,
                    cpu::candidate(&specs[TREES], seed, inp, &avail, TFS, i, j),
                );
            }
        }
        let g = cpu::greedy(&specs[TREES], &cand);
        let kds: Vec<Grid<u8>> = d_list
            .iter()
            .map(|&d| cpu::kd(&specs[TREES], &cand, d))
            .collect();
        (specs, own, cand, g, kds)
    };
    let set = |g: &Grid<u8>, own: Rect| -> HashSet<(i32, i32)> {
        let mut s = HashSet::new();
        for j in own.y0..own.y1 {
            for i in own.x0..own.x1 {
                if g.get(i, j) != 0 {
                    s.insert((i, j));
                }
            }
        }
        s
    };
    let violations = |s: &HashSet<(i32, i32)>, cand: &Grid<Cand>, r: i32| -> usize {
        let mut n = 0;
        for &(i, j) in s {
            for dj in -r..=r {
                for di in -r..=r {
                    let b = (i + di, j + dj);
                    if b > (i, j)
                        && s.contains(&b)
                        && conflict(&cand.get(i, j), &cand.get(b.0, b.1))
                    {
                        n += 1;
                    }
                }
            }
        }
        n
    };
    let ds = [1u32, 2, 3, 4, 5, 7, 9];
    let (specs, own, cand, g, kds) = run(1.0, &ds);
    let gs = set(&g, own);
    let r = specs[TREES].r();
    println!(
        "  greedy keeps {} trees, spacing violations {}",
        gs.len(),
        violations(&gs, &cand, r)
    );
    let mut prev_odd: Option<HashSet<(i32, i32)>> = None;
    for (d, k) in ds.iter().zip(&kds) {
        let ks = set(k, own);
        let nested = if d % 2 == 1 {
            let ok = prev_odd.as_ref().is_none_or(|p| p.is_subset(&ks));
            prev_odd = Some(ks.clone());
            format!(", K_(d-2) subset {ok}")
        } else {
            format!(", superset of greedy {}", gs.is_subset(&ks))
        };
        println!(
            "  K_{d}: keeps {} = {:.4} of greedy, differs at {}, subset of greedy {}, violations {}{nested}",
            ks.len(),
            ks.len() as f64 / gs.len() as f64,
            ks.symmetric_difference(&gs).count(),
            ks.is_subset(&gs),
            violations(&ks, &cand, r)
        );
    }
    // Nesting in global density: fewer candidates kept only removes trees.
    for d in [3u32, 5] {
        let (_, own, _, _, hi) = run(1.0, &[d]);
        let (_, _, _, _, lo) = run(0.4, &[d]);
        println!(
            "  K_{d}: survivors at 40% density are a subset of those at 100%: {}",
            set(&lo[0], own).is_subset(&set(&hi[0], own))
        );
    }

    println!(
        "\n== Oracle (CPU): slot hashes, 200,000 keys (compare docs/decisions/d2/slots_prototype.out)"
    );
    let keys: Vec<u32> = (0..200_000u32).map(slots::key).collect();
    for (name, f) in [
        ("jump32", slots::jump32 as fn(u32, u32) -> u32),
        ("rendezvous", slots::rendezvous),
    ] {
        for n in [4u32, 8, 16, 32, 64] {
            let a: Vec<u32> = keys.iter().map(|&k| f(k, n)).collect();
            let b: Vec<u32> = keys.iter().map(|&k| f(k, n + 1)).collect();
            let mut counts = vec![0usize; n as usize];
            for &s in &a {
                counts[s as usize] += 1;
            }
            let moved = a.iter().zip(&b).filter(|(x, y)| x != y).count();
            let bad = a.iter().zip(&b).filter(|(x, y)| x != y && **y != n).count();
            let even = keys.len() as f64 / n as f64;
            println!(
                "  {name:10} N={n:3}: slot share min {:.3} max {:.3} of even; N+1 moves {:.4} (ideal {:.4}); moves not to the new slot: {bad}",
                *counts.iter().min().unwrap() as f64 / even,
                *counts.iter().max().unwrap() as f64 / even,
                moved as f64 / keys.len() as f64,
                1.0 / (n + 1) as f64
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Parity: GPU against the CPU twin, bit for bit.

#[derive(Default)]
struct Diff {
    avail_texels: usize,
    avail_diff: usize,
    cand_cells: usize,
    cand_diff: usize,
    kept_copies: usize,
    kept_diff: usize,
    mask_texels: usize,
    mask_diff: usize,
    grass_slots: usize,
    grass_kept: usize,
    grass_diff: usize,
}

fn compare(a: &ChunkOut, b: &ChunkOut, d: &mut Diff) {
    fn cmp<T: PartialEq>(x: &Grid<T>, y: &Grid<T>) -> usize {
        assert_eq!(x.rect, y.rect);
        x.data.iter().zip(&y.data).filter(|(p, q)| p != q).count()
    }
    for k in 0..4 {
        if let (Some(x), Some(y)) = (&a.avail[k], &b.avail[k]) {
            d.avail_texels += x.data.len();
            d.avail_diff += cmp(x, y);
        }
    }
    for k in 0..3 {
        if let (Some(x), Some(y)) = (&a.lattice[k], &b.lattice[k]) {
            d.cand_cells += x.cand.data.len();
            d.cand_diff += cmp(&x.cand, &y.cand);
            d.kept_copies += x.own.data.iter().filter(|&&v| v != 0).count();
            d.kept_diff += cmp(&x.kept, &y.kept);
        }
        if let (Some(x), Some(y)) = (&a.masks[k], &b.masks[k]) {
            d.mask_texels += x.data.len();
            d.mask_diff += cmp(x, y);
        }
    }
    if let (Some(x), Some(y)) = (&a.grass, &b.grass) {
        assert_eq!(x.len(), y.len());
        d.grass_slots += x.len();
        d.grass_kept += x.iter().filter(|p| p.0 != i32::MIN).count();
        d.grass_diff += x.iter().zip(y).filter(|(p, q)| p != q).count();
    }
}

fn parity(g: &Gpu, inp: &Inputs) {
    println!("\n== Parity: GPU against the CPU twin, window A, 64 m chunks");
    let mut all_ok = true;
    let runs: Vec<(u32, u32, i32)> = vec![
        (1, 1, 10),
        (3, 1, 10),
        (5, 1, 10),
        (7, 1, 10),
        (5, 0xC0FFEE, 5),
        (5, 0xDEADBEEF, 5),
    ];
    for (tiled, (d, seed, side)) in [true, false]
        .iter()
        .flat_map(|&t| runs.iter().map(move |&r| (t, r)))
    {
        g.kd_tiled.set(tiled);
        let specs = stack(d);
        let mut diff = Diff::default();
        let t = Instant::now();
        for cy in 0..side {
            for cx in 0..side {
                let w = stack::window_a(&specs, chunk_rect(cx, cy, 64), GRASS, TFS);
                let c = cpu::build(&specs, seed, inp, &w);
                let (gpu_out, _) = g.build_a(&specs, seed, &w);
                compare(&c, &gpu_out, &mut diff);
            }
        }
        let bad =
            diff.avail_diff + diff.cand_diff + diff.kept_diff + diff.mask_diff + diff.grass_diff;
        all_ok &= bad == 0;
        println!(
            "  {} K_d, d={d} seed={seed:#x}, {} chunks ({:.1?}): available-mask texels {} (diff {}), candidates {} (diff {}), \
             kept lattice copies {} (kept-flag diff {}), footprint texels {} (diff {}), grass slots {} with {} kept (diff {})",
            if tiled { "tiled" } else { "naive" },
            side * side,
            t.elapsed(),
            diff.avail_texels,
            diff.avail_diff,
            diff.cand_cells,
            diff.cand_diff,
            diff.kept_copies,
            diff.kept_diff,
            diff.mask_texels,
            diff.mask_diff,
            diff.grass_slots,
            diff.grass_kept,
            diff.grass_diff
        );
    }
    g.kd_tiled.set(true);
    println!(
        "  parity: {}",
        if all_ok {
            "PASS (zero differences)"
        } else {
            "FAIL"
        }
    );
}

// ---------------------------------------------------------------------------
// Order and chunk size: window B on the GPU in three orders and three chunk
// sizes, and window A per chunk, against one CPU build of the whole area.

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
struct Digest {
    count: u64,
    sum: u64,
    xor: u64,
}

impl Digest {
    fn add(&mut self, i: i32, j: i32, point: u32, x: i32, y: i32) {
        let h = hash2(
            hash2(i as u32, j as u32),
            hash2(point, hash2(x as u32, y as u32)),
        );
        let v = (h as u64) << 32 | hash(h ^ 0x5BD1_E995) as u64;
        self.count += 1;
        self.sum = self.sum.wrapping_add(v);
        self.xor ^= v;
    }
}

type Copies = BTreeMap<(usize, i32, i32), (i32, i32)>;

/// Lattice copies kept in `own` whose cell corner lies in `region`.
fn owned_copies(
    specs: &[Spec; 4],
    k: usize,
    own: &Grid<u8>,
    pos: impl Fn(i32, i32) -> (i32, i32),
    region: Rect,
    out: &mut Copies,
) {
    let r = stack::cells_with_corner_in(&specs[k], region);
    for j in r.y0..r.y1 {
        for i in r.x0..r.x1 {
            if own.get(i, j) != 0 {
                out.insert((k, i, j), pos(i, j));
            }
        }
    }
}

fn grass_digest(
    own: Rect,
    n_keep: u32,
    slots: &[(i32, i32)],
    region: Rect,
    cs: u32,
    dg: &mut Digest,
) {
    for (n, &(x, y)) in slots.iter().enumerate() {
        if x == i32::MIN {
            continue;
        }
        let cell = n as u32 / n_keep;
        let i = own.x0 + (cell % own.w() as u32) as i32;
        let j = own.y0 + (cell / own.w() as u32) as i32;
        if region.contains(i << cs, j << cs) {
            dg.add(i, j, n as u32 % n_keep, x, y);
        }
    }
}

fn order_list(side: i32, kind: &str) -> Vec<(i32, i32)> {
    let mut v: Vec<(i32, i32)> = (0..side)
        .flat_map(|y| (0..side).map(move |x| (x, y)))
        .collect();
    match kind {
        "row" => {}
        "spiral" => {
            let c = (side as f64 - 1.0) / 2.0;
            v.sort_by(|a, b| {
                let ra = (a.0 as f64 - c).abs().max((a.1 as f64 - c).abs());
                let rb = (b.0 as f64 - c).abs().max((b.1 as f64 - c).abs());
                let ta = (a.1 as f64 - c).atan2(a.0 as f64 - c);
                let tb = (b.1 as f64 - c).atan2(b.0 as f64 - c);
                ra.partial_cmp(&rb)
                    .unwrap()
                    .then(ta.partial_cmp(&tb).unwrap())
            });
        }
        _ => v.sort_by_key(|&(x, y)| hash2(x as u32 ^ 0xABCD, y as u32)),
    }
    v
}

struct BBuild<'a> {
    g: &'a Gpu,
    specs: [Spec; 4],
    seed: u32,
    size_m: i32,
    built: [HashSet<(i32, i32)>; 4],
    grass: Digest,
    region: Rect,
    /// GPU nanoseconds per Scatter build, for the bench.
    times: [Vec<f64>; 4],
    read_grass: bool,
    /// False for the negative control: chunks above are not placed first.
    deps: bool,
}

impl BBuild<'_> {
    /// Places Scatter `k` in chunk `c` over window B, first placing every
    /// chunk of the Scatters above whose copies its masks need.
    fn ensure(&mut self, k: usize, c: (i32, i32)) {
        if self.built[k].contains(&c) {
            return;
        }
        let chunk = chunk_rect(c.0, c.1, self.size_m);
        let w = stack::window_b(&self.specs, chunk, k, TFS);
        let csize = self.size_m * FP;
        for j in 0..if self.deps { k.min(3) } else { 0 } {
            let cells = stack::cells_reaching(&self.specs[j], w.avail[k], TFS);
            let cs = self.specs[j].cs;
            let (x0, y0) = (
                (cells.x0 << cs).div_euclid(csize),
                (cells.y0 << cs).div_euclid(csize),
            );
            let (x1, y1) = (
                ((cells.x1 - 1) << cs).div_euclid(csize),
                ((cells.y1 - 1) << cs).div_euclid(csize),
            );
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    self.ensure(j, (cx, cy));
                }
            }
        }
        let mut ds: Vec<Dispatch> = (0..k.min(3))
            .map(|j| self.g.stamp(&self.specs, self.seed, TFS, j, w.avail[k]))
            .collect();
        ds.extend(
            self.g
                .place(&self.specs, self.seed, &w, k, &[w.avail[k]; 3]),
        );
        let grass_len = if k == GRASS && self.read_grass {
            w.own[k].len() * 800 * 2
        } else {
            0
        };
        let read = (grass_len > 0).then_some((&[Rect::EMPTY; 3][..], grass_len));
        let (t, data) = self.g.run(&ds, &[], read);
        self.times[k].push(t.pass_ns.iter().sum());
        if grass_len > 0 {
            let slots: Vec<(i32, i32)> = data[3]
                .chunks_exact(2)
                .take(grass_len / 2)
                .map(|c| (c[0] as i32, c[1] as i32))
                .collect();
            grass_digest(
                w.own[k],
                800,
                &slots,
                self.region,
                self.specs[GRASS].cs,
                &mut self.grass,
            );
        }
        self.built[k].insert(c);
    }

    fn world_copies(&self) -> Copies {
        let mut out = Copies::new();
        for k in 0..3 {
            let rect = gpu::wk_rect(&self.specs[k]);
            let data = read_buffer(self.g, &self.g.wk[k], rect.len() as u64 * 4);
            let own = Grid {
                rect,
                data: data.iter().map(|&v| v as u8).collect(),
            };
            let s = self.specs[k];
            let seed = self.seed;
            owned_copies(
                &self.specs,
                k,
                &own,
                |i, j| lattice_pos(cell_hash(seed, s.id, i, j), s.cs, i, j),
                self.region,
                &mut out,
            );
        }
        out
    }
}

fn read_buffer(g: &Gpu, src: &wgpu::Buffer, size: u64) -> Vec<u32> {
    let b = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = g.device.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(src, 0, &b, 0, size);
    g.queue.submit([enc.finish()]);
    b.slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    g.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let v = b.slice(..).get_mapped_range().expect("mapped");
    v.chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn order(g: &Gpu, inp: &Inputs) {
    let d = 5;
    let specs = stack(d);
    let seed = 1;
    let region = Rect::new(0, 0, 640 * FP, 640 * FP);
    println!("\n== Order and chunk size: 640 m square (100 chunks of 64 m), d = {d}");

    // The reference: the whole area as one CPU build.
    let t = Instant::now();
    let w = stack::window_a(&specs, region, GRASS, TFS);
    let mut ref_grass = Digest::default();
    let whole = cpu::build_with(&specs, seed, inp, &w, |s, avail, own| {
        cpu::grass_each(s, seed, inp, avail, TFS, own, |i, j, point, kept| {
            if let Some((x, y)) = kept {
                ref_grass.add(i, j, point, x, y);
            }
        });
        None
    });
    let mut reference = Copies::new();
    for k in 0..3 {
        let l = whole.lattice[k].as_ref().unwrap();
        owned_copies(
            &specs,
            k,
            &l.own,
            |i, j| (l.cand.get(i, j).x, l.cand.get(i, j).y),
            region,
            &mut reference,
        );
    }
    let count = |c: &Copies, k: usize| c.keys().filter(|key| key.0 == k).count();
    println!(
        "  whole-area CPU build ({:.1?}): trees {}, boulders {}, logs {}, grass {} (digest)",
        t.elapsed(),
        count(&reference, TREES),
        count(&reference, BOULDERS),
        count(&reference, LOGS),
        ref_grass.count
    );
    let mut all_ok = true;
    let mut report = |label: String, copies: &Copies, grass: Digest| {
        let diff = copies
            .iter()
            .filter(|(k, v)| reference.get(k) != Some(v))
            .count()
            + reference
                .iter()
                .filter(|(k, _)| !copies.contains_key(k))
                .count();
        let ok = diff == 0 && grass == ref_grass;
        all_ok &= ok;
        println!(
            "  {label}: lattice copies differing {diff}, grass digest equal {} -> {}",
            grass == ref_grass,
            if ok { "same" } else { "DIFFERENT" }
        );
    };

    // Window B, every chunk size and order.
    for size in [32, 64, 128] {
        let side = 640 / size;
        for kind in ["row", "spiral", "random"] {
            g.clear_world();
            let t = Instant::now();
            let mut b = BBuild {
                g,
                specs,
                seed,
                size_m: size,
                built: Default::default(),
                grass: Digest::default(),
                region,
                times: Default::default(),
                read_grass: true,
                deps: true,
            };
            for c in order_list(side, kind) {
                for k in 0..4 {
                    b.ensure(k, c);
                }
            }
            let copies = b.world_copies();
            report(
                format!(
                    "GPU window B, {size} m chunks, {kind} order ({:.1?})",
                    t.elapsed()
                ),
                &copies,
                b.grass,
            );
        }
    }
    // Negative control: window B without placing the chunks above first must
    // differ, or the comparison could not see a scheduling error.
    {
        g.clear_world();
        let mut b = BBuild {
            g,
            specs,
            seed,
            size_m: 64,
            built: Default::default(),
            grass: Digest::default(),
            region,
            times: Default::default(),
            read_grass: true,
            deps: false,
        };
        for c in order_list(10, "row") {
            for k in 0..4 {
                b.ensure(k, c);
            }
        }
        let copies = b.world_copies();
        let diff = copies
            .iter()
            .filter(|(k, v)| reference.get(k) != Some(v))
            .count()
            + reference
                .iter()
                .filter(|(k, _)| !copies.contains_key(k))
                .count();
        println!(
            "  negative control (window B, 64 m, row order, chunks above NOT placed first): lattice copies differing {diff}, grass digest equal {} (expected: differences)",
            b.grass == ref_grass
        );
    }
    // Window A, each chunk on its own.
    for size in [32, 64, 128] {
        let side = 640 / size;
        let t = Instant::now();
        let mut copies = Copies::new();
        let mut dg = Digest::default();
        for (cx, cy) in order_list(side, "random") {
            let chunk = chunk_rect(cx, cy, size);
            let w = stack::window_a(&specs, chunk, GRASS, TFS);
            let (out, _) = g.build_a(&specs, seed, &w);
            for k in 0..3 {
                let l = out.lattice[k].as_ref().unwrap();
                owned_copies(
                    &specs,
                    k,
                    &l.own,
                    |i, j| (l.cand.get(i, j).x, l.cand.get(i, j).y),
                    chunk,
                    &mut copies,
                );
            }
            grass_digest(
                w.own[GRASS],
                800,
                out.grass.as_ref().unwrap(),
                chunk,
                specs[GRASS].cs,
                &mut dg,
            );
        }
        report(
            format!(
                "GPU window A, {size} m chunks, random order ({:.1?})",
                t.elapsed()
            ),
            &copies,
            dg,
        );
    }
    println!(
        "  order and chunk size: {}",
        if all_ok {
            "PASS (zero differences)"
        } else {
            "FAIL"
        }
    );
}

// ---------------------------------------------------------------------------
// Timings.

fn stage_name(k: Kernel) -> &'static str {
    match k {
        Kernel::Product => "mask product",
        Kernel::Candidates => "candidates+density+footing",
        Kernel::KdRound | Kernel::KdTiled => "K_d rounds",
        Kernel::Commit => "commit",
        Kernel::Stamp => "stamp",
        Kernel::Grass => "grass candidates",
    }
}

fn window_none_dispatches(g: &Gpu, specs: &[Spec; 4], seed: u32, w: &Windows) -> Vec<Dispatch> {
    let mut ds = Vec::new();
    for k in 0..4 {
        ds.extend(g.place(specs, seed, w, k, &w.mask));
        if k < 3 {
            ds.push(g.stamp(specs, seed, TFS, k, w.mask[k]));
        }
    }
    ds
}

struct Summary {
    total: Vec<f64>,
    span: Vec<f64>,
    wall: Vec<f64>,
    by: BTreeMap<(usize, &'static str), Vec<f64>>,
}

fn summarise(runs: &[(Vec<Dispatch>, gpu::Timing)]) -> Summary {
    let mut s = Summary {
        total: vec![],
        span: vec![],
        wall: vec![],
        by: BTreeMap::new(),
    };
    for (ds, t) in runs {
        s.total.push(t.pass_ns.iter().sum());
        s.span.push(t.span_ns);
        s.wall.push(t.wall_ns);
        let mut per: BTreeMap<(usize, &'static str), f64> = BTreeMap::new();
        for (d, ns) in ds.iter().zip(&t.pass_ns) {
            *per.entry((d.scatter, stage_name(d.kernel))).or_default() += ns;
        }
        for (k, v) in per {
            s.by.entry(k).or_default().push(v);
        }
    }
    s
}

fn bench(g: &Gpu, inp: &Inputs) {
    let seed = 1;
    let interior: Vec<(i32, i32)> = (4..12).flat_map(|y| (4..12).map(move |x| (x, y))).collect();
    println!(
        "\n== Bench: 64 m chunks, {} interior chunks of the 1 km World, GPU times from timestamp queries",
        interior.len()
    );
    let ms = |ns: f64| ns / 1e6;

    // Cold: the first chunk after the device and pipelines exist.
    let specs = stack(5);
    let w = stack::window_a(&specs, chunk_rect(4, 4, 64), GRASS, TFS);
    let (ds, _) = g.window_a(&specs, seed, &w);
    let (t, _) = g.run(&ds, &[], None);
    println!(
        "  cold first chunk (window A, whole stack): GPU {:.3} ms (sum of passes), wall {:.3} ms",
        ms(t.pass_ns.iter().sum()),
        ms(t.wall_ns)
    );
    for _ in 0..30 {
        g.run(&ds, &[], None);
    }

    // Window areas.
    for k in 0..4 {
        let s = &specs[k];
        let a = w.win[k].len() as f64 * (s.cell() as f64 / FP as f64).powi(2) / 4096.0;
        let wb = stack::window_b(&specs, chunk_rect(4, 4, 64), k, TFS);
        let b = wb.win[k].len() as f64 * (s.cell() as f64 / FP as f64).powi(2) / 4096.0;
        println!(
            "  {:8}: window A {:.0} x {:.0} m ({a:.2}x the chunk), window B {:.0} m square ({b:.2}x)",
            NAMES[k],
            w.win[k].w() as f64 * s.cell() as f64 / FP as f64,
            w.win[k].h() as f64 * s.cell() as f64 / FP as f64,
            wb.win[k].w() as f64 * s.cell() as f64 / FP as f64
        );
    }

    for d in [5u32, 3, 7] {
        let specs = stack(d);
        // Window A and no margin.
        let mut runs_a = Vec::new();
        let mut runs_none = Vec::new();
        for &(cx, cy) in &interior {
            let chunk = chunk_rect(cx, cy, 64);
            let w = stack::window_a(&specs, chunk, GRASS, TFS);
            let (ds, _) = g.window_a(&specs, seed, &w);
            let (t, _) = g.run(&ds, &[], None);
            runs_a.push((ds, t));
            let wn = stack::window_none(&specs, chunk, TFS);
            let ds = window_none_dispatches(g, &specs, seed, &wn);
            let (t, _) = g.run(&ds, &[], None);
            runs_none.push((ds, t));
        }
        // Window B, steady state: each Scatter placed once per chunk, level by
        // level over rings wide enough for the Scatters below.
        g.clear_world();
        let mut b = BBuild {
            g,
            specs,
            seed,
            size_m: 64,
            built: Default::default(),
            grass: Digest::default(),
            region: Rect::EMPTY,
            times: Default::default(),
            read_grass: false,
            deps: true,
        };
        for (k, ring) in [(TREES, 3), (BOULDERS, 2), (LOGS, 1), (GRASS, 0)] {
            for y in 4 - ring..12 + ring {
                for x in 4 - ring..12 + ring {
                    b.ensure(k, (x, y));
                }
            }
        }
        let mut sa = summarise(&runs_a);
        let mut sn = summarise(&runs_none);
        let mut b_total = 0.0;
        let mut b_parts = Vec::new();
        for k in 0..4 {
            let m = median(&mut b.times[k]);
            b_total += m;
            b_parts.push(format!("{} {:.3}", NAMES[k], ms(m)));
        }
        let a_med = median(&mut sa.total);
        let n_med = median(&mut sn.total);
        println!("\n  d = {d}:");
        println!(
            "    whole stack, window A: median {:.3} ms, p90 {:.3} ms (first-to-last timestamp span {:.3} ms; wall per submit {:.3} ms)",
            ms(a_med),
            ms(pct(&mut sa.total, 0.9)),
            ms(median(&mut sa.span)),
            ms(median(&mut sa.wall))
        );
        println!(
            "    whole stack, window B: {:.3} ms ({})",
            ms(b_total),
            b_parts.join(", ")
        );
        println!(
            "    whole stack, no margin: median {:.3} ms (wrong at borders; baseline)",
            ms(n_med)
        );
        println!(
            "    window overhead: A {:.2}x, B {:.2}x the no-margin time",
            a_med / n_med,
            b_total / n_med
        );
        let mut kd_a = sa.by.get_mut(&(TREES, "K_d rounds")).unwrap().clone();
        println!(
            "    trees K_{d} (all rounds), window A: median {:.3} ms, p90 {:.3} ms",
            ms(median(&mut kd_a)),
            ms(pct(&mut kd_a, 0.9))
        );
        if d == 5 {
            println!("    window A breakdown (median ms per chunk):");
            for ((k, stage), v) in sa.by.iter_mut() {
                println!("      {:8} {:28} {:.4}", NAMES[*k], stage, ms(median(v)));
            }
        }
    }

    // Sustained: 16 chunks per submit, so the GPU stays busy and clocked up
    // as it is in a running game. Per-chunk times are summed from that
    // chunk's own timestamp pairs.
    println!("\n  Sustained (16 chunks per submit, per-chunk sums of pass times):");
    for d in [5u32, 3, 7] {
        let specs = stack(d);
        // Runs `lists` (one dispatch list per chunk) in batches of 16 and
        // returns per-chunk totals, plus per-chunk trees K_d.
        let batched = |lists: Vec<Vec<Dispatch>>| -> (Vec<f64>, Vec<f64>) {
            let mut totals = Vec::new();
            let mut kds = Vec::new();
            for group in lists.chunks(16) {
                let ds: Vec<Dispatch> = group.iter().flatten().cloned().collect();
                let (t, _) = g.run(&ds, &[], None);
                let mut at = 0;
                for list in group {
                    let span = &t.pass_ns[at..at + list.len()];
                    totals.push(span.iter().sum());
                    kds.push(
                        list.iter()
                            .zip(span)
                            .filter(|(d, _)| {
                                d.scatter == TREES
                                    && matches!(d.kernel, Kernel::KdRound | Kernel::KdTiled)
                            })
                            .map(|(_, n)| n)
                            .sum(),
                    );
                    at += list.len();
                }
            }
            (totals, kds)
        };
        for tiled in [true, false] {
            g.kd_tiled.set(tiled);
            let label = if tiled { "tiled" } else { "naive" };
            // Warm up, then window A and no margin.
            let a_lists: Vec<Vec<Dispatch>> = interior
                .iter()
                .map(|&(cx, cy)| {
                    g.window_a(
                        &specs,
                        seed,
                        &stack::window_a(&specs, chunk_rect(cx, cy, 64), GRASS, TFS),
                    )
                    .0
                })
                .collect();
            batched(a_lists.clone());
            let (mut a, mut a_kd) = batched(a_lists);
            let n_lists: Vec<Vec<Dispatch>> = interior
                .iter()
                .map(|&(cx, cy)| {
                    window_none_dispatches(
                        g,
                        &specs,
                        seed,
                        &stack::window_none(&specs, chunk_rect(cx, cy, 64), TFS),
                    )
                })
                .collect();
            let (mut none, _) = batched(n_lists);
            // Window B, level by level: each level's chunks only need the
            // levels above, already placed over wider rings.
            g.clear_world();
            let mut b_level = [0.0f64; 4];
            let mut b_kd = Vec::new();
            for (k, ring) in [(TREES, 3), (BOULDERS, 2), (LOGS, 1), (GRASS, 0)] {
                let lists: Vec<Vec<Dispatch>> = (4 - ring..12 + ring)
                    .flat_map(|y| (4 - ring..12 + ring).map(move |x| (x, y)))
                    .map(|(cx, cy)| {
                        let w = stack::window_b(&specs, chunk_rect(cx, cy, 64), k, TFS);
                        let mut ds: Vec<Dispatch> = (0..k.min(3))
                            .map(|j| g.stamp(&specs, seed, TFS, j, w.avail[k]))
                            .collect();
                        ds.extend(g.place(&specs, seed, &w, k, &[w.avail[k]; 3]));
                        ds
                    })
                    .collect();
                let (mut t, mut kd) = batched(lists);
                b_level[k] = median(&mut t);
                if k == TREES {
                    b_kd = std::mem::take(&mut kd);
                }
            }
            let b: f64 = b_level.iter().sum();
            let (am, nm) = (median(&mut a), median(&mut none));
            println!(
                "    d = {d}, {label} K_d: whole stack A median {:.3} ms (p90 {:.3}), B {:.3} ms (trees {:.3}, boulders {:.3}, logs {:.3}, grass {:.3}), \
                 no margin {:.3} ms; overhead A {:.2}x, B {:.2}x; trees K_{d} A {:.3} ms (p90 {:.3}), B {:.3} ms",
                ms(am),
                ms(pct(&mut a, 0.9)),
                ms(b),
                ms(b_level[0]),
                ms(b_level[1]),
                ms(b_level[2]),
                ms(b_level[3]),
                ms(nm),
                am / nm,
                b / nm,
                ms(median(&mut a_kd)),
                ms(pct(&mut a_kd, 0.9)),
                ms(median(&mut b_kd))
            );
        }
    }
    g.kd_tiled.set(true);

    // Trees' K_d alone, both kernels, windows A and B.
    for (tiled, d) in [true, false]
        .iter()
        .flat_map(|&t| [3u32, 5, 7].map(move |d| (t, d)))
    {
        g.kd_tiled.set(tiled);
        let label = if tiled { "tiled" } else { "naive" };
        let specs = stack(d);
        let mut v = Vec::new();
        for &(cx, cy) in &interior {
            let w = stack::window_a(&specs, chunk_rect(cx, cy, 64), GRASS, TFS);
            let ds = g.place(&specs, seed, &w, TREES, &w.mask);
            let (t, _) = g.run(&ds, &[], None);
            v.push(
                ds.iter()
                    .zip(&t.pass_ns)
                    .filter(|(d, _)| matches!(d.kernel, Kernel::KdRound | Kernel::KdTiled))
                    .map(|(_, n)| n)
                    .sum(),
            );
        }
        println!(
            "  trees K_{d}, {label} kernel, window A: median {:.3} ms, p90 {:.3} ms",
            ms(median(&mut v)),
            ms(pct(&mut v, 0.9))
        );
    }
    for (tiled, d) in [true, false]
        .iter()
        .flat_map(|&t| [3u32, 5, 7].map(move |d| (t, d)))
    {
        g.kd_tiled.set(tiled);
        let label = if tiled { "tiled" } else { "naive" };
        let specs = stack(d);
        let mut v = Vec::new();
        for &(cx, cy) in &interior {
            let w = stack::window_b(&specs, chunk_rect(cx, cy, 64), TREES, TFS);
            let ds = g.place(&specs, seed, &w, TREES, &[Rect::EMPTY; 3]);
            let (t, _) = g.run(&ds, &[], None);
            let kd: f64 = ds
                .iter()
                .zip(&t.pass_ns)
                .filter(|(d, _)| matches!(d.kernel, Kernel::KdRound | Kernel::KdTiled))
                .map(|(_, n)| n)
                .sum();
            v.push(kd);
        }
        println!(
            "  trees K_{d}, {label} kernel, window B ({} m square): median {:.3} ms",
            stack::window_b(&specs, chunk_rect(0, 0, 64), TREES, TFS).win[TREES].w() / 2,
            ms(median(&mut v))
        );
    }
    g.kd_tiled.set(true);

    // The CPU twin, one core.
    println!(
        "\n  CPU twin, one core, window A (everything above placed over the stacked margins):"
    );
    for (lowest, n) in [(TREES, 16), (BOULDERS, 16), (LOGS, 16), (GRASS, 6)] {
        for d in [5u32, 3] {
            let specs = stack(d);
            let mut v = Vec::new();
            cpu::take_profile();
            for &(cx, cy) in interior.iter().take(n) {
                let w = stack::window_a(&specs, chunk_rect(cx, cy, 64), lowest, TFS);
                let t = Instant::now();
                let out = cpu::build(&specs, seed, inp, &w);
                v.push(t.elapsed().as_nanos() as f64);
                std::hint::black_box(out);
            }
            let prof = cpu::take_profile();
            let parts: Vec<String> = (0..5)
                .filter(|&s| prof[s] > 0.0)
                .map(|s| format!("{} {:.2}", cpu::STAGE_NAMES[s], ms(prof[s] / n as f64)))
                .collect();
            println!(
                "    down to {:8} d = {d}: median {:.2} ms over {n} chunks (mean ms per stage: {})",
                NAMES[lowest],
                ms(median(&mut v)),
                parts.join(", ")
            );
        }
    }

    // Window B on the CPU: the Scatters above are cached copies; a query
    // stamps their masks and places only its own Scatter over its own margin.
    println!("  CPU twin, one core, window B (copies of the Scatters above already cached):");
    for (k, n) in [(TREES, 16), (BOULDERS, 16), (LOGS, 16), (GRASS, 6)] {
        let specs = stack(5);
        let mut v = Vec::new();
        for &(cx, cy) in interior.iter().take(n) {
            let chunk = chunk_rect(cx, cy, 64);
            let cache = cpu::build(&specs, seed, inp, &stack::window_a(&specs, chunk, k, TFS));
            let w = stack::window_b(&specs, chunk, k, TFS);
            let t = Instant::now();
            let mut masks: [Option<Grid<u8>>; 3] = Default::default();
            for j in 0..k.min(3) {
                let own = &cache.lattice[j].as_ref().unwrap().own;
                masks[j] = Some(cpu::stamp(&specs[j], seed, own, w.avail[k], TFS));
            }
            let avail = cpu::product(&specs[k], k, inp, w.avail[k], TFS, &masks);
            if k == GRASS {
                std::hint::black_box(cpu::grass(&specs[k], seed, inp, &avail, TFS, w.own[k]));
            } else {
                let mut cand = Grid::new(w.win[k], Cand::default());
                for j in w.win[k].y0..w.win[k].y1 {
                    for i in w.win[k].x0..w.win[k].x1 {
                        cand.set(
                            i,
                            j,
                            cpu::candidate(&specs[k], seed, inp, &avail, TFS, i, j),
                        );
                    }
                }
                std::hint::black_box(cpu::kd(&specs[k], &cand, 5));
            }
            v.push(t.elapsed().as_nanos() as f64);
        }
        println!(
            "    {:8} d = 5: median {:.2} ms over {n} chunks",
            NAMES[k],
            ms(median(&mut v))
        );
    }
}

fn slots_cmd(g: &Gpu) {
    let count = 1 << 20;
    println!("\n== Slot choice on the GPU: {count} keys per dispatch, median of 21");
    let period = g.queue.get_timestamp_period() as f64;
    for r in slots::run(&g.device, &g.queue, period, count, 21) {
        println!(
            "  {:26} kernel {:8.1} us, {:6.3} ns per copy over the baseline, CPU mismatches {}",
            r.label, r.kernel_us, r.per_copy_ns, r.mismatches
        );
    }
}

// ---------------------------------------------------------------------------
// Mask resolution: how footprint texel size and grid size change what grass
// (and the lower lattice Scatters) see, against exact disc tests.

fn masks() {
    println!("\n== Mask resolution (CPU), 64 m chunks, d = 5");
    let specs = stack(5);
    let seed = 1;
    let chunks: Vec<(i32, i32)> = (5..9).flat_map(|y| (5..9).map(move |x| (x, y))).collect();
    let inp = Inputs::new(9);

    println!(
        "  Footprint masks, {} chunks: grass blades and lower lattice copies inside a true footprint disc of a kept copy above.\n  \
         The stamp radius is the footprint grown by `grow` texels; leaks are always measured against the true footprint.",
        chunks.len()
    );
    let variants: Vec<(u32, i32)> = [6u32, 7, 8]
        .iter()
        .flat_map(|&t| [0, 2, 4, 6].map(|q| (t, q)))
        .collect();
    for (tfs, grow_q) in variants {
        // Stamp with every footprint grown by grow_q / 4 texels.
        let mut stamp_specs = specs;
        for s in stamp_specs.iter_mut().take(3) {
            if let Kind::Lattice { footprint, .. } = &mut s.kind {
                *footprint += ((1u32 << tfs) * grow_q as u32) / 4;
            }
        }
        let mut leak = [0usize; 3];
        let mut lattice_leak = 0usize;
        let mut lattice_total = 0usize;
        let mut exact_kept = 0usize;
        let mut mask_kept = 0usize;
        let mut over = 0usize;
        let mut mem = 0usize;
        for &(cx, cy) in &chunks {
            let chunk = chunk_rect(cx, cy, 64);
            let w = stack::window_a(&stamp_specs, chunk, GRASS, tfs);
            let out = cpu::build(&stamp_specs, seed, &inp, &w);
            mem += w.mask.iter().take(3).map(|r| r.len()).sum::<usize>();
            // Kept copies of each lattice Scatter, bucketed by 4 m.
            let mut discs: Vec<BTreeMap<(i32, i32), Vec<(i32, i32)>>> = vec![BTreeMap::new(); 3];
            for k in 0..3 {
                let l = out.lattice[k].as_ref().unwrap();
                for j in l.own.rect.y0..l.own.rect.y1 {
                    for i in l.own.rect.x0..l.own.rect.x1 {
                        if l.own.get(i, j) != 0 {
                            let c = l.cand.get(i, j);
                            discs[k]
                                .entry((c.x >> 10, c.y >> 10))
                                .or_default()
                                .push((c.x, c.y));
                        }
                    }
                }
            }
            let inside = |k: usize, x: i32, y: i32| -> bool {
                let f = specs[k].footprint();
                for by in (y >> 10) - 1..=(y >> 10) + 1 {
                    for bx in (x >> 10) - 1..=(x >> 10) + 1 {
                        if let Some(v) = discs[k].get(&(bx, by)) {
                            if v.iter().any(|&(px, py)| {
                                ((px - x) * (px - x) + (py - y) * (py - y)) < f * f
                            }) {
                                return true;
                            }
                        }
                    }
                }
                false
            };
            // Lower lattice copies inside an upper copy's disc.
            for k in 1..3 {
                let l = out.lattice[k].as_ref().unwrap();
                let owned = stack::cells_with_corner_in(&specs[k], chunk);
                for j in owned.y0..owned.y1 {
                    for i in owned.x0..owned.x1 {
                        if l.own.get(i, j) != 0 {
                            let c = l.cand.get(i, j);
                            lattice_total += 1;
                            if (0..k).any(|u| inside(u, c.x, c.y)) {
                                lattice_leak += 1;
                            }
                        }
                    }
                }
            }
            // Grass: masked against exact (no footprint masks, exact discs).
            let masked = out.grass.as_ref().unwrap();
            let av = cpu::product(
                &specs[GRASS],
                0,
                &inp,
                w.avail[GRASS],
                tfs,
                &Default::default(),
            );
            let mut shade_only = Vec::new();
            cpu::grass_each(
                &specs[GRASS],
                seed,
                &inp,
                &av,
                tfs,
                w.own[GRASS],
                |_, _, _, kept| shade_only.push(kept),
            );
            for (m, e) in masked.iter().zip(&shade_only) {
                let in_disc = e.map(|(x, y)| [0, 1, 2].map(|k| inside(k, x, y)));
                let exact = matches!(in_disc, Some(d) if !d.iter().any(|&b| b));
                if exact {
                    exact_kept += 1;
                }
                if m.0 != i32::MIN {
                    mask_kept += 1;
                    let (x, y) = *m;
                    for k in 0..3 {
                        if inside(k, x, y) {
                            leak[k] += 1;
                        }
                    }
                } else if exact {
                    over += 1;
                }
            }
        }
        println!(
            "    {:4} m texels, grow {:.2}: grass inside trunk discs {}, boulder discs {}, log discs {}; grass kept {} vs exact {} ({:+.2}%), \
             exact-kept blades thinned out {} ({:.2}%); boulders/logs inside an upper disc {} of {}; footprint mask bytes per chunk {}",
            (1 << tfs) as f64 / FP as f64,
            grow_q as f64 / 4.0,
            leak[0],
            leak[1],
            leak[2],
            mask_kept,
            exact_kept,
            (mask_kept as f64 / exact_kept as f64 - 1.0) * 100.0,
            over,
            over as f64 / exact_kept as f64 * 100.0,
            lattice_leak,
            lattice_total,
            mem / chunks.len()
        );
    }

    // Influence: coverage times shade from the grid against the exact field.
    println!(
        "  Influence (coverage x canopy shade) grid against the exact field, grass keep decisions, same chunks:"
    );
    for gs in [8u32, 9, 10] {
        let inp = Inputs::new(gs);
        let mut flips = 0usize;
        let mut total = 0usize;
        let mut err = 0i64;
        let mut max_err = 0i64;
        for &(cx, cy) in &chunks {
            let w = stack::window_a(&specs, chunk_rect(cx, cy, 64), GRASS, TFS);
            let av = cpu::product(
                &specs[GRASS],
                0,
                &inp,
                w.avail[GRASS],
                TFS,
                &Default::default(),
            );
            let s = specs[GRASS];
            let Kind::Tile { n_keep } = s.kind else {
                unreachable!()
            };
            for j in w.own[GRASS].y0..w.own[GRASS].y1 {
                for i in w.own[GRASS].x0..w.own[GRASS].x1 {
                    let ch = cell_hash(seed, s.id, i, j);
                    for point in 0..n_keep {
                        let (x, y) = int::tile_pos(ch, inp.tile[point as usize], s.cs, i, j);
                        let grid = int::bilinear(&av, TFS, x, y);
                        let c = stack::coverage_fn(x as f64 / FP as f64, y as f64 / FP as f64);
                        let exact =
                            (c * (1.0 - c * int::SHADE as f64 / 255.0) * 255.0).floor() as u32;
                        let e = grid as i64 - exact as i64;
                        err += e.abs();
                        max_err = max_err.max(e.abs());
                        if (point * 255 < n_keep * grid) != (point * 255 < n_keep * exact) {
                            flips += 1;
                        }
                        total += 1;
                    }
                }
            }
        }
        println!(
            "    {} m grid: mean |error| {:.2}/255, max {}, keep decisions flipped {:.3}% of {} candidates; bytes per 64 m chunk {}",
            (1 << gs) as f64 / FP as f64,
            err as f64 / total as f64,
            max_err,
            flips as f64 / total as f64 * 100.0,
            total,
            ((64 * FP) >> gs).pow(2) + 4 * ((64 * FP) >> gs) + 4
        );
    }
}

// ---------------------------------------------------------------------------
// Grass candidates from a lattice and from plant's tile, for the eye.

fn png_cmd(inp: &Inputs) {
    let patch_m = 6;
    let ppm = 64;
    let side = (patch_m * ppm) as usize;
    let gap = 12usize;
    let densities = [10u32, 50, 200];
    let w = densities.len() * side + (densities.len() - 1) * gap;
    let h = 2 * side + gap;
    let mut px = vec![false; w * h];
    let seed = 1;
    let origin = 300 * FP;
    let dot = |px: &mut Vec<bool>, ox: usize, oy: usize, x: i32, y: i32| {
        let fx = ((x - origin) as i64 * ppm as i64 / FP as i64) as i64;
        let fy = ((y - origin) as i64 * ppm as i64 / FP as i64) as i64;
        for dy in 0..2 {
            for dx in 0..2 {
                let (qx, qy) = (fx + dx, fy + dy);
                if qx >= 0 && qy >= 0 && (qx as usize) < side && (qy as usize) < side {
                    px[(oy + qy as usize) * w + ox + qx as usize] = true;
                }
            }
        }
    };
    let mut counts = Vec::new();
    for (col, &dens) in densities.iter().enumerate() {
        let ox = col * (side + gap);
        // Lattice: one candidate per 1/16 m cell, kept while u24 < p.
        let cs = 4u32;
        let p24 = ((dens as f64 / 256.0) * (1u64 << 24) as f64) as u32;
        let cells = (patch_m * FP) >> cs;
        let c0 = origin >> cs;
        let mut n_l = 0;
        for j in c0..c0 + cells {
            for i in c0..c0 + cells {
                let hsh = cell_hash(seed, 99, i, j);
                if (hash(hsh ^ int::SALT_RANK) >> 8) < p24 {
                    let (x, y) = lattice_pos(hsh, cs, i, j);
                    dot(&mut px, ox, 0, x, y);
                    n_l += 1;
                }
            }
        }
        // Tile: 2 m cells, the first 4 x density points.
        let n_keep = dens * 4;
        let t0 = origin >> 9;
        let mut n_t = 0;
        for j in t0..t0 + patch_m * FP / 512 {
            for i in t0..t0 + patch_m * FP / 512 {
                let ch = cell_hash(seed, 98, i, j);
                for point in 0..n_keep {
                    let (x, y) = int::tile_pos(ch, inp.tile[point as usize], 9, i, j);
                    dot(&mut px, ox, side + gap, x, y);
                    n_t += 1;
                }
            }
        }
        counts.push(format!("{dens}/m2: lattice {n_l}, tile {n_t}"));
    }
    std::fs::create_dir_all("out").unwrap();
    let path = std::path::Path::new("out/grass_lattice_vs_tile.png");
    png::write(path, w, h, &px).unwrap();
    println!(
        "\n== Grass candidates in a {patch_m} m square: columns {:?} per m2; top row lattice (1/16 m cells), bottom row plant's tile (2 m cells). \
         Wrote {} ({})",
        densities,
        path.display(),
        counts.join("; ")
    );
}
