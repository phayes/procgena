# procgena: rules for agents working in parallel

Several agents often work in this one checkout at the same time. They share
the working tree, the `target/` directory and the CPU. These rules keep them
from breaking each other's builds, overwriting each other's files, and paying
for the same workspace-wide build more than once.

## Read the architecture first

Before you design or change anything that crosses crates (a generator's
public API, placement, masks, seeds, ids, LOD, loading, presets, or a new
crate), read [ARCHITECTURE.md](ARCHITECTURE.md). It sets the hierarchy
(Asset, Scatter, Ecotope, Biome and Terrain, World), the rules every level
follows, and the alignment rules every asset is moving towards. The current code is far
behind it, and that is expected for now: the gap is not a bug to fix as you
go. Don't take today's code as a sign the document is wrong. Follow the
document in new work, and only change existing code to match it when that
is your task.
If your change conflicts with it, stop and ask rather than diverging
quietly. If your change settles one of its TODOs or open questions, update
the document and say so in your report.

## Your role: worker or coordinator

- **Worker**: an agent that was given one task. A worker runs only the checks
  scoped to what it changed (see "Scoped checks"), and never runs the full
  suite.
- **Coordinator**: the agent that launched the workers. When the sessions were
  started independently, the coordinator is the user. The coordinator runs the
  full suite once, after **every** worker has reported done (see "The full
  suite").

If nobody told you that you are the coordinator, you are a worker.

## Scoped checks (workers)

The root `Cargo.toml` is a virtual workspace manifest. Running a bare
`cargo test`, `cargo build`, `cargo clippy`, `cargo doc` or `cargo fmt` at the
repository root covers **every** crate. So does `--workspace`. Do not run any
of these.

Always name the package, and narrow it further where you can:

```bash
cargo test -p <package> --test <test-file> <name-filter>
cargo test -p <package> --lib <module::path>
cargo clippy -p <package> --all-targets -- -D warnings
cargo doc -p <package> --no-deps
cargo test -p <package> --doc
```

- Add `--features ...` only for features your change touches.
- Format only the files you edited: `rustfmt --edition 2024 <file>...`. If a
  full `cargo fmt` runs by accident, accept the result and move on.
- Run an example only if you changed that example, or the example is the only
  way to see your change.

| Folder   | Package              | Notes                                                                     |
|----------|----------------------|---------------------------------------------------------------------------|
| `stone/` | `procgena_stone`     | Features `gpu`, `export-obj`, `export-gltf`. The `gpu` shader tests are slow: run them only if you touched shaders or GPU code. |
| `stone/` | `procgena_stone_bevy`| Feature `gpu`                                                             |
| `tree/`  | `procgena_tree`      | Features `export-obj`, `export-gltf`. One file per area in `tests/`.     |
| `tree/`  | `procgena_tree_bevy` | Feature `gltf`                                                            |
| `clump/` | `procgena_clump`     |                                                                           |
| `clump/` | `procgena_clump_bevy`| Headless lifecycle tests in `src/spawn.rs`. `--example viewer -- --headless --screenshot <png>` checks the shaders; download textures first with `uv run --project tools textures download clump`. |
| `deadwood/` | `procgena_deadwood` | Features `export-obj`, `export-gltf`. One file per area in `tests/`. `--example export` writes every preset as GLB/OBJ to `target/procgena_deadwood_export/`. |
| `deadwood/` | `procgena_deadwood_bevy` | Headless spawn tests in `src/spawn.rs`. `--test gpu -- --include-ignored` checks the shaders. `--example viewer -- --headless --screenshot <png>` (also `--preset <name> --closeup`, `--flat`) shows the presets; it reads bark and end grain from `deadwood/assets/textures` (`uv run --project tools textures download deadwood`). |
| `impostor/` | `procgena_impostor` | Bakes generated objects into octahedral impostors (Bevy). `--example impostor -- --headless --screenshot <png>` bakes a primitive and draws it beside its impostor. |
| `mesh/`  | `procgena_mesh`      | Feature `bevy`. Every generator depends on it, so a change rebuilds most of the workspace. |
| `mushroom/` | `procgena_mushroom` | Features `export-obj`, `export-gltf`. One file per area in `tests/`.   |
| `mushroom/` | `procgena_mushroom_bevy` | Headless spawn tests in `src/spawn.rs`. `--example viewer -- --headless --screenshot <png>` checks the shaders (also `--preset <name>`, `--flat`). |
| `plant/` | `procgena_plant`     | Features `debug`, `rapier`, `avian`. Read `plant/.claude/skills/procgena-plant-style/SKILL.md` before changing it. |
| `plant/` | `procgena_plant_web` | Wasm demo                                                                 |
| `terrain/` | `procgena_terrain` | Border tests in `tests/seams.rs`, rivers and roads in `tests/features.rs`. `--example seam_spike -- <dir>` writes shaded images of joined regions. |
| `procgena/` | `procgena`        | Desktop studio app (binary). Its GPU tests are `#[ignore]`d: run with `-- --include-ignored`. Textures missing from `<project>/assets/textures/` are downloaded into the user's cache folder on first use, so the first run needs the network; `missing_textures_are_downloaded_and_drawn` always does. Use `cargo clippy -p procgena --all-targets --no-deps`. |

### The comment-rewrite step

The global instructions say to run `cargo doc --all-features` and
`cargo test --doc --all-features` after a comment rewrite. This file narrows
that rule. While other agents are active, add `-p <package>` to both commands,
and do it for each package you touched.

## The full suite (coordinator only, after all workers are done)

Wait until every worker has reported done before you start. Then run each
step once, and send each failure to the worker that owns those files. Do not
fix failures in files that a worker is still editing.

The list comes from `.github/workflows/ci.yml`. CI checks only `plant/`, so
the first three lines add the rest of the workspace.

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --workspace --all-features`
4. From `plant/`: `python tools/lint.py`, `cargo test`,
   `cargo test --features rapier`, `cargo test --features avian`, and
   `cargo test --example bench`
5. `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --all-features`
6. `cargo machete`, `typos`, `cargo deny check`
7. From `plant/`, if your machine supports these:
   `cargo check --target wasm32-unknown-unknown --lib --examples`, the
   MSRV check `cargo +1.97.1 check --all-targets --all-features`, and the
   goldens with `cargo run --example golden`

Coverage, benches (`tools/bench_*.py`) and golden blessing (`--bless`) are
run only when the user asks for them. Workers must never bless goldens.

## The shared `target/` directory

- When cargo prints `Blocking waiting for file lock`, another agent is
  building. Wait for it to finish. Never kill another agent's `cargo`
  process.
- Never run `cargo clean`, and never delete anything under `target/`.
- Do not change `[profile.*]` sections, `rust-toolchain.toml` or `RUSTFLAGS`.
  Any of these forces every agent to rebuild Bevy from scratch.
- Run long builds and tests in the background, so that other work can
  continue while you wait.

## Shared files

- Only one agent at a time may edit the root `Cargo.toml`, `Cargo.lock`,
  `deny.toml`, `_typos.toml`, `rustfmt.toml`, `rust-toolchain.toml` or
  `.github/`. Before adding or bumping a dependency, tell the coordinator
  first. A new dependency rewrites `Cargo.lock`, and every agent then
  rebuilds.
- Edit only the files that your task needs. Do not reformat or tidy files
  outside your task.
- If a file that you need is part of another agent's task, stop and ask the
  coordinator.

## Git

- Never run `git stash`, `checkout`, `switch`, `reset`, `restore`, `clean` or
  `rebase` in this checkout. Each of these changes the files that other
  agents are working in.
- Stage files by explicit path. Never use `git add -A` or `git add .`. Commit
  only when you have been asked to.

## Delegating to other tools

Codex, Pi and other agents do not read this file. When you pass work to one
of them, put the "Scoped checks", "Shared files" and "Git" rules in the
brief you give it, and point it at ARCHITECTURE.md when the work crosses
crates.
