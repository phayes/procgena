"""Paired A/B runs of the `bench` example, the way the performance pass was measured.

    python tools/plant/bench_ab.py snapshot NAME
        Copy the freshly built bench.exe to target/bench-ab/NAME.exe. Build each
        variant (`cargo build --example bench`), snapshot it, then change the
        code back.
    python tools/plant/bench_ab.py run ROUNDS NAME [NAME ...]
        Run every named variant in both scenes, interleaved, ROUNDS times.
        Add the variant `noplants` to measure the grass-free baseline (it runs
        the first named variant's exe with --no-plants). `NAME+flag` runs NAME
        with `--flag` added, e.g. `m4+sheen` for the bench's `--sheen`.
    python tools/plant/bench_ab.py report REFERENCE [p50|p95]
        For each variant, the median paired difference against the REFERENCE
        run of the same round and scene, in ms and as a share of the
        reference's grass cost (reference minus no-plants, both medians), for
        frame time p50 (the default) or p95.

Results append to target/bench-ab/results.jsonl, so rounds can be added later.
A run whose log contains ERROR or a panic is rejected: a shader that fails to
compile draws no grass and would otherwise look like a large speed-up.

Measure on a quiet machine, plugged in, one GPU-heavy program at a time. Noise
on the development laptop was about +/-2% of grass cost; 4 rounds resolve 5%.
"""

import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import time

# The Cargo workspace (procgena), which owns Cargo.lock and target/, and the
# procgena_plant_bevy package in it, whose examples this runs. This script
# lives in <workspace>/tools/plant/.
WORKSPACE = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ROOT = os.path.join(WORKSPACE, "plant", "crates", "procgena_plant_bevy")
CARGO_TARGET = os.path.join(WORKSPACE, "target")
EXE = ".exe" if os.name == "nt" else ""
BENCH = os.path.join(CARGO_TARGET, "debug", "examples", "bench" + EXE)
OUT = os.path.join(CARGO_TARGET, "bench-ab")
RESULTS = os.path.join(OUT, "results.jsonl")
SCENES = {"perspective": [], "demo_arpg_game": ["--ortho"]}
LINE = re.compile(
    r"^\s+(frame|grass trample|grass cull|grass generate|main pass)\s+p50\s+([\d.]+) ms\s+p95\s+([\d.]+) ms",
    re.M,
)


def snapshot(name):
    os.makedirs(OUT, exist_ok=True)
    shutil.copy(BENCH, os.path.join(OUT, name + EXE))
    print("saved", name)


def run_one(name, scene, plants_exe):
    # `NAME+flag+flag` runs NAME's exe with `--flag --flag`.
    base, *flags = (plants_exe if name == "noplants" else name).split("+")
    exe = os.path.join(OUT, base + EXE)
    args = [exe, *SCENES[scene], *("--" + f for f in flags), "--csv", os.path.join(OUT, "last.csv")]
    if name == "noplants":
        args.append("--no-plants")
    out = subprocess.run(args, cwd=ROOT, capture_output=True, text=True, timeout=900)
    text = out.stdout + out.stderr
    if "ERROR" in text or "panicked" in text:
        print(text[-3000:])
        raise SystemExit(f"{name} {scene}: errors in the log, run rejected")
    found = {m.group(1): float(m.group(2)) for m in LINE.finditer(text)}
    found.update({m.group(1) + " p95": float(m.group(3)) for m in LINE.finditer(text)})
    if "frame" not in found:
        print(text[-3000:])
        raise SystemExit(f"{name} {scene}: no summary in the log")
    row = {"variant": name, "scene": scene, "time": time.time(), **found}
    with open(RESULTS, "a", encoding="utf-8") as f:
        f.write(json.dumps(row) + "\n")
    print(f"{name:>20} {scene:11} frame p50 {found['frame']:6.3f} ms"
          f"  main pass {found.get('main pass', float('nan')):6.3f} ms", flush=True)


def run(rounds, names):
    plants_exe = next(n for n in names if n != "noplants")
    for _ in range(rounds):
        for scene in SCENES:
            for name in names:
                run_one(name, scene, plants_exe)


def report(reference, percentile="p50"):
    key = "frame" if percentile == "p50" else "frame p95"
    rows = [json.loads(line) for line in open(RESULTS, encoding="utf-8")]
    rows = [r for r in rows if key in r]
    medians = {}
    for scene in SCENES:
        for name in ("noplants", reference):
            values = [r[key] for r in rows if r["variant"] == name and r["scene"] == scene]
            medians[(name, scene)] = statistics.median(values) if values else None
    last = {}
    deltas = {}
    for r in rows:
        if r["variant"] == reference:
            last[r["scene"]] = r[key]
        elif r["variant"] != "noplants" and r["scene"] in last:
            deltas.setdefault((r["variant"], r["scene"]), []).append(r[key] - last[r["scene"]])
    print(f"frame {percentile} against {reference}; negative is faster")
    for (name, scene), d in sorted(deltas.items()):
        m = statistics.median(d)
        ref, base = medians[(reference, scene)], medians[("noplants", scene)]
        share = f"{100 * m / (ref - base):+6.1f}% of grass cost" if base is not None else ""
        print(f"{name:>20} | {scene:11} | n={len(d)} | {m:+6.3f} ms | {share}")


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "snapshot":
        snapshot(sys.argv[2])
    elif command == "run":
        run(int(sys.argv[2]), sys.argv[3:])
    elif command == "report":
        report(sys.argv[2], *sys.argv[3:4])
    else:
        print(__doc__)
