"""Throwaway CPU prototype for D2/D3: specimen slot choice in 32-bit integers.

Not part of the workspace. Run with:
    python3 -I docs/decisions/d2/slots_prototype.py

WGSL has no 64-bit integers and does not promise correctly rounded f32
division, so the published jump consistent hash (64-bit LCG, f64 divide)
cannot run bit-for-bit on the GPU. This checks an integer-only variant:
a 32-bit hash per step, and the jump computed as
    j = ((b + 1) << 16) // ((r >> 16) + 1)
which is exact in u32 while N <= 2^15. It is compared with rendezvous
hashing (highest 32-bit score wins) on balance and on how many copies move
when N grows by one.
"""
import numpy as np

M32 = 0xFFFFFFFF


def pcg(x):
    """Plant's 32-bit hash (`common.wesl::hash`)."""
    state = (x * 747796405 + 2891336453) & M32
    word = (((state >> ((state >> 28) + 4)) ^ state) * 277803737) & M32
    return ((word >> 22) ^ word) & M32


def jump32(key, n):
    """Jump consistent hash with 32-bit steps and an exact integer jump."""
    b, j, h = -1, 0, key & M32
    while j < n:
        b = j
        h = pcg(h ^ 0x9E3779B9)
        j = ((b + 1) << 16) // ((h >> 16) + 1)
    return b


def rendezvous(key, n):
    best, slot = -1, 0
    for k in range(n):
        s = pcg((key ^ pcg(k)) & M32)
        if s > best:
            best, slot = s, k
    return slot


def main():
    keys = [pcg(i * 2654435761 & M32) for i in range(200_000)]
    for name, f in (("jump32", jump32), ("rendezvous", rendezvous)):
        for n in (4, 8, 16, 32, 64):
            a = np.array([f(k, n) for k in keys])
            b = np.array([f(k, n + 1) for k in keys])
            counts = np.bincount(a, minlength=n)
            moved = (a != b).mean()
            bad = ((a != b) & (b != n)).sum()
            print(f"{name:10s} N={n:3d}: slot share min {counts.min() / len(keys) * n:.3f}"
                  f" max {counts.max() / len(keys) * n:.3f} of even; N+1 moves "
                  f"{moved:.4f} (ideal {1 / (n + 1):.4f}); moves not to the new slot: {bad}")


if __name__ == "__main__":
    main()
