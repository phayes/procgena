"""Throwaway CPU prototype for D2: chunk-local spacing for tree groves.

Not part of the workspace. Run with:
    python3 -I docs/decisions/d2/spacing_prototype.py
Needs numpy and scipy. Takes about a minute.

Candidates sit on a World-anchored 0.5 m lattice, as in today's
`GrovePlacement::new`: one hashed site per cell, with a rank
`u / local_density`, kept as a candidate while the rank is below the
candidates-per-cell threshold. Two spacing rules are compared:

  G    today's rule: visit candidates in rank order and keep one when no
       kept, lower-ranked candidate lies within the larger of the two
       spacings. Pure in the ranks, but a site's fate can hang on a chain of
       lower-ranked sites of any length, so a chunk built with a finite
       margin can disagree with the whole World.
  K_d  the bounded rule proposed in D2.md. K_0 keeps every candidate;
       K_{n+1} keeps a candidate when no lower-ranked conflicting candidate
       is in K_n. For odd d, K_d respects spacing, is a subset of G, and
       depends only on candidates within d * max_spacing.
"""
import numpy as np
from scipy.spatial import cKDTree

CELL = 0.5
U64 = np.uint64


def mix(x):
    """SplitMix64 finaliser on a uint64 array (wrapping multiply)."""
    with np.errstate(over="ignore"):
        x = x ^ (x >> U64(30))
        x = x * U64(0xBF58476D1CE4E5B9)
        x = x ^ (x >> U64(27))
        x = x * U64(0x94D049BB133111EB)
        return x ^ (x >> U64(31))


def draw(h, salt):
    return (mix(h ^ mix(np.full_like(h, salt))) >> U64(40)).astype(
        np.float64) / float(1 << 24)


def candidates(seed, box, per_m2, spacings, shares, local=None):
    """Candidates in box = (x0, z0, x1, z1): (ids, points, ranks, spacings).

    `local(x, z)` is the mask, from 0 to 1, applied as today's grove does:
    rank = u / local."""
    x0, z0, x1, z1 = box
    i = np.arange(int(np.floor(x0 / CELL)), int(np.ceil(x1 / CELL)))
    j = np.arange(int(np.floor(z0 / CELL)), int(np.ceil(z1 / CELL)))
    I, J = np.meshgrid(i, j, indexing="ij")
    I, J = I.ravel(), J.ravel()
    ids = ((I.astype(np.int64).astype(U64) & U64(0xFFFFFFFF)) << U64(32)) | (
        J.astype(np.int64).astype(U64) & U64(0xFFFFFFFF))
    h = mix(U64(seed) ^ mix(ids))
    x = (I + draw(h, 1)) * CELL
    z = (J + draw(h, 2)) * CELL
    loc = np.ones_like(x) if local is None else np.clip(local(x, z), 0, 1)
    with np.errstate(divide="ignore"):
        rank = np.where(loc > 0, draw(h, 3) / loc, np.inf)
    keep = (rank < per_m2 * CELL * CELL) & (x >= x0) & (x < x1) & (
        z >= z0) & (z < z1)
    species = np.searchsorted(np.cumsum(shares), draw(h, 4), side="right")
    sp = np.asarray(spacings)[np.minimum(species, len(spacings) - 1)]
    return ids[keep], np.stack([x[keep], z[keep]], 1), rank[keep], sp[keep]


def lower_edges(pts, ranks, ids, sps):
    """Edges (site, lower-ranked conflicting site). Ties break on the id."""
    pairs = cKDTree(pts).query_pairs(sps.max(), output_type="ndarray")
    if len(pairs) == 0:
        return np.zeros(0, int), np.zeros(0, int)
    a, b = pairs[:, 0], pairs[:, 1]
    d2 = ((pts[a] - pts[b]) ** 2).sum(1)
    hit = d2 < np.maximum(sps[a], sps[b]) ** 2
    a, b = a[hit], b[hit]
    a_lower = (ranks[a] < ranks[b]) | ((ranks[a] == ranks[b]) & (ids[a] < ids[b]))
    return np.where(a_lower, b, a), np.where(a_lower, a, b)


def greedy(n, src, dst, ranks, ids):
    order = np.lexsort((ids, ranks))
    nbrs = [[] for _ in range(n)]
    for s, t in zip(src, dst):
        nbrs[s].append(t)
    kept = np.zeros(n, bool)
    for s in order:
        kept[s] = not any(kept[t] for t in nbrs[s])
    return kept


def bounded(n, src, dst, rounds):
    kept = np.ones(n, bool)
    for _ in range(rounds):
        blocked = np.bincount(src, weights=kept[dst].astype(float), minlength=n)
        kept = blocked == 0
    return kept


def place(seed, box, per_m2, spacings, shares, rule, local=None, own=None):
    """Kept ids inside `own` (default: the whole box)."""
    ids, pts, ranks, sps = candidates(seed, box, per_m2, spacings, shares, local)
    src, dst = lower_edges(pts, ranks, ids, sps)
    kept = (greedy(len(ids), src, dst, ranks, ids) if rule == 0
            else bounded(len(ids), src, dst, rule))
    x0, z0, x1, z1 = own or box
    inside = (pts[:, 0] >= x0) & (pts[:, 0] < x1) & (pts[:, 1] >= z0) & (
        pts[:, 1] < z1)
    sel = kept & inside
    return dict(zip(ids[sel].tolist(), map(tuple, pts[sel])))


def chunked(seed, extent, chunk, margin, per_m2, spacings, shares, rule,
            local=None):
    out = {}
    for cx in range(0, extent, chunk):
        for cz in range(0, extent, chunk):
            box = (cx - margin, cz - margin, cx + chunk + margin,
                   cz + chunk + margin)
            out.update(place(seed, box, per_m2, spacings, shares, rule, local,
                             own=(cx, cz, cx + chunk, cz + chunk)))
    return out


def violations(kept, spacing_of):
    if not kept:
        return 0
    ids = list(kept)
    pts = np.array([kept[i] for i in ids])
    sps = np.array([spacing_of[i] for i in ids])
    pairs = cKDTree(pts).query_pairs(sps.max(), output_type="ndarray")
    if len(pairs) == 0:
        return 0
    a, b = pairs[:, 0], pairs[:, 1]
    d2 = ((pts[a] - pts[b]) ** 2).sum(1)
    return int((d2 < np.maximum(sps[a], sps[b]) ** 2).sum())


def mask(x, z):
    """A smooth, uneven mask: rolling stands and gaps tens of metres across."""
    return 0.55 + 0.45 * np.sin(x / 13.0) * np.cos(z / 17.0 + 0.7 * np.sin(x / 29.0))


CASES = [
    ("pine_large, scale 1", 550 / 1e4, [1.3], [1.0]),
    ("oak_large, scale 3", 3 * 130 / 1e4, [2.7], [1.0]),
    ("ash_large + aspen_small, scale 2", 2 * 2760 / 1e4, [2.5, 0.6],
     [160 / 2760, 1.0]),
    ("ash_large + aspen_small, scale 4 (dense mix)", 4 * 2760 / 1e4,
     [2.5, 0.6], [160 / 2760, 1.0]),
    ("pine_small, scale 4 (saturated)", 4 * 2600 / 1e4, [0.6], [1.0]),
]


def main():
    seed = 0x5EED
    extent = 128
    pad = 48.0
    whole_box = (-pad, -pad, extent + pad, extent + pad)
    own = (0, 0, extent, extent)
    for name, per_m2, spacings, shares in CASES:
        R = max(spacings)
        ids, pts, ranks, sps = candidates(seed, whole_box, per_m2, spacings,
                                          shares, mask)
        spacing_of = dict(zip(ids.tolist(), sps))
        G = place(seed, whole_box, per_m2, spacings, shares, 0, mask, own)
        print(f"\n== {name}: asked {per_m2 * 1e4:.0f}/ha under an uneven "
              f"mask, max spacing {R} m, {extent} m square")
        print(f"  G   kept {len(G):6d}, spacing violations "
              f"{violations(G, spacing_of)}")
        for d in (1, 3, 5, 7, 9):
            K = place(seed, whole_box, per_m2, spacings, shares, d, mask, own)
            print(f"  K_{d} kept {len(K):6d} = {len(K) / len(G):.4f} of G, "
                  f"differs from G at {len(set(K) ^ set(G)):4d} sites, "
                  f"subset {set(K) <= set(G)}, violations "
                  f"{violations(K, spacing_of)}")
        for m in (1, 2):
            Gc = chunked(seed, extent, 32, m * R, per_m2, spacings, shares, 0,
                         mask)
            print(f"  G built per 32 m chunk, margin {m}R: "
                  f"{len(set(Gc) ^ set(G))} sites differ from whole")
        for d in (3, 5):
            Kw = place(seed, whole_box, per_m2, spacings, shares, d, mask, own)
            for chunk in (16, 64):
                Kc = chunked(seed, extent, chunk, d * R, per_m2, spacings,
                             shares, d, mask)
                print(f"  K_{d} built per {chunk} m chunk, margin {d}R: "
                      f"{len(set(Kc) ^ set(Kw))} sites differ from whole")

    # Local edits: clear the mask in a disc, or raise it to full, and see how
    # far outside the disc the survivors change. Raising it reranks the
    # candidates inside, which is what reaches outside.
    name, per_m2, spacings, shares = CASES[3]
    R = max(spacings)
    c, r = extent / 2, 6.0
    in_disc = lambda x, z: (x - c) ** 2 + (z - c) ** 2 < r * r
    edits = {
        "cleared": lambda x, z: np.where(in_disc(x, z), 0.0, mask(x, z)),
        "raised to 1": lambda x, z: np.where(in_disc(x, z), 1.0, mask(x, z)),
    }
    for what, edited in edits.items():
        print(f"\n== Local edit on '{name}': mask {what} in a {r} m disc")
        for rule, label in ((0, "G  "), (3, "K_3"), (5, "K_5")):
            a = place(seed, whole_box, per_m2, spacings, shares, rule, mask,
                      own)
            b = place(seed, whole_box, per_m2, spacings, shares, rule, edited,
                      own)
            changed = [p for i, p in {**a, **b}.items()
                       if (i in a) != (i in b)]
            outside = [np.hypot(x - c, z - c) - r for x, z in changed
                       if not in_disc(x, z)]
            far = max(outside, default=0.0)
            bound = "unbounded" if rule == 0 else f"bound {rule * R:.1f} m"
            print(f"  {label}: {len(changed)} sites change, {len(outside)} "
                  f"outside the disc, the farthest {far:.2f} m out ({bound})")

    # The tail: on a larger area of the densest mix, how many sites still
    # differ from G after d rounds.
    big = 384
    box = (-pad, -pad, big + pad, big + pad)
    G = place(seed, box, per_m2, spacings, shares, 0, mask, (0, 0, big, big))
    print(f"\n== Tail on '{name}', {big} m square, {len(G)} trees kept by G")
    for d in (1, 3, 5, 7, 9, 11):
        K = place(seed, box, per_m2, spacings, shares, d, mask,
                  (0, 0, big, big))
        print(f"  K_{d}: {len(set(K) ^ set(G))} sites differ from G")

    # Nesting: lowering the global scale only removes trees.
    for d in (3, 5):
        hi = place(seed, (0, 0, 64, 64), 1100 / 1e4, [1.55], [1.0], d, mask)
        lo = place(seed, (0, 0, 64, 64), 400 / 1e4, [1.55], [1.0], d, mask)
        print(f"\nK_{d}: 400/ha survivors are a subset of 1100/ha survivors: "
              f"{set(lo) <= set(hi)}")


if __name__ == "__main__":
    main()
