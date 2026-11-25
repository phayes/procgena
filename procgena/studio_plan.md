# Studio revamp: plan

Status: **planned with the user on 2026-11-24.** The choices under
"Decisions" are the user's. Everything marked "proposed" is a
recommendation the user has not decided; each is listed under "Open
questions" or "Proposed, not decided". No code was changed or run.

Written from ARCHITECTURE.md (the hierarchy, "Attached assets", rule 2
"One LOD model", rule 10 "One preset and file pattern", the impostor
paragraph in "Where shared code lives", "Ecotope", "Extensibility",
"Queries: reading the result"), ROADMAP.md (IX, I1, I1b, I2, C3a, C3,
B1, GM0–GM5, X1), [ground-materials](../docs/decisions/ground-materials.md),
[D3](../docs/decisions/D3.md) section 7b, [D5](../docs/decisions/D5.md),
[fruit/plan.md](../fruit/plan.md), `procgena_core::preset`,
`procgena_impostor`, `procgena_specimen::tag`, plant's web tuning panel
(`plant/web/src/tune.rs`), and the studio code (`procgena/src/`: `studio/`,
`scene/`, `params/`, every tab's `params.rs` and `editor.rs`,
`environment.rs`, `export/`, `headless.rs`).

Milestones here are named **ST0–ST9** (studio). In ROADMAP.md they form
the "Studio track".

## Bottom line

- **One selection-driven window replaces the tabs** (decided). Left: a
  **Library** (every preset of every kind, with search, thumbnails and
  filters) above an **Outline** (the tree of what is open: World → Biome
  → Ecotope → Scatter → asset → parts → attached assets → their parts).
  Centre: the **viewport** inside a **view frame** that holds everything
  that is never saved into a preset (camera, viewing distance, LOD
  forcing, quality, environment, overlays, stats). Right: one
  **Inspector** showing only the settings of what is selected. The kinds
  (Trees, Stones, …) become Library filters. A node-link diagram of the
  same tree comes later (decided: outliner first).
- **Everything hangs off one edit model keyed by serde path.** An open
  preset is a **document**: its kind, key, origin (built-in or project),
  base, and options. Every edit is `(path, old, new)` at a dotted serde
  path such as `branch.length.0`, the same paths that today's studio
  specs use and that `preset::diff` and `merge` write. Undo and redo,
  unsaved state, "differs from base", "reset to base", save, update
  and "used by" are all built on it.
- **Metadata is YAML beside each crate's presets** (decided):
  `<kind>.meta.yml` gives each setting its label, description, unit,
  range, step, scale, the **part** it targets (branch, leaf, fruit,
  cap, …) and simple "shown if" conditions. A test in each generator
  crate keeps it in step with the options types: every serde field has
  an entry, every entry names a field. The Inspector, tooltips, search
  and part filtering are generated from it; today's hand-written studio
  specs (about 390 settings in `procgena/src/*/params.rs` and
  `environment.rs`) are migrated into it and deleted.
- **User presets live in a project folder** (decided): `presets/<kind>/`,
  `textures/`, and an ignored `.cache/` for thumbnails and bakes, all
  git-friendly. A runtime **preset registry** in `procgena_core::preset`
  layers project presets over the built-in catalogues, so user presets
  can be the `base` of other user presets, and Scatters and Ecotopes can
  refer to them by key. Built-in presets only offer "Save as new"; user
  presets offer "Update" too.
- **Saving rewrites the whole file, for now** (user decision,
  2026-11-24, replacing "Update keeps comments"): Update writes the file
  again through serde as `base` plus differences, in a stable key order,
  so diffs stay small. Hand-written comments and layout are lost on
  Update. Keeping them is a long-term goal (see decision 5).
- **Namespaced keys** (user decision, 2026-11-24): every layer has a
  namespace, `builtin` for the built-in presets and the project's id
  from its `procgena.yml` (later a bundle's id), as in
  `builtin:pine_small` or `my_forest:pine_small`. A bare key works while
  only one layer has it; a key in two layers is a problem, fixed by
  writing the namespace. `builtin` is reserved. A reference resolves
  first in the namespace of the file that wrote it (user decision,
  2026-11-24).
- **Editing in context edits the referenced preset** (decided). Click a
  pine in an Ecotope and you edit the pine preset, with its "used by"
  list in view. A built-in pine offers "Save as new", then offers to
  point this Ecotope at the new preset.
- **Clicking a part in the viewport selects that part's settings.** First
  by the tags every specimen part already carries (bark, leaf, fruit,
  flower), mapped to a part through the metadata; later by D3 part paths
  (`fruit 17 / item 2`) once X1 and fruit's AC1 give assets `part()`.
  Selection is always at the options level: the document, the child by
  key, and the part kind. An address only gets you there.
- **One capture service takes pictures of anything** (proposed). It owns
  the offscreen camera on its own render layer, framing from bounds, a
  queue with a per-frame budget, GPU readback and the disk cache. The
  impostor bake becomes one user of it (octahedral views, albedo and
  normal outputs); preset thumbnails, turntables and screenshots are
  others. Thumbnails are cached by content hash and never committed
  (decided).
- **One texture catalogue** (decided, shared with GM2): an engine-free
  `TextureCatalogue` in `procgena_core` lists every texture set. Every
  crate's `textures.yml`, the terrain's ground textures and the
  project's own `textures.yml` are its sources. The ground *material*
  catalogue stays in the terrain crates as ground-materials.md decided,
  and names texture ids from it, so there is one texture library, not
  two. Replacing the compile-time kind lists (`BARK_KINDS`, …) with it is
  proposed. Importing converts images to BC7 KTX2
  with mipmaps and Zstandard right away, in Rust, with `ctt` (decided);
  UASTC copies for shipping stay with the Python tool.
- **Viewing distance is split from LOD and lives on the view frame**
  (decided). Look at a tree up close while it draws as it would at
  300 m, impostor included. This needs a per-view LOD distance override
  that every renderer honours; that is a **requirement on IX**, whose
  plan designs the API (decided). ST7b waits for it.
- **Staging: one churn window, then lanes in separate folders.** ST0
  runs alone, after the Fruit and Flower tabs land, and replaces the
  enumerations of every kind (`StudioTab`, `SceneMode`, `SceneStart`,
  `BuildState`, `editing.rs`) with a kind registry and the new layout.
  After it, each milestone adds its own module (`studio/library/`,
  `studio/outline/`, …) and up to three lanes run side by side.
  Metadata files and the preset registry can start in the generator and
  core crates while ST0 runs.

## Requirements map

| # | Requirement (the user's) | Design | Milestone |
|---|---|---|---|
| 1 | Save our own named presets | Project folder, preset registry, documents | ST2 |
| 2 | A nice UI for navigating presets and assets | Library | ST3 |
| 3 | Tree graph of how the parts fit together | Outline (outliner); diagram later | ST3; ST9 |
| 4 | Preset icon or preview picture, impostor machinery made generic | Capture service, thumbnail cache | ST4 |
| 5 | Texture library with import | Texture catalogue, import with `ctt`, Library's texture view | ST6 |
| 6 | Preset library with search | Library search over the registry and metadata | ST3 |
| 7 | Metadata per type and setting, what it does and which part it targets; click a part to get its editor; "update" or "save as new" | `<kind>.meta.yml`, Inspector, picking, documents | ST1, ST5, ST2 |
| 8 | Names and descriptions in the metadata feed the UI | Labels, tooltips, part names and kind names from metadata | ST1 |
| 9 | Split viewing distance from LOD | Per-view LOD distance override (IX), view-frame control; game default (level 1) shown apart from level 0 preview | ST7b |
| 10 | Viewing distance on the view frame | View frame | ST0, ST7b |
| 11 | Environment on the view frame; clear what is saved and what is studio-only | View frame, "what is saved where" table | ST0 |

## Decisions

Made by the user on 2026-11-24 in this planning session.

1. **Navigation.** Replace the tabs with Library and Outline on the left,
   the viewport and view frame in the centre, and one Inspector on the
   right. Kinds become Library filters; the Fruit and Flower tabs (decided
   the same day under I1b) become fruit and flower documents opened from
   the Library or from a tree's Outline. ARCHITECTURE.md's "Studio: a tab
   per new level" TODO is reworded to match.
2. **Tree graph.** An outliner first: an indented, collapsible tree with
   icons and thumbnails. A node-link diagram of the same data is a later
   milestone (ST9).
3. **Metadata.** YAML beside the presets, `<kind>.meta.yml` in each
   generator crate, with a drift test in that crate. No new dependency.
4. **User presets.** A project folder: `presets/<kind>/<key>.<kind>.yml`,
   `textures/` with its `textures.yml`, and an ignored `.cache/`. A
   default project in the user's Documents folder on first run. User
   presets can be the base of other user presets.
5. **Saving a preset rewrites its file; keeping comments is a long-term
   goal** (user decision, 2026-11-24, replacing the earlier choice of
   `yamlpatch` + `yamlpath`). Update writes the whole file through serde,
   deterministically: `base`, `name`, `description`, `group`, then only
   the settings that differ from the base, in the options' declared
   order. Comments and hand layout are lost, and the studio and the docs
   say so. No dependency and no patcher are added now. The ST2 spike's
   findings, kept as input for the later goal:

   | | `yamlpatch` + `yamlpath` | `yaml-edit` | Own patcher on `granit-parser` |
   |---|---|---|---|
   | Version, licence | 1.33.1, MIT | 0.3.6, Apache-2.0 | 2.2.0, MIT or Apache-2.0 |
   | Downloads (all / recent) | 463k / 211k and 774k / 292k | 371k / 305k | 19.5M / 16.9M |
   | Already in the tree | No | No | Yes, through `serde-saphyr` 2.2 |
   | New crates in `Cargo.lock` | About 11, including `tree-sitter`, `tree-sitter-yaml`, `yaml_serde` and `libyaml-rs` | About 4, through `rowan` | None |
   | Build | Two C builds through `cc`; two more YAML parsers | Pure Rust | Pure Rust |
   | Wasm | Needs a C toolchain for `wasm32-unknown-unknown` (unverified) | Likely fine | Fine |
   | Notes | zizmor project, active; abilities from search results only | Reported to misplace comments | 300–400 lines of our own; block-style maps only; spans not yet tried in a scratch build |

   Figures from crates.io on 2026-11-24. Whoever takes up the goal runs
   the replace / insert nested key / remove spike first.
6. **Thumbnails are cached, not committed.** Keyed by content hash under
   the project's `.cache/`. (Whether built-in presets' thumbnails ship
   pre-rendered is open question 4.)
7. **Textures convert on import,** with `ctt` (MIT/Apache/Zlib, v0.6.1,
   about 100k downloads): BC7 KTX2 with mipmaps and Zstandard. UASTC
   copies are made by the Python tool when publishing. Chosen over adding
   `basisu_c_sys` (a C++ build) and over `intel_tex_2` + `ktx2` (our own
   mip and container code). Its abilities also come from search results:
   **ST6 starts with a spike** converting one bark set and loading it in
   Bevy; if it fails, the fallback is `intel_tex_2` + `ktx2`.
8. **Viewing distance is a requirement on IX.** This plan states what the
   studio needs; IX's plan designs the per-view override in the shared LOD
   runtime.
9. **Early extras:** undo, redo, unsaved state and reset to base;
   debug overlays, live stats and a problems panel; export,
   screenshot and turntable capture, a command palette and shortcuts. A/B
   compare and the seed browser come later.
10. **Editing in context edits the referenced preset.** No inline
    overrides inside Scatter or Ecotope files.
11. **One texture catalogue in `procgena_core`.** It lists every texture
    set (every crate's, the terrain's ground textures, the project's).
    The ground material catalogue stays in the terrain crates, as
    ground-materials.md decided, and names texture ids from it.

### Decided for ST0 (2026-11-24, asked by the ST0 worker)

12. **Detail starts at "Game default"** before IX's API exists: levels by
    distance, never finer than level 1 near the camera, through the
    studio's level forcing. "Level 0 preview" is levels by distance from
    level 0. Subjects that can't leave out finer levels (plants, rocks,
    fruit) draw every level; a clump's host draws level 0, the level the
    clump grows on; groves and Ecotopes choose per copy by distance.
13. **Documents are identity in ST0.** Each document's typed editor
    (`ParamPanel<T>`) holds its settings; the `Document` names the preset
    and keeps its camera view and detail choice. The `Value` edit model
    comes with ST1 and ST2.
14. **Fruit and flowers are two kinds and two file types**
    (`.fruit.yml`, `.flower.yml`).
15. **The studio opens one document** (the `oak_medium` tree). Any number
    of documents may be open, several of one kind included; opening a
    preset that is open selects it. Each document keeps its own camera
    view.

### Decided for ST1 (2026-11-24, asked by the ST1 worker)

16. **Named conditions.** The "shown if" language gains parentheses, and
    each metadata file a `conditions:` block of named conditions used as
    `@name` (with `{index}` and `{parent}` allowed in the name, such as
    `@grows_{index}`). Chains such as "every level above this one grows"
    are written out once per level there, rather than growing the language.
17. **Revive values as data:** a number, or
    `{ from: <path>, times, min, if_zero }` for values taken from another
    setting, such as a new branch level's length from its parent's.
18. **The first metadata files were drafted by a throwaway dump** of the
    hand-written specs (keys, labels, ranges, steps, scales, choice names).
    Conditions, revives and descriptions were written by hand. The dump
    was not committed.
19. **Choices from Rust lists** (texture sets, stone detail levels, dead
    wood end shapes) are named in the metadata file (`choices: bark_kinds`)
    and supplied by the crate as `(label, value)` pairs in an engine-free
    function. A choice of maps with a `kind` keeps that kind's settings
    when the same kind is picked again. Only the tree's fruit and flower
    preset picker stays a custom Inspector section.

### Proposed, not decided

- The document edit model (`Value` and path edits, ST1 and ST2). The kind
  registry, documents as identity and `Selection { document, node, part }`
  are built as ST0 (see "ST0 as built").
- The metadata file's exact shape and its "shown if" language.
- The capture service in `procgena_core_bevy`, with the impostor bake on
  top (open question 5).
- Replacing the compile-time texture kind lists with catalogue lookups.
- The "what is saved where" table: environment, preview area, detail
  and quality as view-frame settings kept in session state.
- Picking through tags first and part paths later.

## The window

```
┌──────────────┬───────────────────────────────────────┬──────────────┐
│ Library      │ View frame: camera · distance · LOD · │ Inspector    │
│  search      │   quality · environment · overlays    │  doc header: │
│  kind chips  │ ┌───────────────────────────────────┐ │  name, key,  │
│  thumbnails  │ │                                   │ │  origin,     │
│──────────────│ │             viewport              │ │  Save / Save │
│ Outline      │ │                                   │ │  as new      │
│  Ecotope     │ └───────────────────────────────────┘ │  part chips  │
│   ▸ pines    │ stats · build status · LOD readout    │  settings of │
│     ▸ pine_… │                                       │  the part    │
├──────────────┴───────────────────────────────────────┴──────────────┤
│ Problems (collapsible) · toasts                                     │
└─────────────────────────────────────────────────────────────────────┘
```

- **Library** (ST3): every preset in the registry, built-in and project,
  as a grid of thumbnails or a list. Search matches name, key,
  description, group, kind and the metadata's part names, with substring
  and word-start matching (no fuzzy-search dependency). Filters by kind,
  origin and group; recent and pinned presets at the top. Actions: open,
  duplicate (save as new), rename, delete (moves the file to the system
  trash after confirming), reveal in Finder, show "used by". A second
  view lists textures (ST6).
- **Outline** (ST3): the open document as a tree (see "Outline and
  selection"). Icons for kinds and parts, thumbnails for referenced
  presets, a link mark on references, a dot on nodes with unsaved
  changes or problems.
- **Viewport and view frame** (ST0, ST7): the scene, with the controls
  that are not part of any preset on a bar above it and a readout below
  (see "The view frame").
- **Inspector** (ST1): a header for the document (name, key, origin,
  base, Save, Save as new, Revert), then chips for the parts of the
  selection, then the settings of the selected part, built from the
  metadata. Each changed setting has a "differs from base" dot with
  "reset to base" (ST2).
- **Problems** (ST7a): validation errors, unresolved references, missing
  textures and files that failed to read, each linking to its setting.

Today's per-tab extras (preset and seed rows, export panels, the
Ecotope's Scatter list editor) move into the Inspector header or into a
kind's custom Inspector sections (see "The kind registry").

## Documents and the edit model

A **document** is one open preset:

```rust
pub struct Document {
    pub kind: KindId,                 // "tree", "tree_scatter", "ecotope", …
    pub key: String,                  // the preset's key
    pub origin: Origin,               // BuiltIn, Project(PathBuf), Unsaved
    pub base: Option<PresetRef>,      // the preset this one is written over
    pub value: serde_json::Value,     // the options as the file's settings tree
    saved: Value,                     // as last saved or opened
    undo: Vec<EditGroup>, redo: Vec<EditGroup>,
}

pub struct Edit { pub path: SettingPath, pub old: Value, pub new: Value }
```

(Sketch, not an API. `Value` is `serde_json::Value`, which
`procgena_core::preset` already uses for `diff`, `merge`, `to_value` and
`from_value`.)

- **One path language.** A `SettingPath` is the dotted serde path the
  studio specs use today (`bark.texture_scale.x`, `branch.length.0`),
  plus `[i]` for list items and the child key for keyed maps
  (`scatters.pines.density_scale`). The metadata, the drift test, file
  diffs, updates, undo and reset all use it.
- **Typed options come from the value.** After each edit the document
  rebuilds its kind's options with `from_value` and sends them to the
  scene. Options are small (a tree's is a few hundred values), so this
  is cheap even during a slider drag; ST1 measures it on the tree. The
  kind's `validate()` and `Preset::check` stay the authority on what is
  valid; metadata ranges only shape the controls.
- **Undo and redo** (ST2) per document. A slider drag or a text entry
  coalesces into one group; structural edits (adding a Scatter, a list
  item) are path edits too. Undo crosses saves.
- **Unsaved state.** A document is dirty while `value != saved`. The
  Outline, Library and window title mark it, and quitting or closing asks
  first (today's quit handling in `studio/quit.rs` grows this).
- **Differs from base and reset to base.** `preset::diff(base, value)`
  gives the set of changed paths; the Inspector marks them, and reset
  writes the base's value back as an ordinary edit.
- **Save.** A built-in preset offers only "Save as new". A project preset
  offers "Update" and "Save as new" (write a new file over the same base).
  Both write the whole file: its `base` and name parts, then the
  settings that differ from the base (`preset::diff`) in a stable order.
  Comments and layout in an updated file are lost (decision 5).
  References to other presets are written as keys, as the document holds
  them. A key another project preset has is refused. A built-in
  preset's key is allowed, though it makes bare references to it
  ambiguous, so the dialog proposes a free key such as `pine_small_2`.
- **Several documents open at once,** for editing in context: opening a
  pine from an Ecotope opens the pine's document as well, and the Outline
  shows it under the Ecotope. Saving one never saves another.

## The kind registry

Today every kind is spelled out across the studio: `StudioTab` (10
variants), `SceneMode`, `SceneStart`, `BuildState`, the `spawn_editors`
match, `ViewerCommand` variants per kind, and an `XEditorPlugin` per tab.
Adding Fruit and Flower touches all of them. ST0 replaces that with a
registry of kinds, so each later kind (fruit, flower, Biome, World,
ground material) is one new module plus one registration line.

A kind registration says (sketch):

- **Identity:** `KindId` (the preset file kind, `"tree"`), its metadata,
  its preset type (through a small object-safe wrapper over `Preset`).
- **Scene:** how to show a document of this kind: a scene subject plugin
  that takes the typed options (today's `scene/subject.rs` arms, split per
  kind) and reports build status and stats.
- **Children:** the references and inline children in a value, for the
  Outline and "used by". Generic from metadata (`ref: tree` fields and
  keyed maps), so most kinds need nothing here.
- **Capture recipe:** how to frame a thumbnail (single object, patch from
  above, terrain overview).
- **Custom Inspector sections,** where the metadata can't describe the
  control: the Ecotope's Scatter list (add, remove, rename a key, pick a
  kind), the tree Scatter's species list, the Terrain's preview controls.
- **Picking map** from specimen tags to parts (also from metadata).

The registry lives in the studio (`procgena/src/studio/kinds/`). The
engine-free parts (metadata, preset registry, references) live in
`procgena_core` so games and other tools can use them.

## Metadata

### The file

Each generator crate ships one file per preset kind next to its presets,
for example `tree/crates/procgena_tree/presets/tree.meta.yml`,
embedded with `include_str!` like the presets. Proposed shape:

```yaml
# tree.meta.yml
kind: tree
name: Tree
description: "One tree: trunk, branches, leaves, and optional fruit and flowers."
parts:
  trunk:   { name: Trunk,    description: "The main stem.",     tags: [trunk] }
  branch:  { name: Branches, description: "Every branch level.", tags: [limb, branch] }
  bark:    { name: Bark,     description: "The bark texture and tint on trunk and branches." }
  leaf:    { name: Leaves,   tags: [leaf] }
  fruit:   { name: Fruit,    tags: [fruit],  attached: fruit }     # an attached asset
  flower:  { name: Flowers,  tags: [flower], attached: flower }
sections:                         # Inspector order within each part
  - { title: Look, part: bark, settings: [bark.tint, bark.flat_shading, bark.textured] }
settings:
  bark.texture_scale.x:
    label: Texture Scale X
    description: How many times the bark texture repeats around the trunk.
    unit: repeats
    range: [5, 200]
    step: 1
    scale: exponential
    shown_if: bark.textured
  bark.kind:
    label: Type
    texture: bark                 # chooses from the texture catalogue's bark category
  branch.length.*:                # one entry for every branch level
    label: Length
    unit: m
    range: [0.1, 50]
    part: branch
    shown_if: "levels > {index}"
  fruit.preset:
    label: Fruit
    ref: fruit                    # a preset key of another kind: drives Outline and "used by"
```

- **Entries per setting:** `label`, `description` (written for end
  users; Rust doc comments stay written for programmers), `unit`,
  `range`, `step`, `scale` (linear or exponential), a control override
  (`color`, `choice` with labels for enum variants, `texture: <category>`,
  `ref: <kind>`), `part`, `revive` (the value to use when a setting comes
  back into effect, today's `revive` closures), `advanced` (folded away by
  default), `hidden` (in the file format but not in the Inspector, such as
  `default_seed`, which the Inspector header edits).
- **"Shown if" conditions** are a tiny language over other settings:
  `path`, `!path`, `path == value`, `path in [a, b]`, `path > n`, joined
  with `&&` and `||`, with `{index}` for the list or level index. It
  replaces the 86 `shown_if` closures in today's specs (counted
  2026-11-24, plus a few dozen `revive` values, which become data).
  Anything it can't say belongs in a custom Inspector section.
- **Parts** name what a setting targets, give the picking map (`tags`),
  and mark attached assets (`attached: fruit`), so the Outline can show a
  tree's fruit as a link to the fruit preset.
- **Patterns:** `*` matches a list index, a level, or a map key, so one
  entry covers every branch level or every Scatter in an Ecotope.
- **Enums:** each variant's settings are listed under the enum's path
  with the variant name, for example `leaf.billboard{Cross}.angle`, so a
  variant not used by any built-in preset is still covered.

### Reading it

`procgena_core::meta` (engine-free, new module) parses the file into
`KindMeta`, `PartMeta`, `SettingMeta` and `Condition`, matches setting
paths against patterns, and evaluates conditions over a `Value`. Games
can use it for their own tools.

### Keeping it in step with the code

A helper in `procgena_core::meta`, called from one test in each generator
crate:

- serialises the defaults, every built-in preset, and each enum variant
  the metadata names, and collects every leaf path;
- fails if a leaf path has no entry (or `hidden: true`), if an entry
  matches no path, if a `shown_if` names a missing path, if a `ref`
  names an unknown kind, or if a part has no setting;
- reports any built-in preset value outside its entry's `range` (the
  range is too narrow).

This replaces today's studio test that each spec key names the field it
writes, and moves the check next to the options types, so a renamed
field fails in the crate that renamed it.

### Migration

The first metadata files are written from today's specs: labels, ranges,
steps, scales and conditions are in `procgena/src/*/params.rs` already.
Descriptions are new writing, taken from the options' doc comments and
rewritten for end users. This is a large mechanical step; per the user's
rules, the worker asks before using a script to draft it.

## Preset registry and the project folder

### The project folder

```
my_project/
  procgena.yml                  # format: 1, id (the namespace), name
  presets/
    tree/        tall_pine.tree.yml
    tree_scatter/ pine_belt.tree_scatter.yml
    ecotope/     dark_forest.ecotope.yml
    fruit/ …
  textures/
    textures.yml                # the project's texture catalogue
    bark/my_bark/…              # converted BC7 KTX2 (512, 1k, 2k)
  .cache/                       # ignored: thumbnails, impostor bakes
  .gitignore                    # written when the project is made: .cache/
```

- Files are the F3 format unchanged: `base`, `name`, `description`,
  `group`, and the options under the kind's key.
- **Studio session state is not in the project:** open documents,
  window layout, view-frame settings and camera bookmarks are kept per
  project under the user's app-data folder, so switching machines or
  pulling a branch never conflicts on them.
- **Outside edits reload.** The studio watches `presets/` and
  `textures/`; a changed file reloads its document if it is clean, and
  shows a problem if it has unsaved edits.

### The registry

`procgena_core::preset::Registry` (built in ST2, additive):

- **Layers:** the built-in catalogue (today's `&'static` presets) and
  one project. `Registry::register::<P>()` adds a kind;
  `load_folder(presets)` reads `presets/<kind>/<key>.<kind>.yml`,
  reporting and skipping misplaced files, unknown kind folders, bad keys
  and unreadable files.
- **Owned presets.** `resolve::<P>(key)` returns a `PresetRef<P>`:
  `&'static` for a built-in, `Arc<P>` for a project preset. `Preset`,
  `from_parts`, `read` and `catalogue!` keep `&'static`: a project
  preset's inherited fields (such as a tree's stand) come from the
  built-in preset at the root of its `base` chain, since files can't set
  them. The workspace-wide move to `PresetRef` (stage a′) is not needed.
- **User bases.** A file's `base` may name a project preset; the chain is
  laid from its root, and a loop is an error naming the chain.
- **References resolve through the registry.** A kind whose settings
  name other presets by key gives a `References` (`Preset::references`,
  `references:` in `catalogue!`): where its references are, and how to
  replace them with the settings they name before reading. The tree
  Scatter and the Ecotope have them; the Biome's Terrain waits for
  `.biome.yml` files (B1, W0). Built-in presets refer only to built-in
  presets. Reading without a registry is unchanged.
- **Used by.** `used_by(kind, key)` lists presets that use a preset as
  their `base` or refer to it, with the setting's path. ST1's metadata
  `ref` fields can replace the per-kind slot lists later.
- **Namespaced keys** (user decision, 2026-11-24). A reference is a bare
  key or `namespace:key`, read by one function (`Reference::parse`). The
  namespaces are `builtin` and the project's `id` from `procgena.yml`
  (`Registry::set_namespace`); later each bundle's id. A bare key in more
  than one layer is an `Ambiguous` error naming the qualified choices,
  and `problems()` lists every shared key, including one a newer
  procgena's built-ins introduce, so nothing silently changes meaning.
  Writing the namespace fixes it; the rename dialog is a convenience.
  **A reference resolves first in the namespace of the file that wrote
  it** (user decision, 2026-11-24): names in a built-in file always mean
  `builtin:` (the registry writes `builtin:` before them when it lays a
  built-in base), and a project file sees the project's presets first,
  with a bare key going to the general lookup only when the project
  lacks it. A file's bare `base` never names the file itself, so
  `pine_small.tree.yml` with `base: pine_small` is written over the
  built-in one.
  The namespace is never part of a preset's key, so it never reaches
  child keys or seeds. `builtin`, and any id that isn't a valid key, is
  refused when `procgena.yml` is read or a project is made. Two loaded
  layers can't share a namespace: today the only case is a project named
  `builtin`; when bundles are mounted, a bundle whose id is already
  loaded is refused with a problem and not loaded.

## Outline and selection

One `Selection` resource drives the Outline, the Inspector and the
viewport highlight:

```rust
pub struct Selection {
    pub document: DocumentId,
    pub node: NodePath,          // child keys and list items inside the document,
                                 // e.g. scatters.pines / trees[1]
    pub part: Option<PartId>,    // "branch", "leaf", "fruit", from the metadata
}
```

- **The Outline is built from documents.** An Ecotope lists its Scatters
  by key in priority order (inline children); each Scatter lists the
  presets it refers to (references, with a link mark and a thumbnail);
  each asset lists its parts from the metadata; an attached part (fruit,
  flower) lists the fruit or flower preset it refers to, and so on.
  Biome and World documents join with B1 and W1 (a Biome shows its
  Terrain and Ecotope rules above its Ecotopes).
- **Selecting a reference opens the referenced document** (decided:
  edits go to the referenced preset), keeping the Outline anchored in the
  parent so the context is still visible.
- **The Inspector shows the selected node's settings filtered to the
  selected part,** with part chips to switch. Selecting the asset itself
  shows every part, grouped.
- **Addresses are not selection.** A D3 `Address` names one copy or one
  fruit in a world; it is used only to get from a pick to a document,
  child and part. Selection stays at the options level, because the
  settings are per part kind (all branches of level 2 share settings),
  not per instance.

## Picking in the viewport

- **ST5a, by tags (needs only ST3).** A click casts a ray on the CPU:
  - for a single asset, against its CPU specimen's parts (each
    `procgena_specimen::Part` carries tags such as `bark`, `trunk`,
    `limb`, `branch`, `leaf`, `fruit`, `flower`, `cap`, `stem`), and the
    metadata's part `tags` map the hit part to a part id.
  - **Branch levels need tree-specific work:** the tree's `Specimen`
    tags its bark by role (trunk, limb, branch), not by level, while its
    settings are per level. The tree's picking adapter casts against
    `TreeMesh::part(MeshPart::BranchLevel(n))` instead, so a hit names the
    level and the Inspector opens that level's settings.
  - for a Scatter or Ecotope, first against copy bounds
    (`TreeScatterTrees::ray_cast` today; R4 gives every Scatter
    `within` and ray casts), then into that copy's specimen as above.
    The hit names the Scatter by key and the specimen's preset, which
    gives the document, node and part.
  - Hover shows the part's name and description as a tooltip; the
    selected part is drawn highlighted with an outline pass over its
    part mesh.
- **ST5b, by part paths (after X1 and fruit AC1).** Assets with
  `part()` (tree first, then fruit and the mushroom ring) turn a hit into
  a D3 part path, so a single fruit or branch can be highlighted and its
  address shown. Editing still happens at the part kind.
- **Plants** are built in a shader, with no CPU specimen until R3, so
  they pick as a whole Plant Scatter until then.

## The capture service

Today `procgena_impostor` owns an offscreen bake camera
(`ImpostorBakeCamera`), a request queue (`Impostors::request`,
`ImpostorBatch`), readback and a disk cache (`cache.rs`), all specialised
to octahedral atlases. Thumbnails, turntables and screenshots need the
same machinery with different views and outputs.

**Proposed:** a `capture` module in `procgena_core_bevy` (the shared Bevy
runtime), which the impostor bake then uses.

| Shared (capture) | Stays in the impostor bake |
|---|---|
| Offscreen camera on an isolated render layer, sized render targets | Octahedral and elevation-strip view layouts, view coverage |
| Framing from bounds (fit, margin, ¾ view, top-down) | Albedo and normal material overrides, atlas packing |
| A request queue with a per-frame budget and cancellation | `ImpostorKey`, `BakeSettings`, the impostor material |
| GPU readback to an `Image` and to PNG | The bake version and its cache layout |
| A disk cache by key with pruning, and its writer thread | |

- **Outputs:** lit colour (thumbnails, turntables, screenshots), or
  material overrides supplied by the caller (impostors).
- **Subjects:** a single specimen spawned on the capture layer, or a
  whole scene (a Scatter or Ecotope patch, a Terrain overview), spawned
  by the kind's scene subject on that layer. Plants are captured as
  scenes.
- **Thumbnail key:** the kind, the options' content hash (F2's
  `OptionsHash`), `default_seed`, the capture recipe and a
  `THUMBNAIL_VERSION`. A preset whose references change (a tree used by
  an Ecotope) changes the key through the referenced presets' hashes.
- **Thumbnail cache:** `<project>/.cache/thumbnails/<kind>/<key>.png`,
  never committed (decided). Built-in presets' thumbnails are rendered
  ahead by a headless command (`procgena --render-thumbnails <dir>`) and
  shipped in the studio's assets.
- **Budget:** thumbnails render in the background a few per frame while
  the studio is idle, never during a slider drag, and only for presets
  scrolled into view.

The move touches `procgena_impostor` (bake on top of capture), which the
fruit D2 and IX lanes also touch, so ST4's crate half is scheduled with
them (see "Stages"). `procgena_impostor` depends only on `procgena_core`
today and `procgena_core_bevy` does not depend on it (checked
2026-11-24), so the impostor crate gaining a dependency on
`procgena_core_bevy` makes no cycle.

## The texture library

Today each generator lists its textures in `<crate>/textures.yml`, the
Python `textures` tool converts and publishes them, the studio downloads
missing files into the user's cache (`scene/assets.rs`), and the valid
kinds are compile-time arrays in Rust (`BARK_KINDS`, `LEAF_KINDS`, and
the like). GM2 plans a ground texture list (`terrain/textures.yml`) and a
shared loader.

**One catalogue, shared with GM2** (decided; the details below are
proposed).

- **`procgena_core::texture::TextureCatalogue`** (engine-free): entries
  with an id, a category (`bark`, `leaf`, `rock`, `ground`, `snow`,
  `card`, …), the map layout (`roughness` or `orh`), sizes and formats on
  disk, tile size in metres where it applies, licence, source and author.
  Its sources are every crate's `textures.yml`, the terrain's ground list,
  and the project's `textures/textures.yml`.
- **Options keep string ids** (`BarkOptions::kind`); validity moves from
  the const arrays to a catalogue lookup, reported by `validate` with the
  catalogue passed in, and by the Problems panel.
- **GM2 owns the loader** (`procgena_core_bevy`, rule 13) and the ground
  arrays; the terrain crates keep the ground material catalogue (decided
  in ground-materials.md), whose materials name texture ids in this
  catalogue (decided here). The studio owns the import and the library
  view. The two meet at the catalogue type, which ST6 or GM2 lands
  first, whichever starts first; the other builds on it.
- **Import** (ST6): choose the maps (colour, normal, roughness or
  packed AO/roughness/height; a folder is matched by file-name
  suggestions), a category, an id and the licence (required, as the repo
  takes only CC0 or MIT-style textures). The studio converts each map
  with `ctt` to BC7 KTX2 with mipmaps and Zstandard at 512, 1k and 2k,
  writes them under `textures/<category>/<id>/`, adds the entry to the
  project's `textures.yml`, and registers the project as one more asset
  source. The source images are kept beside them only if the user asks
  (they are large).
- **The library's texture view** shows each set as a lit sphere or flat
  swatch (through the capture service), filtered by category, with "used
  by". A texture picker in the Inspector (`texture: bark` in the
  metadata) opens the same view.
- **Publishing:** the Python tool learns to read a project's
  `textures.yml`, to make UASTC copies and export images.

## The view frame

The view frame holds everything that is not saved into a preset. Its
settings are kept per project in the studio's session state (app data),
never in project files.

**What is saved where** (today's settings, classified):

| Setting | Today | Saved in |
|---|---|---|
| Every asset, Scatter, Ecotope and Terrain option | Tab panels | **Preset** (project file) |
| `default_seed` | Seed row on each tab | **Preset** (Inspector header) |
| Sun angle and height, wind on/off | Environment panel (every tab) | **View frame**, session only. Later read-only "from Biome" when previewing inside a Biome that owns its climate |
| Show ground | Environment panel | View frame |
| Field size (plant), Scatter size (tree Scatter) | Environment panel, per tab | View frame ("preview area"): a Scatter's area comes from above, never from its preset |
| Auto rotate, speed, reset camera | Camera overlay | View frame |
| Camera mode (orbit or fly), flying speed | View frame bar | View frame, session only, one for the whole studio (not per document); see "Camera modes" |
| Camera pose | Kept per open document | Session only (decision 15) |
| Level of detail dropdowns (tree, mushroom, dead wood, plant) | Settings column | View frame: one "Detail" control (game default, level 0 preview, any level, impostor); see "Game default and level 0" |
| Level 0 opt-in, per asset | — | **Preset**, if IX puts it in the asset's options |
| Level 0 opt-in and quality, per game | — | **Project** (`procgena.yml` game profile), as IX decides |
| Viewing distance | — | View frame (ST7b) |
| Quality (`LOW`..`ULTRA`) | — | View frame |
| Overlays, stats | — | View frame |
| Impostor bake settings, LOD distances of a preset | Preset options where they exist | Preset |

The bar above the viewport groups them: **Camera** (orbit, frame
selection, bookmarks, auto rotate), **Distance** (viewing distance,
"lock to camera"), **Detail** (auto, level N, impostor), **Quality**,
**Environment** (a popover: sun, wind, ground, preview area),
**Overlays**. The readout below shows triangles, draw calls, the level
drawn and build or bake progress.

### Camera modes

Built 2026-11-24 on Bevy alone (`scene/fly.rs`, `studio/camera.rs`).

- **Orbit** is the camera as before. **Fly** moves with W, A, S and D
  (level, whatever the pitch), Space rises and Shift drops, as in a
  creative-mode game. The view frame's Orbit and Fly buttons, the `C`
  key and the commands `orbit-camera`, `fly-camera` and
  `toggle-camera-mode` switch between them.
- **Looking** is a drag with any button over the 3D view, with no cursor
  capture: Escape already asks before quitting, the panels stay usable
  while flying, a captured cursor can't be driven headless, and a click
  without a drag stays free for picking (ST5).
- **Speed** is fitted to the view whenever flying starts or the camera is
  set while flying (another subject, Reset Camera): half the orbit
  distance per second, so a mushroom flies at centimetres and terrain at
  hundreds of metres per second. The wheel scales it (1/100 to 100
  times), and the scale is kept across subjects. The bar shows it.
- **Keyboard focus:** keys fly only while no UI element has keyboard
  focus, no dialog is open, and neither Ctrl nor Cmd is held. Clicking
  the 3D view takes focus back, and clicking a camera control gives it
  back at once.
- **One pose for both:** `OrbitCamera` holds the pose in both modes; while
  flying its focus point sits straight ahead at the orbit's distance.
  Fly starts from the orbit's pose. Orbit starts around that point
  ahead, keeping the camera where it is and tilting it only as far as
  the subject's tilt limits need. Documents keep their own pose as
  before.
- **`PanOrbitCamera`** (proposed under "Bevy editor") would replace only
  the orbit half. Bevy's `bevy_camera_controller` (a Bevy feature not
  turned on here) has it and a `FreeCamera`, but neither shares a pose
  between the two, guards keyboard focus, or fits the speed to the
  subject, so adopting them buys little now.

### Viewing distance (requirement on IX)

The studio needs, from the shared LOD runtime (`procgena_core_bevy`,
designed in IX's plan):

1. A **per-view LOD distance override**: a camera component that says
   "choose levels as if every subject were `d` metres away", honoured by
   entity levels (today Bevy's `VisibilityRange`), the shared instanced
   renderer's GPU cull and level choice, plant's shader levels, fruit's
   two-step check, and the impostor level. Shadows keep using the real
   view.
2. **Forcing a level** ("show level N", "show impostor") through the same
   component, replacing today's four per-kind `SetLod` commands.
3. A way to **read back which level each subject drew**, for the readout.

With these, "look up close, see the 300 m impostor" is the override set
to 300 with the camera wherever the user puts it. ST7b builds the view
frame control and waits for this piece of IX, which can be pulled forward
on its own.

### Game default and level 0

Decided by the user 2026-11-24 and recorded under IX in ROADMAP.md:
every asset draws at detail level 1 by default, near the camera too, for
meshes and textures; full-detail level 0 is opt-in, per game or per
asset. IX's plan designs the API; the studio is planned around it, and
shows both what a game gets and level 0 where it is switched on.

- **Two different things, kept apart in the UI:**
  - **The opt-in** is saved data. Per asset, it is a setting of the
    preset (if IX puts it in the asset's options), shown in the
    Inspector's Detail section from the metadata. Per game, it lives in
    the project's game settings (`procgena.yml`, a "game profile" with
    the quality and level-0 choices), since a project stands for one
    game's content. Where exactly each lives follows IX.
  - **Previewing level 0** is a view-frame setting, never saved: look at
    an asset in full detail while editing it, whether or not the game
    turns level 0 on.
- **The Detail control on the view frame** offers: *Game default* (what
  the game draws: level 1, or level 0 where the preset or the game
  profile opts in), *Level 0 preview*, and forcing any level or the
  impostor. Game default is the starting choice, so what you see is what
  the game gets unless you ask otherwise.
- **The readout says which and why:** for example "level 1 · game
  default", "level 0 · opted in by this preset", or "level 0 · preview
  only, not on in this game". Previewing level 0 when it is not opted in
  shows a mark on the view frame, so a screenshot or a judgement isn't
  made on detail the game won't draw.
- **Settings that only show at level 0** (full-size textures, the
  finest mesh detail) carry `levels: [0]` in the metadata. The Inspector
  marks them "level 0 only", says when the game won't draw them, and
  offers to switch the view to Level 0 preview while one is edited.
- **Thumbnails and captures** use the game default unless their recipe
  asks otherwise, so the Library shows what a game gets.
- **What the studio needs from IX's API** (added to IX's studio
  requirement): the effective level for an asset under a game profile
  and why (preset opt-in, game opt-in, or default); a per-view override
  that draws level 0 even when not opted in; the levels each asset has,
  with names, for the Detail control; and where the per-asset and
  per-game opt-ins are stored, so the metadata and the project file can
  show them.

## Problems, overlays and stats

- **Problems panel** (ST7a): each open document's `validate()` and
  `check`, unresolved references, missing textures, project files that
  failed to read (with the file and setting path from `PresetFileError`),
  key clashes, and base cycles. Clicking a problem selects the setting.
- **Overlays** (ST7a, each as far as its renderer offers it): LOD level
  colours per copy, Scatter footprints and masks (the Ecotope already
  computes footprints), terrain chunk borders and levels (the terrain
  viewer has `--borders` and `--levels`), bounds, and normals.
- **As built: the segmentation overlay** (ST7a's first overlay). An
  Overlay picker on the view frame's bar, shown while the subject offers
  overlays: Regions, Landform (G4b's labels), Ground materials, Slope,
  Curvature, Height above water, Water distance and Road distance on
  terrain; Ground materials
  on an Ecotope's ground patch. A legend over the bottom-left of the
  view gives the colours, the cost per chunk and the value under the
  pointer. Scripts use `overlay-off` and `overlay-<label>` (such as
  `overlay-slope`, `overlay-ground-materials`).
  - Drawn by `procgena_terrain_bevy`'s `TerrainOverlay`: an image mixed
    over a chunk's (or patch's) terrain material, at bindings 109 and
    110 (GM2's texture arrays hold 105 to 108). The ground's own look
    is unchanged while no overlay is drawn.
  - Sampled on the CPU in the background per chunk, at the chunk's own
    tier, kept per chunk and family (regions, labels, ground, measures), so
    switching between measures only recolours. Dropped when the terrain
    changes. Code: `procgena/src/scene/overlay/`, `studio/overlays.rs`.
  - Cost per chunk, dev build, rolling hills, a busy machine (86
    chunks): regions 37 ms (65² region lookups); ground materials 213
    ms (33² sites painted); measures 398 ms (33² measure queries, shared
    by all five measures). Ecotope ground 0.3 ms (read from the patch's
    weights).
  - Landform colours each texel by `Terrain::annotate`'s landform, read
    at the chunk's tier; the readout adds its flags. Its cost per chunk is shown in the
    legend; it was not measured here.
  - Not yet: Biome types per region (once the World drives terrain);
    region ids drawn in the view (today only in the readout).
  - **Follow-ups:**
    1. Move the chunk Levels and Borders toggles from the terrain
       Inspector's Show section onto the view frame's overlays.
    2. A `TerrainStream` constructor that shares its `WorldTerrain`
       (`Arc`), so the studio doesn't repeat `TerrainStream::world`'s
       setup to give the overlay the same world.
    3. Sample the Ecotope overlay at its patch weights' own size (0.5 m)
       rather than 33² texels (about 1.5 m).
- **Stats** (ST7a): triangles and draw calls (Bevy's render
  diagnostics), copies per Scatter, specimens per pool, build time per
  document, bake times and queue length from the capture service.

## Export, capture and commands

- **Export** (ST8): one Export action for every kind with a CPU path
  (today tree, stone, mushroom and dead wood; plant and clump after R3),
  replacing the per-tab export panels; glTF and OBJ with texture images.
  Scatters export as a whole when rule 11 lands for them.
- **Screenshot and turntable** (ST8): a screenshot of the viewport at a
  chosen size; a turntable as a PNG sequence of N frames around the
  subject, through the capture service.
- **Command palette and shortcuts** (ST8): one command registry (open,
  save, save as new, undo, redo, frame selection, toggle overlays,
  export, screenshot, …) behind a palette (`cmd+K`) and shortcuts
  (`cmd+Z`, `cmd+shift+Z`, `cmd+S`, `cmd+shift+S`, `F` to frame).
  Commands are also what the headless harness drives.

## Testing

The headless harness (`procgena --headless <script>`, `HeadlessStudio`)
stays the way each milestone proves itself. It needs, in ST0:

- `type TEXT` (today `key` takes only letters and a few named keys),
- `hover X Y` (tooltips),
- `select PATH` (select a document node and part by path, for scripts that
  don't click through the Outline),
- `command NAME` (run a palette command).

Each milestone's "done when" below names a headless script; screenshots
follow the bevy-screenshots skill.

## Coordinator ideas and additions

| Idea | Verdict | Where |
|---|---|---|
| Undo/redo and unsaved-change state | Adopt, early (user) | ST2 |
| Differs from base / reset to base | Adopt, early (user) | ST2 |
| A/B compare of two presets or seeds | Later (user) | ST9 |
| Seed browser (step, randomise, pin) | Later (user); stepping the seed stays in the Inspector header from ST1 | ST9 |
| Debug overlays and live stats | Adopt, early (user) | ST7a |
| Problems panel | Adopt, early (user) | ST7a |
| "Used by" | Adopt (falls out of the registry) | ST2, shown in ST3 |
| Project folder, git-friendly | Adopt (decided) | ST2 |
| glTF export, screenshot and turntable | Adopt, early (user) | ST8 |
| Command palette and shortcuts | Adopt, early (user) | ST8 |
| Tooltips from the metadata | Adopt | ST1 |
| **Added:** outside edits reload (git pull, hand edits) | Adopt | ST2 |
| **Added:** autosave of unsaved documents to app data, restored after a crash | Adopt | ST2 |
| **Added:** preview contexts (an asset alone, in a Scatter, in an Ecotope, on a Terrain) | Later | ST9 |
| **Added:** camera bookmarks per document | Adopt, small | ST7a |
| **Added:** budget badges (triangles per level against a target, bake memory) | Later, after IX sets targets | ST9 |
| **Added:** node-link diagram of the Outline | Later (decided) | ST9 |

## Stages

Every milestone ends with something usable, and names what it needs and
when it is done. Studio files are split so that lanes after ST0 add their
own folders.

### ST0: Studio shell (churn window, alone)

- **Covers:** the kind registry replacing `StudioTab`, `SceneMode`,
  `SceneStart`, `BuildState` and the `spawn_editors` match; documents
  (open, close, several at once) without saving changes yet; the new
  layout with Library and Outline as plain lists, the Inspector showing
  today's hand-written specs for the selected document, the view frame
  holding environment, camera and the detail dropdowns; headless `type`,
  `hover`, `select` and `command`.
- **Needs:** the Fruit and Flower tabs merged (I1b's studio lane), and no
  other worker in `procgena/src/`. Runs alone.
- **Done when:** every kind that had a tab opens from the Library list
  and edits as before; headless scripts open a tree, an Ecotope and the
  Terrain and take screenshots; the existing studio tests pass with their
  intent kept.

#### ST0 as built

- **Kinds** (`procgena/src/studio/kinds/`): a `Kind` per kind, in a file
  of its own, registered by one line in `kinds::register_all`. It names
  the kind (`id`, the preset file kind; `label`, the Library filter), its
  presets, how to spawn its editor from a preset key, what the scene shows
  for an editor (`show`), its detail levels, the Environment settings it
  hides and its status line. Fruit and flowers are two kinds.
- **Documents** (`studio/documents.rs`): `Document` entities (kind, key,
  name, origin, Inspector page, editor, camera view, detail choice),
  `Selection`, `OpenDocument`, `CloseDocument`, `DocumentEdited`. Editors
  of unselected documents carry `Dormant` and their pages are hidden.
- **Scene** (`procgena/src/scene/`): a `Subject` trait per kind of
  subject, each in its own module, replaces `SceneMode`, `SceneStart` and
  the per-kind commands. The host sends `ViewerCommand::Show` with the
  document's id; the same document updates in place, another replaces the
  subject. `SetDetail`, `SetEnvironment`, `ResetCamera` and `SetCamera`
  are shared. The scene reports `ViewerEvent::Building` when a build
  starts. `ViewerEvent` still has a variant per kind's build report.
- **Layout:** Library (kind chips and the chosen kind's presets) above the
  list of open documents, the view frame bar (Detail, camera controls)
  over the 3D view with the Environment settings folded over its corner,
  and the Inspector (the selected document's title and editor). The
  per-editor preset pickers are gone; the Library opens presets.
- **Commands and scripts** (`studio/commands.rs`): named commands
  (`reset-camera`, `close-document`, `next-document`,
  `previous-document`), and headless `type`, `hover`, `select`,
  `command`. ST8 builds the palette and shortcuts on this registry.

### ST1: Metadata and the generated Inspector

- **Covers:** `procgena_core::meta` and its drift helper; one
  `<kind>.meta.yml` per kind with a drift test in each crate (tree, tree
  Scatter, plant, plant Scatter, stone, clump, mushroom, dead wood, fruit,
  flower, Ecotope, Terrain); the Inspector built from metadata, with part
  chips, tooltips and "shown if"; today's `procgena/src/*/params.rs` specs
  deleted, keeping custom sections where needed.
- **Needs:** the core module first; then one worker per crate, each only
  adding a `meta.yml` and one test file in its own crate, so they run
  beside ST0. They still share crates with other lanes: the tree and
  fruit workers wait until fruit AC1 has left `tree/` and `fruit/`, the
  plant workers coordinate with the plant-align lane (R3), and the
  Ecotope worker with GM0 and B1 in `procgena_biome`. The studio
  switch-over runs after ST0.
- **Done when:** every crate's drift test passes; a headless script
  selects the tree's Leaves chip and finds only leaf settings, hovers one
  and shows its description; no hand-written spec remains.

#### ST1 as built

- **Core** (`procgena_core::meta`): `KindMeta` read from
  `<kind>.meta.yml`; `SettingPath` (get and set on the settings tree) and
  `Edit { path, old, new }`, shared with ST2; `*` patterns; the condition
  language with named conditions; revive values; `optional: <start>` for
  numbers that may be unset; groups that name a `custom` control; choice
  lists a crate supplies (`ChoiceLists`); the Inspector `layout`; and
  `assert_in_step` and `variants` for tests.
- **Per crate:** `presets/<kind>.meta.yml`, a `<KIND>_META` constant, a
  `meta_choices()` where the file names lists, and `tests/meta.rs`, for
  tree, tree Scatter, stone, clump, mushroom, dead wood, fruit, flower,
  terrain and World. Settings today's studio didn't show are `hidden`
  entries.
- **Studio** (`studio/inspector/`): a kind's panel edits `Settings<T>` (the
  file-form settings tree plus the typed options read back with
  `from_value`, about 70 µs per edit on a tree) laid out by `sections`
  from the metadata, on the existing `params` panel, which gained tooltips,
  a part filter, runtime choice labels and `is_final`. `spawn_meta_panel`
  adds part chips that set `Selection::part` and open the part's sections.
  Every user edit triggers `InspectorEdited { document, edits, is_final }`
  (nested panels, such as a tree Scatter's species rows, don't yet).
- **Still hand-written:** plant, plant Scatter and Ecotope (their metadata
  comes later), the environment, the export panels and studio-only view
  panels (dead wood's View and Cover, the Terrain's Show).
  The stone, dead wood and mushroom specs remain only for the Ecotope's
  embedded panels.

### ST2: Saving presets and the project folder

- **Covers:** the runtime registry with owned presets, user bases and
  resolvers for references (tree Scatter, Ecotope and Biome files); the
  project folder, `procgena.yml`, the default project; Save as new and
  Update, both writing whole files (decision 5); undo and redo, unsaved state,
  differs from base and reset; outside-edit reload; autosave; "used by".
- **Needs:** ST0 for the studio half. The core half has two parts with
  different costs:
  - **Additive** (beside ST0 and ST1): the registry type, resolvers
    passed to the tree Scatter, Ecotope and Biome file readers, and
    user bases read through a new function next to today's `read`.
  - **Workspace-wide** (a short churn window of its own): changing
    `Preset::from_parts`, `read` and the `catalogue!` macro from
    `&'static` presets to `PresetRef` touches every generator crate and
    rebuilds the workspace. It runs alone, batched with any other change
    to `procgena_core::preset` waiting at the time, and is skipped if the
    additive form proves enough. **Skipped:** the additive form was
    enough (2026-11-24).
- **Done when:** a headless script saves a built-in pine as
  `tall_pine`, makes `taller_pine` over it, puts it in a project
  Ecotope, updates `tall_pine` three ways (a changed value, a newly
  differing nested setting, a setting reset to its base) and finds each
  written as `base` plus differences, undoes and redoes, and reopens
  the project to the same result.

#### ST2 as built (2026-11-25)

- **Core:** `procgena_core::preset::Registry` (project layer, user bases,
  namespaced keys, `References` for the tree Scatter and Ecotope,
  `used_by`, `file_text`, `builtin_root`, `base_for`). No change to
  `Preset`, `read` or `catalogue!`'s `&'static` presets.
- **Project** (`studio/project/`): `procgena.yml` (format, `id` as the
  namespace, name, and the bundle manifest's optional fields), the
  default project in Documents, save new, update, free keys, reload on
  outside edits. `Presets` is the registry resource the studio reads.
- **Documents** (`studio/documents/`): the document holds its settings
  (`DocumentSettings`) with undo and redo as steps of `meta::Edit`, a
  drag being one step; unsaved state in the Inspector title; Cmd+Q asks
  while anything is unsaved. Each kind gives `Kind::settings` (read and
  load the editor's settings, and resolve a preset to them); kinds with
  a `Settings` panel use one generic hook. Plant and plant Scatter have
  none yet: their presets aren't in the registry.
- **Saving:** Save (project presets), Save as new (key dialog proposing a
  free key), Reset to base, from commands, Inspector header buttons and
  Cmd+S, Cmd+Shift+S, Cmd+Z, Cmd+Shift+Z. Files are written whole as
  `base` plus differences. Parts of the settings unchanged since opening
  or the last save keep their file text, references included (list items
  matched by `key`); nothing is matched against presets.
- **Pickers:** the Library, the Ecotope's stone, dead wood and mushroom
  pickers and tree Scatter species pickers (on their own and inside an
  Ecotope) list project presets, marked, before the built-in ones.
  Picking one records its reference (`PresetPicked`), written on save
  while the setting keeps that preset's settings.
- **Headless:** saving a built-in tree as new, updating and reopening it;
  every plain kind saving an unedited built-in as its base alone; a
  saved stone and a saved tree picked in the oak and maple forest,
  saved and still resolved after reopening; project Scatters and
  Ecotopes keeping their references when edited and saved.
- **Not done:** the per-setting "differs from base" mark and reset in the
  Inspector, the rename dialog for an existing preset, autosave,
  reloading open documents after outside edits, and the Problems panel.

### ST3: Library, Outline and selection

- **Covers:** the Library (grid and list, search, filters, recent and
  pinned, open, duplicate, rename with reference updates, delete to
  trash, reveal); the Outline with references and parts; the shared
  `Selection`; editing in context (opening a referenced preset from its
  parent).
- **Needs:** ST1 (parts and names), ST2 (registry, used by).
- **Done when:** a headless script types "pine" in the search, opens the
  pine forest floor Ecotope, expands `pines` in the Outline, selects
  `pine_small`'s Branches, and the Inspector shows only branch settings
  with the "used by" list naming the Ecotope.

### ST4: Capture service and thumbnails

- **Covers:** `procgena_core_bevy::capture`; the impostor bake moved
  onto it; thumbnail recipes per kind; the thumbnail cache; the headless
  `--render-thumbnails`; thumbnails in the Library and Outline.
- **Needs:** ST0. The crate half needs `procgena_impostor` free of the
  fruit D2 and IX lanes, or done in agreement with them; the studio half
  needs ST3.
- **Done when:** impostor bakes match their pictures from before the
  move (the impostor crate's tests, and a side-by-side shot); the Library
  shows a thumbnail for every preset kind, a changed preset gets a new
  one, and a scroll through the Library never drops a frame below the
  budget.

### ST5: Picking parts in the viewport

- **ST5a (by tags):** CPU ray picks for single assets and for Scatter
  and Ecotope copies, hover tooltips, part highlight, click to select.
  Needs ST3 (and R4's ray casts for Scatters other than trees). Done when
  a headless script clicks a leaf of a single tree and the Inspector
  shows the leaf settings, and clicks a log in the Ecotope and gets the
  log preset's settings.
- **ST5b (by part paths):** D3 part paths for highlighting single parts
  and showing addresses. Needs X1's `part()` for tree, fruit's AC1. Done
  when clicking one apple shows its address `… / fruit i / item k` and
  highlights that apple only.

### ST6: Texture library

- **Covers:** `TextureCatalogue` (unless GM2 lands it first) and the
  generator crates moving from const kind lists to it; the project as an
  asset source; import with `ctt`; the texture view in the Library; the
  texture picker in the Inspector; the Python tool reading project
  catalogues.
- **Needs:** ST2 (project), ST3 (Library), ST4 (swatches). Agreement with
  the GM2 worker on the catalogue type. The generator-crate changes touch
  every crate with textures, so they run as one worker, not beside other
  writers in those crates.
- **Done when:** a headless script imports three images as a bark set,
  picks it on a tree, sees it drawn, saves, and the project reopens with
  the bark; the catalogue lists built-in bark, leaf, rock and ground sets
  together.

### ST7: View frame, overlays, stats and problems

- **ST7a:** the Problems panel; overlays as far as each renderer offers
  them; stats; camera bookmarks; one Detail control for every kind; the
  Quality control. Needs ST0, ST1. Done when a headless script breaks a
  setting and the Problems panel lists it and selects it on click, and
  the LOD overlay colours a tree Scatter by level. (Until IX's API
  lands, the Detail control forces levels as today's dropdowns do.)
- **ST7b:** the viewing-distance control, the game default and level 0
  preview in the Detail control, the level readout with its reason, the
  "level 0 only" marks in the Inspector, and the game profile in the
  project. Needs IX's per-view override and its default-level API. Done
  when a headless shot of a tree 5 m away with viewing distance 300 m
  shows its impostor, and the readout says so; and when a tree not opted
  into level 0 reads "level 1 · game default" up close, switches to
  level 0 under Level 0 preview with the preview mark shown, and reads
  "level 0 · opted in by this preset" once its preset opts in.

### ST8: Export, capture and commands

- **Covers:** one Export action, screenshot and turntable, the command
  registry, palette and shortcuts.
- **Needs:** ST0; turntables need ST4.
- **Done when:** a headless script exports the open tree to glTF from
  the palette, and a turntable writes N frames.

### ST9: Later

A/B compare in a split viewport; the seed browser; the Outline as a
node-link diagram; preview contexts; budget badges; Biome and World
documents as B1 and W1 bring their types (registered as new kinds,
not new tabs).

Far future, with no milestone: the studio as an extension of the Bevy
editor, once Bevy offers editor extensions (see "Bevy editor").

### Running them in parallel

| Wave | Run together | Notes |
|---|---|---|
| a | ST0; `procgena_core::meta` → per-crate `meta.yml` workers; ST2's additive registry in `procgena_core`, then the tree Scatter, Ecotope and Biome resolvers | ST0 alone in `procgena/src/`. Meta workers each stay in their crate, and wait for or agree with the lanes named in ST1 (fruit AC1 in `tree/`, R3 in `plant/`, GM0 and B1 in `procgena_biome`). The registry worker touches `core/preset` and the file readers, so it is not beside other writers of `scatter/file.rs` or `ecotope/file.rs`. |
| a′ | ST2's workspace-wide `Preset` change, if needed | Alone: it rebuilds the workspace. |
| b | ST1 studio switch-over; ST2 studio half; ST4 crate half | ST1 owns `studio/inspector/`, ST2 `studio/documents/` and `studio/project/`. ST4 is in `procgena_core_bevy` and `procgena_impostor` (with the fruit D2 and IX lanes' agreement). |
| c | ST3; ST7a; ST8 (without turntable) | `studio/library/` and `studio/outline/`; `studio/problems/` and `view_frame/`; `studio/commands/`. All three register through ST0's registry and read `Selection`. |
| d | ST4 studio half; ST5a; ST6 | ST6's crate changes run as one worker. |
| later | ST5b (after X1 and fruit AC1); ST7b (after IX's override); ST8 turntable; ST9 | |

Every lane registers through `studio/kinds/` and reads `Selection`, both
defined in ST0; changes to those two are coordinated, not made in
passing. Builds stay scoped: `cargo clippy -p procgena --all-targets
--no-deps` and headless scripts; GPU tests only where a lane touched
drawing.

## Bevy editor

Researched 2026-11-24 at the user's request, to see whether the studio
should also become a plugin to the Bevy editor.

**Decided by the user (2026-11-24): not now.** The official editor has
no extension API, and its design book says adding editor functionality
is, for now, done by forking. The studio is planned as a standalone app
only. Becoming an extension of the Bevy editor is a **far-future goal,
taken up once Bevy offers editor extensions**. No milestone, crate split
or host interface is planned for it, and ST0–ST9 are unchanged.

### What exists (November 2026)

| Project | What it is | Architecture | Status |
|---|---|---|---|
| Official Bevy editor ([design book](https://bevyengine.github.io/bevy_editor_prototypes/architecture.html), [roadmap](https://bevyengine.github.io/bevy_editor_prototypes/roadmap.html), [constraints](https://bevyengine.github.io/bevy_editor_prototypes/design-constraints.html), [prototypes repo](https://github.com/bevyengine/bevy_editor_prototypes)) | A standalone Bevy app built only on `bevy_ui` and feathers; it edits `.bsn` scenes. | Self-contained tools (asset browser, entity inspector) are meant to be plugins usable without the editor. Roadmap stages: read-only scene viewer, basic editing, undo and hot reload, design pass, end-to-end projects (first alpha, moving into the Bevy repo), customisation. | The prototypes repo was archived 2026-04-16; work continues in the main repo. Open: undo design, extensions ("forking" for now), talking to a running game, distribution, preferences. |
| Bevy 0.19 and 0.20 groundwork ([0.20](https://bevy.org/news/bevy-0-20/), [0.19](https://bevy.org/news/bevy-0-19/)) | Pieces the editor will be built from. | BSN syntax "largely nailed down", `.bsn` asset files coming; feathers (text, number, colour, dropdown, list, tabs, contextual theming) called "editor-centric"; headless `bevy_ui_widgets`; `PanOrbitCamera` upstreamed; App Settings saved through `Reflect`. | "What's next" lists `.bsn` assets, assets as entities and a remote inspector. No editor date. |
| [Discussion #24010](https://github.com/bevyengine/bevy/discussions/24010) | A proposal (2026-04-28) for a thin editor host with WebAssembly (WIT) extensions. | Reflection across the boundary and how extensions contribute UI are its hard problems. | One reply, no maintainer response, no conclusion. |
| [jackdaw](https://github.com/jbuehler23/jackdaw) ([docs.rs](https://docs.rs/crate/jackdaw/latest), [API](https://docs.rs/jackdaw_api/latest/jackdaw_api/)) | Community scene editor that the archived repo points to for experiments; 0.20.2 on Bevy 0.20 (2026-11-10). | A plugin group plus a binary. `JackdawExtension` adds docked windows, menus and inspector categories. Undo goes through "operators", which group into history spans. Selection is a resource plus a `Selected` marker. It uses `.bsn` scenes and its own feathers fork. | Active; undo described as still buggy. Not an official Bevy project. |
| [bevy-inspector-egui](https://github.com/jakobhellermann/bevy-inspector-egui) | `Reflect`-driven inspector plugins. | `#[derive(InspectorOptions)]` with `#[inspector(min, max)]` for ranges; egui. No selection or undo. | 0.38 on Bevy 0.20, maintained. |
| [Yoleck](https://github.com/idanarye/bevy-yoleck) | Kit to build your own level editor into the game binary. | Type handlers per entity type; UI generated from `Reflect` and attributes since 0.30; egui; `.yol` JSON levels. | 0.35 (2026-10-31) on Bevy 0.20, adds BSN. |
| [Space Editor](https://github.com/rewin123/space_editor) | Prefab and scene editor. | egui with docking; its own undo crate; inspector via bevy-inspector-egui; RON scenes. | Stable branch on Bevy 0.14. |
| [Blenvy](https://github.com/kaosat-dev/Blenvy), [Skein](https://bevy-skein.netlify.app) ([reflection](https://bevy-skein.netlify.app/docs/reflection)) | Blender as the editor. | Both read Bevy's `Reflect` type registry (Skein over the remote protocol) and store components in glTF extras. | Blenvy alpha; Skein 0.5 on Bevy 0.18. |

### What it means for this plan

- **Nothing changes now.** The studio already uses feathers,
  `bevy_ui_widgets` and BSN scene functions, which is what any future
  Bevy editor extension would be built from.
- **Themes to recheck when extensions arrive:** a `Reflect`-driven
  inspector (every editor above builds inspectors from `Reflect`, so
  procgena options may need an optional `Reflect` derive), presets as
  Bevy assets for an asset browser and `.bsn` scenes, the host's own undo
  and selection, and which studio parts are editor-agnostic (the kind
  registry, metadata, preset registry, capture service and project folder
  are the likely ones).
- **Worth adopting on its own merit (proposed, not part of this
  decision):** Bevy's `PanOrbitCamera` in place of the studio's own orbit
  camera, and Bevy's App Settings for the studio's session state.

## Effects on other documents

- **ARCHITECTURE.md:** the "Studio" TODO ("a tab per new level") is
  reworded: each level is a kind in the studio's registry, opened from
  the Library and shown in the Outline. Rule 10 gains the metadata file
  and the runtime registry (project presets over built-in catalogues,
  references resolved through it) when ST1 and ST2 land.
- **ROADMAP.md:** the Studio track (ST0–ST9), with IX gaining the
  per-view override as a studio need and GM2 sharing the texture
  catalogue.
- **IX plan (`docs/decisions/IX-plan.md`, not written yet):** must cover
  "Viewing distance (requirement on IX)" above.
- **ground-materials.md:** GM2's texture list is a source of the shared
  `TextureCatalogue`, not a list of its own.
- **fruit/plan.md:** the Fruit and Flower tabs become kinds at ST0.

## Open questions

1. **Key clashes.** *Decided by the user, 2026-11-24:* namespaced keys.
   The namespaces are `builtin` (reserved) and the project's or bundle's
   id, as in `builtin:pine_small` or `my_forest:pine_small`. Bare keys
   work when only one layer has them; a shared key is a problem fixed by
   writing the namespace, and the rename dialog is a convenience.
2. **Metadata language.** Descriptions are English only for now. If the
   studio is ever translated, the metadata files are where the strings
   live; no i18n work is planned.
3. **Enum coverage in the drift test.** Listing variant settings by name
   in the metadata (proposed) versus each crate supplying one sample value
   per variant to the test.
4. **Built-in thumbnails in git.** Shipping pre-rendered built-in
   thumbnails puts binary files in the repository (proposed: in the
   studio's assets, re-rendered by the headless command when presets
   change). Alternative: render them on first run into the user's cache.
5. **Where the capture module lives.** `procgena_core_bevy` (proposed) or
   a new `procgena_capture` crate, or inside `procgena_impostor` with the
   bake on top.
6. **Biome climate on the view frame.** When a Biome owns its sun and
   wind, should previewing inside a Biome lock the environment to it, or
   keep the view frame's environment with a "use Biome's" switch?
7. **Fruit and flower editing in context.** Opening the fruit preset from
   a tree's Fruit part is decided; whether the tree's Fruit chip also
   shows the tree's own placement settings (where on the branches fruit
   grows) beside a link to the fruit preset needs the fruit tab's design
   (I1b) to settle.
8. **The game profile.** Whether the per-game level-0 opt-in and quality
   belong in the project's `procgena.yml` (proposed) or in a separate
   game settings file a project can hold several of (for example one per
   target machine); settled with IX's plan.
