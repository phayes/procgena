// GPU twin of `int.rs` and `cpu.rs`. Every function matches the Rust one of
// the same name, in u32/i32 wrapping arithmetic only.

struct P {
    a: vec4<u32>,          // seed, slot (0 trees .. 3 grass), cell shift cs, stencil radius r
    w: vec4<i32>,          // evaluated window W (cells): x0, y0, width, height
    o: vec4<i32>,          // owned cells O: x0, y0, width, height
    av: vec4<i32>,         // available-mask texels: x0, y0, width, height
    fm: array<vec4<i32>, 3>, // footprint-mask texel rects of trees, boulders, logs
    b: vec4<u32>,          // tfs, gs, p24, share24
    c: vec4<u32>,          // spacing 0, spacing 1, max slope, n_keep
    e: vec4<u32>,          // round (1-based) or 0, footprint F, masks read, reads shade
    g: vec4<i32>,          // global coverage/slope grid: x0, y0, width, height
    wk: array<vec4<i32>, 3>, // World kept-grid rects of trees, boulders, logs
    h: vec4<u32>,          // half of the kept buffer, Scatter id (hashed), 0, 0
}

@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read> cov: array<u32>;
@group(0) @binding(2) var<storage, read> slope: array<u32>;
@group(0) @binding(3) var<storage, read> tile: array<u32>;
@group(0) @binding(4) var<storage, read_write> avail: array<u32>;
@group(0) @binding(5) var<storage, read_write> fm0: array<u32>;
@group(0) @binding(6) var<storage, read_write> fm1: array<u32>;
@group(0) @binding(7) var<storage, read_write> fm2: array<u32>;
@group(0) @binding(8) var<storage, read_write> cand: array<vec4<u32>>;
@group(0) @binding(9) var<storage, read_write> kept: array<u32>;
@group(0) @binding(10) var<storage, read_write> wk0: array<u32>;
@group(0) @binding(11) var<storage, read_write> wk1: array<u32>;
@group(0) @binding(12) var<storage, read_write> wk2: array<u32>;
@group(0) @binding(13) var<storage, read_write> gout: array<u32>;

const SALT_JX: u32 = 0x68E31DA4u;
const SALT_JY: u32 = 0xB5297A4Du;
const SALT_RANK: u32 = 0x1B56C4E9u;
const SALT_SPECIES: u32 = 0x9E3779B9u;
const SHADE: u32 = 150u;
const NONE: u32 = 0x80000000u;

fn hash(x: u32) -> u32 {
    let state = x * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn hash2(a: u32, b: u32) -> u32 {
    return hash(a ^ hash(b));
}

fn cell_hash(seed: u32, sid: u32, i: i32, j: i32) -> u32 {
    return hash2(hash2(seed, sid), hash2(bitcast<u32>(i), bitcast<u32>(j)));
}

fn lattice_pos(h: u32, cs: u32, i: i32, j: i32) -> vec2<i32> {
    let jx = i32(hash(h ^ SALT_JX) >> (32u - cs));
    let jy = i32(hash(h ^ SALT_JY) >> (32u - cs));
    return vec2<i32>((i << cs) + jx, (j << cs) + jy);
}

fn tile_pos(ch: u32, q: u32, cs: u32, i: i32, j: i32) -> vec2<i32> {
    let ox = ch >> 16u;
    let oy = hash(ch) >> 16u;
    let px = (((q & 0xFFFFu) + ox) & 0xFFFFu) >> (16u - cs);
    let py = (((q >> 16u) + oy) & 0xFFFFu) >> (16u - cs);
    return vec2<i32>((i << cs) + i32(px), (j << cs) + i32(py));
}

// Texel of the global coverage/slope grids.
fn gidx(x: i32, y: i32) -> u32 {
    return u32((y - p.g.y) * p.g.z + (x - p.g.x));
}

fn infl(c: u32) -> u32 {
    return 255u - c * SHADE / 255u;
}

// Which grid a bilinear read samples.
const G_COV: u32 = 0u;
const G_SHADE: u32 = 1u;
const G_SLOPE: u32 = 2u;
const G_AVAIL: u32 = 3u;

fn texel(kind: u32, x: i32, y: i32) -> u32 {
    switch kind {
        case 0u: { return cov[gidx(x, y)]; }
        case 1u: { return infl(cov[gidx(x, y)]); }
        case 2u: { return slope[gidx(x, y)]; }
        default: { return avail[u32((y - p.av.y) * p.av.z + (x - p.av.x))]; }
    }
}

fn bilinear(kind: u32, shift: u32, px: i32, py: i32) -> u32 {
    let half = 1 << (shift - 1u);
    let ux = px - half;
    let uy = py - half;
    let tx = ux >> shift;
    let ty = uy >> shift;
    let wx = (u32(ux - (tx << shift)) << 8u) >> shift;
    let wy = (u32(uy - (ty << shift)) << 8u) >> shift;
    let top = texel(kind, tx, ty) * (256u - wx) + texel(kind, tx + 1, ty) * wx;
    let bot = texel(kind, tx, ty + 1) * (256u - wx) + texel(kind, tx + 1, ty + 1) * wx;
    return (top * (256u - wy) + bot * wy) >> 16u;
}

fn fm_read(j: u32, tx: i32, ty: i32) -> u32 {
    let r = p.fm[j];
    let i = u32((ty - r.y) * r.z + (tx - r.x));
    switch j {
        case 0u: { return fm0[i]; }
        case 1u: { return fm1[i]; }
        default: { return fm2[i]; }
    }
}

fn wk_index(j: u32, i: i32, k: i32) -> u32 {
    let r = p.wk[j];
    return u32((k - r.y) * r.z + (i - r.x));
}

fn wk_read(j: u32, i: i32, k: i32) -> u32 {
    let n = wk_index(j, i, k);
    switch j {
        case 0u: { return wk0[n]; }
        case 1u: { return wk1[n]; }
        default: { return wk2[n]; }
    }
}

// Available mask of the current Scatter over its texel rect.
@compute @workgroup_size(8, 8)
fn product(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= u32(p.av.z) || id.y >= u32(p.av.w) { return; }
    let tx = p.av.x + i32(id.x);
    let ty = p.av.y + i32(id.y);
    let tfs = p.b.x;
    let gs = p.b.y;
    let size = 1 << tfs;
    let cx = tx * size + size / 2;
    let cy = ty * size + size / 2;
    var v = bilinear(G_COV, gs, cx, cy);
    if p.e.w != 0u {
        v = v * bilinear(G_SHADE, gs, cx, cy) / 255u;
    }
    for (var j = 0u; j < p.e.z; j++) {
        v = v * fm_read(j, tx, ty) / 255u;
    }
    avail[id.y * u32(p.av.z) + id.x] = v;
}

// Lattice candidates over W, the density threshold and the footing check.
// Also sets round 0 of K_d (every alive candidate) in the kept buffer.
@compute @workgroup_size(8, 8)
fn candidates(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= u32(p.w.z) || id.y >= u32(p.w.w) { return; }
    let i = p.w.x + i32(id.x);
    let j = p.w.y + i32(id.y);
    let cs = p.a.z;
    let h = cell_hash(p.a.x, p.h.y, i, j);
    let pos = lattice_pos(h, cs, i, j);
    let u24 = hash(h ^ SALT_RANK) >> 8u;
    let sp = select(p.c.y, p.c.x, (hash(h ^ SALT_SPECIES) >> 8u) < p.b.w);
    let local = bilinear(G_AVAIL, p.b.x, pos.x, pos.y);
    let sl = bilinear(G_SLOPE, p.b.y, pos.x, pos.y);
    let alive = local > 0u && u24 * 255u < p.b.z * local && sl <= p.c.z;
    let n = id.y * u32(p.w.z) + id.x;
    cand[n] = vec4<u32>(bitcast<u32>(pos.x), bitcast<u32>(pos.y), (u24 << 8u) | local, sp | (u32(alive) << 31u));
    kept[n] = u32(alive);
}

fn lower(b: vec4<u32>, bi: i32, bj: i32, a: vec4<u32>, ai: i32, aj: i32) -> bool {
    let l = (b.z >> 8u) * (a.z & 0xFFu);
    let r = (a.z >> 8u) * (b.z & 0xFFu);
    return l < r || (l == r && (bi < ai || (bi == ai && bj < aj)));
}

fn conflict(a: vec4<u32>, b: vec4<u32>) -> bool {
    let dx = bitcast<i32>(a.x) - bitcast<i32>(b.x);
    let dy = bitcast<i32>(a.y) - bitcast<i32>(b.y);
    let s = max(a.w & 0x7FFFFFFFu, b.w & 0x7FFFFFFFu);
    return u32(dx * dx + dy * dy) < s * s;
}

// One round of K_d: reads round e.x - 1, writes round e.x.
@compute @workgroup_size(8, 8)
fn kd_round(@builtin(global_invocation_id) id: vec3<u32>) {
    let ww = p.w.z;
    let wh = p.w.w;
    if id.x >= u32(ww) || id.y >= u32(wh) { return; }
    let x = i32(id.x);
    let y = i32(id.y);
    let n = id.y * u32(ww) + id.x;
    let src = ((p.e.x - 1u) & 1u) * p.h.x;
    let dst = (p.e.x & 1u) * p.h.x;
    let a = cand[n];
    var k = 0u;
    if (a.w >> 31u) != 0u {
        k = 1u;
        let r = i32(p.a.w);
        let ai = p.w.x + x;
        let aj = p.w.y + y;
        for (var by = max(y - r, 0); by < min(y + r + 1, wh) && k != 0u; by++) {
            for (var bx = max(x - r, 0); bx < min(x + r + 1, ww); bx++) {
                if bx == x && by == y { continue; }
                let m = u32(by * ww + bx);
                if kept[src + m] == 0u { continue; }
                let b = cand[m];
                if lower(b, p.w.x + bx, p.w.y + by, a, ai, aj) && conflict(a, b) {
                    k = 0u;
                    break;
                }
            }
        }
    }
    kept[dst + n] = k;
}

// The same round as `kd_round`, with the neighbourhood of each 16 x 16 block
// loaded once into workgroup memory. Needs a stencil radius a.w <= TILE_HALO.
const TILE: i32 = 16;
const TILE_HALO: i32 = 7;
const TILE_W: i32 = 30; // TILE + 2 * TILE_HALO
var<workgroup> tile_cand: array<vec4<u32>, 900>;
var<workgroup> tile_kept: array<u32, 900>;

@compute @workgroup_size(16, 16)
fn kd_tiled(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(local_invocation_index) li: u32,
    @builtin(workgroup_id) wg: vec3<u32>,
) {
    let ww = p.w.z;
    let wh = p.w.w;
    let src = ((p.e.x - 1u) & 1u) * p.h.x;
    let dst = (p.e.x & 1u) * p.h.x;
    // Load the block plus its halo: only candidates kept in the last round
    // can block anyone, so the others are left empty.
    let bx0 = i32(wg.x) * TILE - TILE_HALO;
    let by0 = i32(wg.y) * TILE - TILE_HALO;
    for (var t = li; t < 900u; t += 256u) {
        let tx = bx0 + i32(t % 30u);
        let ty = by0 + i32(t / 30u);
        var c = vec4<u32>(0u);
        var k = 0u;
        if tx >= 0 && ty >= 0 && tx < ww && ty < wh {
            let m = u32(ty * ww + tx);
            k = kept[src + m];
            if k != 0u { c = cand[m]; }
        }
        tile_cand[t] = c;
        tile_kept[t] = k;
    }
    workgroupBarrier();
    if id.x >= u32(ww) || id.y >= u32(wh) { return; }
    let x = i32(id.x);
    let y = i32(id.y);
    let n = id.y * u32(ww) + id.x;
    let a = cand[n];
    var k = 0u;
    if (a.w >> 31u) != 0u {
        k = 1u;
        let r = i32(p.a.w);
        let ai = p.w.x + x;
        let aj = p.w.y + y;
        let lx = x - bx0;
        let ly = y - by0;
        for (var dy = -r; dy <= r && k != 0u; dy++) {
            for (var dx = -r; dx <= r; dx++) {
                let t = (ly + dy) * TILE_W + lx + dx;
                if (dx == 0 && dy == 0) || tile_kept[t] == 0u { continue; }
                let b = tile_cand[t];
                if lower(b, ai + dx, aj + dy, a, ai, aj) && conflict(a, b) {
                    k = 0u;
                    break;
                }
            }
        }
    }
    kept[dst + n] = k;
}

// Copies the final round over the owned cells into the World kept grid.
@compute @workgroup_size(8, 8)
fn commit(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= u32(p.o.z) || id.y >= u32(p.o.w) { return; }
    let i = p.o.x + i32(id.x);
    let j = p.o.y + i32(id.y);
    let src = (p.e.x & 1u) * p.h.x + u32((j - p.w.y) * p.w.z + (i - p.w.x));
    let v = kept[src];
    let n = wk_index(p.a.y, i, j);
    switch p.a.y {
        case 0u: { wk0[n] = v; }
        case 1u: { wk1[n] = v; }
        default: { wk2[n] = v; }
    }
}

// Footprint mask of Scatter slot a.y over its rect fm[a.y]: gathers the kept
// copies whose disc covers each texel centre.
@compute @workgroup_size(8, 8)
fn stamp(@builtin(global_invocation_id) id: vec3<u32>) {
    let slot = p.a.y;
    let r = p.fm[slot];
    if id.x >= u32(r.z) || id.y >= u32(r.w) { return; }
    let tx = r.x + i32(id.x);
    let ty = r.y + i32(id.y);
    let tfs = p.b.x;
    let size = 1 << tfs;
    let cx = tx * size + size / 2;
    let cy = ty * size + size / 2;
    let cs = p.a.z;
    let f = i32(p.e.y);
    let f2 = u32(f * f);
    var v = 255u;
    let i0 = (cx - f) >> cs;
    let i1 = ((cx + f) >> cs) + 1;
    let j0 = (cy - f) >> cs;
    let j1 = ((cy + f) >> cs) + 1;
    for (var j = j0; j < j1 && v != 0u; j++) {
        for (var i = i0; i < i1; i++) {
            if wk_read(slot, i, j) == 0u { continue; }
            let q = lattice_pos(cell_hash(p.a.x, p.h.y, i, j), cs, i, j);
            let dx = q.x - cx;
            let dy = q.y - cy;
            if u32(dx * dx + dy * dy) < f2 {
                v = 0u;
                break;
            }
        }
    }
    let n = id.y * u32(r.z) + id.x;
    switch slot {
        case 0u: { fm0[n] = v; }
        case 1u: { fm1[n] = v; }
        default: { fm2[n] = v; }
    }
}

// Grass over its owned tile cells: x is the point (rank), y the cell.
@compute @workgroup_size(64, 1)
fn grass(@builtin(global_invocation_id) id: vec3<u32>) {
    let n_keep = p.c.w;
    let cells = u32(p.o.z * p.o.w);
    if id.x >= n_keep || id.y >= cells { return; }
    let i = p.o.x + i32(id.y % u32(p.o.z));
    let j = p.o.y + i32(id.y / u32(p.o.z));
    let cs = p.a.z;
    let ch = cell_hash(p.a.x, p.h.y, i, j);
    let pos = tile_pos(ch, tile[id.x], cs, i, j);
    let local = bilinear(G_AVAIL, p.b.x, pos.x, pos.y);
    let sl = bilinear(G_SLOPE, p.b.y, pos.x, pos.y);
    let keep = id.x * 255u < n_keep * local && sl <= p.c.z;
    let n = 2u * (id.y * n_keep + id.x);
    gout[n] = select(NONE, bitcast<u32>(pos.x), keep);
    gout[n + 1u] = select(NONE, bitcast<u32>(pos.y), keep);
}
