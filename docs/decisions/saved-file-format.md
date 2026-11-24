# Saved-file format: preset bundles

Status: **decided.** The user decided every question and the container on
2026-11-24 (see "Decisions" and "Questions for the user"). The zip library
is chosen when PB1 is built. No code was changed or run.

Written from ARCHITECTURE.md ("Seeds", rule 10 "One preset and file
pattern", "Bringing your own assets"),
[studio_plan.md](../../procgena/studio_plan.md) ("Preset registry and the
project folder", "Documents and the edit model", "The texture library",
"Stages", open question 1), [ground-materials](ground-materials.md)
(texture sizes, level 1 as the default), `procgena_core::preset`,
`procgena_core::seed`, `plant/src/files.rs`, `procgena_tree/src/scatter.rs`
and `scatter/file.rs`, `procgena_biome/src/ecotope/file.rs`,
`tree/textures.yml`, the `bevy_asset` 0.20.1 source, `Cargo.lock`,
crates.io (2026-11-24), and the sources linked under "Why zip".

One correction to the first brief: plant no longer has `.plant.ron` or
`.field.ron` files. `plant/src/files.rs` reads `.plant.yml` and
`.plant_scatter.yml` through `procgena_core::preset`, like every other
kind.

## Decisions

Made by the user on 2026-11-24. The numbers are the first round's
question numbers.

1. **The folder is the working form; a `.procgena` file is for sharing**
   (Q1 a). The studio saves into the project folder.
   - **Export takes a subset.** Pick, for example, a Biome. Export
     gathers everything it depends on (presets, textures, models) by
     following `base` and references, and writes a `.procgena` with just
     that.
   - **Two ways in.** "Open" opens a bundle as a project. "Import" copies
     its contents into your own project. Both are designed below
     ("Export, Open and Import").
2. **Games keep today's loading; bundles are for the studio only for
   now** (Q2 deferred). The `procgena://` mount source and the `#label`
   loader are later options, not in the build plan.
3. **Key clashes: qualify only when ambiguous** (Q3 a). With bundles not
   mounted beside a project, this now governs clashes between project
   presets and built-ins (studio_plan.md open question 1), and Import
   conflicts.
4. **No entry list in the manifest** (Q4 a).
5. **Export has a checkbox to flatten built-in bases,** off by default:
   files keep `base` (Q5 c).
6. **Bundles may carry one small thumbnail per preset,** optionally (Q6 b).
7. **`.yml` only.** JSON content inside a `.yml` file still reads (Q7 a).
8. **BC7 KTX2 only** for textures (Q8 a).
9. **The plant web demo is deferred,** and with it the wasm check (Q9).
10. **The container is zip, with every entry stored uncompressed, written
    with a strict profile** (Q10). The profile is specified under "The
    bundle zip profile". The reader accepts only that profile for
    bundles, with clear errors.
11. **The zip library is chosen when it is built** (Q10, nested): the
    `zip` crate, `rawzip`, or our own reader and writer. The candidates
    and their facts are under "Zip code: chosen at PB1".
12. **The extension is `.procgena`** (Q11).

## Bottom line

- **A bundle is a subset of a project folder, packed into one zip file.**
  The layout inside is the project folder's (`procgena.yml`,
  `presets/<kind>/<key>.<kind>.yml`, `textures/`), so packing and
  unpacking are copies, and the folder stays the git-friendly working
  form.
- **The bundle zip profile** keeps every bundle easy to read lazily:
  - every entry is stored uncompressed, so data is never inflated;
  - `procgena.yml` comes first, then the YAML, thumbnails, textures and
    models;
  - every entry's data is 64-byte aligned, through a padding record that
    is the same in the local header and the central directory, so a range
    reader finds where data starts from the central directory alone;
  - there is no zip64, no data descriptor, no comment, no encryption, no
    links, and timestamps are fixed.

  Every bundle the studio writes opens by double-click on every OS.
- **The reader accepts only the profile** (proposed). A zip made by
  Finder or Explorer is compressed, so it is rejected with a message
  saying to unzip it and open the folder (question 3).
- **Export gathers a closure; Open unpacks; Import copies with
  renames.** Export follows `base`, preset references, texture ids and
  model paths from the chosen roots and copies everything but built-ins.
  "Open" unpacks into a new project folder (proposed; question 1).
  "Import" copies into the current project. It skips identical items and,
  by default, renames conflicting ones, rewriting `base:`, references and
  texture ids to match. It writes an explicit tree Scatter `key` where a
  rename would otherwise change a seed.
- **The manifest is the project's `procgena.yml`**, with `format: 1`, an
  id, name, version, licence, authors and `made_with`, and no entry list.
- **The reader is strict and safe on untrusted files** (see "Size and
  security"). Generated meshes are never stored.
- **No new dependency is added by this plan.** With the strict profile,
  our own reader and writer need only `crc32fast`, which is already in the
  tree. Whoever builds PB1 chooses between that, `zip` and `rawzip`.

## Why zip

This section is kept as the record of the decision.

- **Lazy reading** means listing a bundle's presets and reading one
  without reading the whole file. On a local file, which is how the
  studio uses bundles, any format with an index costs one or two seeks.
- **Round trips** matter later, for HTTP range requests.
  - A suffix range (`Range: bytes=-N`) fetches the end of a file without
    knowing its size. RFC 9110 defines it, and
    [Cloudflare documents it](https://developers.cloudflare.com/cache/reference/range-requests/).
  - Without suffix ranges, a front range comes first, and its
    `Content-Range` gives the size.
  - Requests that don't depend on each other go out in parallel, so the
    table counts sequential trips.
- **Compression doesn't decide it.** The textures are BC7 KTX2, already
  supercompressed with Zstandard. A bundle of 20 presets and one 512 px
  texture set holds about 50 KiB of YAML beside about 1 MiB of KTX2.
  Deflating the YAML would save 35–40 KiB.

| Format | Trips to the index (suffix ranges work / don't) | Trips to one entry | Tooling | Why not chosen |
|---|---|---|---|---|
| **Zip, stored, our profile** | 1 / 2 | 1, exact under the profile | Opens by double-click on Windows, macOS and Linux; `unzip`, `7z` | **Chosen** |
| tar | No index: one trip per header walked | k trips for the k-th entry | Linux, macOS; recent Windows (from memory) | No index. An end TOC ([eStargz](https://github.com/containerd/stargz-snapshotter/blob/2713db58dc69454d4ed5cffdd277560e5c52aa25/docs/estargz.md)) would rebuild zip's central directory, with worse tools. |
| [ASAR](https://github.com/electron/asar) | 1 / 1 (index at the front) | 1 | `npx @electron/asar`; no OS support | Electron quirks (Pickle sizes, offsets as strings); no OS tools |
| Custom, index at the front, like [PMTiles](https://gist.github.com/DerZade/cda63bccd24b3999b04e429955f61bfb) | 1 / 1 | 1 | Only the tools we would write | Saves a trip only on servers without suffix ranges, or that ignore ranges altogether; costs every OS tool, plus a format of our own to keep stable |
| SQLite [`sqlar`](https://sqlite.org/sqlar.html) over an HTTP VFS ([sql.js-httpvfs](https://npmjs.com/package/sql.js-httpvfs)) | 1, then the B-tree depth (2–3) | Large blobs follow a chain of overflow pages, each found from the one before (from memory) | `sqlite3 -A`, DB browsers | Slow blob reads over ranges; a bundled C library; in-place update, its strength, isn't needed for files written once |
| [7z](https://py7zr.readthedocs.io/en/latest/archive_format.html) | 2 / 2 (a start header points to a header at the end, often compressed) | 1, or the whole solid block | 7-Zip; recent Windows (from memory) | Built for solid compression; complex |

Prior art for zip over ranges:
- [WACZ](https://specs.webrecorder.net/wacz/1.2.0/) is a stored zip of web
  archives, which ReplayWeb.page reads lazily in the browser with range
  requests.
- [USDZ](https://openusd.org/dev/spec_usdz.html) is a stored zip with every
  entry 64-byte aligned.
- [`.conda`](https://prefix-dev.github.io/rattler-book/deep-dive-package-format)
  packages are an uncompressed outer zip, so the metadata can be read
  without decompressing the payload.

## The bundle zip profile

Terms follow PKWARE's zip specification (APPNOTE): each entry has a
**local header** before its data and a **central directory record** in
the index at the end, which closes with the **end of central directory
record** (EOCD).

### What the writer writes

| Item | Rule |
|---|---|
| Compression | Every entry is STORED (method 0): compressed size equals uncompressed size. |
| Order | `procgena.yml` first, then every other `.yml` sorted by path, then `thumbnails/`, then textures and models sorted by path. A front fetch therefore holds the manifest and the presets. |
| Layout | The first local header at offset 0. Each local header follows the previous entry's data directly. The central directory follows the last entry's data. The EOCD is the last 22 bytes, as there is no comment. No other bytes anywhere. |
| Names | UTF-8, with general-purpose flag bit 11 set. The path rules under "Size and security" make names ASCII in practice. Folders are implicit: no folder entries. |
| Flags | Only bit 11. No bit 3 (data descriptor), no bit 0 (encryption). |
| CRC-32 and sizes | Computed before the local header is written, so local and central values agree. There are no data descriptors. |
| zip64 | **Never written; rejected on read.** The size limits keep every bundle under 2 GiB and 10 000 entries, well inside 32-bit zip's 4 GiB and 65 535. The writer refuses a bundle over the limits, naming the entry or the total. |
| Extra field | **Exactly one padding record, the same bytes in the local header and the central directory record:** a 2-byte id, a 2-byte length, then 0–63 zero bytes. Its length is chosen so the entry's data starts on a 64-byte boundary. Android's zipalign uses id `0xD935` for its alignment record (from memory). PB1 fixes our id, checked against the ids APPNOTE lists, or reuses zipalign's if that one is confirmed. |
| Data start | `local header offset + 30 + name length + extra length`, all read from the central directory. A range reader needs no local header to find an entry's bytes, and fetches exactly `offset .. offset + size`. |
| Alignment | **64 bytes** (see below). |
| Timestamps | Fixed: DOS date 1980-01-01, time 00:00. The same input gives the same bytes. |
| Attributes | Version made by: Unix. External attributes: a regular file, mode `0644`, so `unzip` sets sane permissions. No symlinks, devices or folders. Internal attributes: 0. |
| Version needed to extract | 1.0 (stored). Whether every tool accepts 1.0 with the UTF-8 flag set is checked in PB1's interop test. |
| Comments | None: no archive comment, no file comments. |
| Multi-disk | No: every disk number is 0. |

**Why the padding record sits in both headers.** The request was for an
empty or fixed-size local extra field. That can't hold together with
alignment. An empty field leaves data wherever the name ends. A
fixed-size field shifts every entry by the same amount, so the padding
still has to vary per entry. USDZ puts the varying padding in the local
extra field. Copying the same record into the central directory record
keeps what the fixed size was for: data start computed from the central
directory alone. The reader checks that the two copies match.
Padding between entries instead, outside any header, would leave bytes
that the zip specification doesn't describe. That relies on every tool
skipping them, so it is not used.

**Why 64 bytes.**
- **What it buys.** A stored entry can be used in place from an mmap
  or a fetched buffer, cast as any type up to 64-byte vectors: GLB
  buffers need 4-byte alignment, KTX2 level data 8 or 16, AVX-512 64.
  Every entry also starts on a cache line.
- **Precedent.** USDZ chose 64 for the same reasons.
- **What it costs.** About 36 bytes per entry on average (the 4-byte record
  plus 32 bytes of padding), or about 3.5 KiB for 100 entries.
- **Rejected:**
  - 4 bytes (zipalign) is too small for 8-byte and SIMD reads.
  - 4096 bytes (a page) would allow mapping each entry directly. It costs
    about 2 KiB of padding per entry, so a 1 KiB preset file would take
    three times its size.
- **The honest note.** Today's textures are Zstandard-supercompressed and
  decoded on load, so nothing uses the alignment yet. It is cheap, and it
  can't be added later without rewriting every bundle.

### What the reader accepts

For `.procgena` files the reader accepts **exactly the profile** and
checks:
- the EOCD is the last 22 bytes, with no comment and no zip64 records;
- the central directory sits right before it and fills the space up to it;
- every central directory record matches its local header byte for byte
  in method, flags, CRC, sizes, name and extra field;
- entries follow one another with no gaps or overlaps, starting at 0;
- every data offset is a multiple of 64;
- every CRC matches when an entry is read whole.

Errors name the entry and the rule it breaks, and say what to do, for
example:

> `textures/bark/alpine_bark/color_128.ktx2` is compressed (DEFLATE).
> `.procgena` bundles store every entry uncompressed. If you zipped a
> folder yourself, unzip it and open the folder as a project, or export
> a bundle from the studio.

**Zips made by Finder or Explorer** compress their entries, add data
descriptors, `__MACOSX/` folders and wrapping folders, so they never fit
the profile. Two policies:
- **Reject them** (proposed). Every `.procgena` is then range-readable and
  free of decompression bombs. The reader needs no DEFLATE, no `flate2`
  and no bomb limits. Someone with a hand-made zip unzips it and opens the
  folder: a project folder is the working form anyway (decision 1).
- **Accept DEFLATE through a second, lenient path** that inflates within
  bomb limits. Hand-made zips then open, at the cost of a second reader,
  `flate2`, limits, and bundles that range readers can't read in place.

This is question 3. A future `procgena bundle pack <folder>` command
(not planned yet) would make bundles without the studio.

**The folder form** follows the same path rules. It skips `.DS_Store`,
`Thumbs.db` and `__MACOSX/`, which unzipping a hand-made zip leaves
behind.

### Zip code: chosen at PB1

The user decided this is chosen when PB1 is built. Facts for whoever
builds it:

| Option | What it brings | What the strict profile changes |
|---|---|---|
| **Our own reader and writer** | No new crate: `crc32fast` is already in the tree (through `flate2` and `png`). Estimated at about 150 lines for the writer and 250 for the reader, plus tests. | The profile removes zip's hard parts: compression, zip64, data descriptors, comment scanning, multi-disk. The reader's bulk is profile validation, which we need whatever parses the bytes. The writer controls the padding record exactly. Interop and fuzzing are ours. |
| **`rawzip`** 0.5.3 (MIT, 0.9 M downloads, no dependencies) | A low-level reader and writer that exposes raw records | It could do the parsing under our checks. Whether its writer can mirror a custom extra field into the central directory needs checking. It is a new crate in `Cargo.lock`, and young (0.x). |
| **`zip`** 9.8.2 (MIT, 361 M downloads) | The most used Rust zip crate. Use it with `default-features = false`, since the defaults pull in AES, bzip2, LZMA, xz, PPMd and zstd. | Its reader accepts far more than the profile, so we validate on top anyway. `with_alignment` appears in its 2.x docs, but whether it pads the same way in the central directory is unconfirmed. Many required dependencies. It had a symlink advisory, fixed in 2.3.0 ([SNYK-RUST-ZIP-9460813](https://security.snyk.io/vuln/SNYK-RUST-ZIP-9460813)). |

**Leaning,** for the builder to confirm: our own code. The strict profile
makes it small, keeps the dependency list as it is, and gives exact
control of the layout. Whatever is chosen, PB1 needs an **interop test**:
`unzip -t`, Python's `zipfile`, macOS Archive Utility and Windows Explorer
must all open a bundle the writer made. The tools folder already runs
Python through `uv`.

## Inside the container

### Layout

```
alpine_pack.procgena            (or the folder it unpacks to)
  procgena.yml                  # first entry: the manifest
  README.md                     # optional
  LICENSE                       # optional; the licence named in procgena.yml
  presets/
    biome/       alpine.biome.yml
    ecotope/     alpine_floor.ecotope.yml
    tree_scatter/ pine_belt.tree_scatter.yml
    tree/        tall_pine.tree.yml
    flower/      edelweiss.flower.yml
  thumbnails/                   # optional (decision 6), about 256 px
    tree/        tall_pine.png
  textures/
    textures.yml                # the bundle's texture catalogue entries
    bark/alpine_bark/color_128.ktx2 …
  models/
    cairn.glb                   # your own meshes, for MeshScatter (later)
```

- **The same tree as the project folder** in studio_plan.md, holding only
  what Export gathered. `.cache/`, `.gitignore`, `.git/` and texture
  source images are never packed.
- **Presets are today's F3 files, unchanged.** The key comes from the
  file name; the kind comes from the folder and the extension, and a
  mismatch is a problem.
- **Kinds come from paths, so a new kind needs no format change.**
  `.flower.yml`, which ST0 is splitting out of fruit, is simply
  `presets/flower/`. A kind folder this procgena doesn't know is reported
  and skipped; the rest of the bundle still opens.
- **Allowed file types:** `.yml`, `.ktx2`, `.glb`, `.png` (only under
  `thumbnails/`), `README.md` and `LICENSE`. Anything else is reported
  and left unread.

### The manifest: `procgena.yml`

```yaml
format: 1                       # the bundle format; anything else is rejected
id: alpine_pack                 # lower case, digits, underscores
name: Alpine pack
version: 0.3.0                  # the author's version; free text
description: Pines, edelweiss and a scree floor for alpine Biomes.
licence: CC0-1.0                # SPDX id; required to export
authors: [Jo Example]
made_with: 0.1.0                # procgena version that exported it; for messages only
roots: [biome/alpine]           # what the author picked; the rest came with them
```

- **Preset kinds** come from the paths. **The entry list** is the zip's
  central directory (decision 4). **Hashes:** each entry's CRC-32 is
  checked on read; content hashes for caches and for Import's "identical"
  test are computed on load with `blake3`, already in the tree through
  `bevy_asset`.
- **`roots`** (proposed, new) records what the author picked, so the
  studio can open the bundle on its main preset and Import can offer
  "just the roots".
- **`requires`** (bundles depending on other bundles) is dropped for now.
  Export copies everything a bundle needs except built-ins, so each
  bundle stands alone. It returns with mounting (see "Later").
- The project folder's `procgena.yml` gains `id`, `version`, `licence`
  and `authors` too, so Export can fill the manifest from it.

## Export, Open and Import

### Export a subset

1. **Pick the roots** in the Library: one or more presets of any kind,
   such as a Biome.
2. **Gather the closure** by following:
   - `base` chains, through project presets only;
   - preset references in Scatter, Ecotope, Biome and Terrain files, and
     attached assets (a tree's fruit and flowers);
   - texture ids;
   - model paths.

   ST1's metadata `ref` and `texture:` fields name where references sit,
   and ST2's "used by" index already follows them. **Built-in presets
   and built-in textures are not copied:** every procgena has them.
   Presets from an earlier Import are ordinary project presets by then,
   so they are copied.
3. **Review** in a dialog:
   - the gathered list, with each item's size and why it was included;
   - the texture sizes to include (128 px always, 512, 1k and 2k
     optional);
   - "Flatten built-in bases" (off, decision 5) and "Include thumbnails";
   - the manifest fields, with the licence required, beside the licences
     of the textures that are included.
4. **Write** a `.procgena` file with the profile. The same input gives
   the same bytes.

### Open a bundle as a project

Opening means editing, and the studio edits only folders (decision 1).
So **Open unpacks into a new project folder and opens it** (proposed):

- It asks where, defaulting to `Documents/procgena/<id>/`, and refuses a
  folder that isn't empty.
- The project's `procgena.yml` is the bundle's manifest, plus the
  project-only fields.
- The `.procgena` file is not touched again. Saving writes the folder,
  and re-exporting makes a new bundle.

The alternative, a read-only view straight from the file with "Unpack to
edit", needs the studio to read presets and textures from inside a zip,
which only the deferred mount source would give. That is question 1.

### Import into the current project

1. **Choose** what to import: all, the bundle's `roots`, or a pick; the
   closure inside the bundle comes along.
2. **Plan each item** against the project and the built-ins:
   - **New key:** copied.
   - **Same key, identical content:** skipped, and the project's copy is
     used. Presets are compared as settings trees after parsing, so
     comments and layout don't count. Textures are compared by `blake3`.
   - **Same key, different content, or a built-in's key:** a conflict.
     Under decision 3, a project preset with a built-in's key is a
     problem to qualify, so Import avoids creating one. The qualifier's
     syntax is ST2's to fix along with studio_plan.md's open question 1;
     `builtin:<key>` is proposed. Each conflict offers:
     - **Rename.** The default name is `<key>_<bundle id>`, which can be
       edited. Like every preset key, the new name must be a valid child
       key: `[a-z][a-z0-9_]*`, at most 32 bytes (`procgena_core::seed`).
       So it is shortened and numbered (`_2`, `_3`) when needed.
     - **Use mine:** skip it and point the imported files at the
       project's copy.
     - **Replace:** overwrite the project's copy, showing its "used by"
       first.
3. **Rewrite references to renamed items** inside the imported files:
   `base:`, preset keys in Scatter, Ecotope and Biome files, and texture
   ids. Edits go through `yamlpatch`, as ST2's Update does, so the
   bundle's comments survive.
4. **Keep seeds.** A tree Scatter species with no `key` takes its tree
   preset's key as its child key, and the child key salts its seed
   (`procgena_tree/src/scatter/file.rs`, `scatter.rs`; ARCHITECTURE.md
   "Seeds"). When Import renames that tree, it writes `key: <old key>`
   into the species entry, so the Scatter places exactly as it did in the
   bundle.
   - **A new kind of edit.** Writing `key:` into an item of a list isn't
     covered by ST2's `yamlpatch` spike (replace, insert a nested key,
     remove), so PB3 starts with a spike of its own.
   - **Only the tree Scatter needs this.** Every `ChildKey::new` call site
     was checked; the others build keys from map keys, fixed stems or
     numbered names:
     - Ecotope Scatters and ground layers are keyed by their map keys;
     - stone classes name their own `key`;
     - dead wood and mushroom references resolve to options.

     A reader that adds such a default later must be added to this step.
5. **Write** new files whole, each with a first comment line
   `# Imported from alpine_pack 0.3.0` (proposed). Show a summary, then
   reload the registry.

## Loading

### Engine-free: `procgena_bundle`

A new engine-free crate, `core/crates/procgena_bundle`:

- `Bundle::open(path)` reads a `.procgena` file or a folder;
  `Bundle::from_bytes` reads one in memory. Both read the index once,
  check the profile, every path and every limit, read `procgena.yml`, and
  list presets by kind and key. No other entry is read yet.
- `bundle.read(path)` reads one entry: a slice, checked against its CRC.
- `export(project, roots, options, out)`, `unpack(bundle, dir)` and
  `plan_import` / `apply_import` hold the logic above, so the studio and a
  future CLI share it.

It stays out of `procgena_core`, so that only crates that read bundles
build the container code; `procgena_core` rebuilds most of the workspace.
The closure walk needs the registry's reference index (ST2), so this
crate depends on `procgena_core::preset` and receives the per-kind
reference finders from the studio's kind registry.

### Bevy and games: later

Decision 2 keeps games on today's loading, and Open and Import turn
bundles into ordinary project files, so the studio needs no Bevy reader
for bundles. Two options are kept for later, with facts from the
`bevy_asset` 0.20.1 source:

- **A mount source,** `procgena://<id>/<path>`. Asset sources are fixed
  when `AssetPlugin` is built (`AssetApp::register_asset_source`), so a
  single source must accept bundles mounted at run time. Every existing
  loader reads by path, and only what is asked for is read. A missing
  `.meta` falls back to the loader's defaults (`server/mod.rs`), so the
  source needs to serve no meta files.
- **A `#label` loader,** `pack.procgena#presets/tree/tall_pine.tree.yml`.
  A loader can run nested loaders on in-memory readers
  (`load_value_from_reader`, `add_loaded_labeled_asset`), but it reads
  the whole bundle at once, and entries can't load other entries by path.
- **The wasm reader fetches whole files** (`io/wasm.rs`); range reading on
  the web would need our own reader. Under the profile it needs only the
  central directory and plain byte ranges.

**Open for ST6:** studio_plan.md's ST6 "registers the project as one more
asset source". Sources are fixed at startup, so a project opened or
switched at run time can't be a source of its own. ST6 must settle this
on its own, for example with a fixed project source whose root it can
redirect, now that the mount source is deferred.

## Textures and models

- **BC7 KTX2 with mipmaps and Zstandard only** (decision 8), as ST6
  imports with `ctt`. 128 px is always exported. A missing larger size
  falls back to the largest present.
- **Models are GLB** (one file, buffers inside), for the future
  `MeshScatter`. `.gltf` with side files is refused.
- **Generated meshes are never stored.** They are deterministic from
  options and seeds, and a stored copy would go stale when a generator
  changes. OBJ and glTF export stays separate (rule 11).

## Size and security

Bundles are untrusted input. On top of the profile checks, the reader
enforces:

- **Paths:** relative, `/` separators, UTF-8, at most 255 bytes, each
  part matching `[a-z0-9_.-]+` with no `.` or `..` parts. The exceptions
  are the root files `README.md` and `LICENSE`. No backslashes, drive
  letters, leading `/` or NUL. No two names that differ only in case.
  This blocks [Zip Slip](https://security.snyk.io/research/zip-slip-vulnerability).
  Unpack and Import also refuse to write outside the target folder.
- **No links, devices or encryption,** by the profile's attribute and flag
  rules.
- **One source of truth:** local and central records match, entries
  neither overlap nor leave gaps, and each name appears once.
- **Limits** (proposed constants): 10 000 entries; YAML ≤ 1 MiB a file;
  any entry ≤ 256 MiB; total ≤ 2 GiB. With nothing compressed, an entry's
  size on disk is its real size, so there are no decompression bombs.
- **YAML alias bombs:** `serde_saphyr`'s limits are to be confirmed in
  the PB1 spike; if it has none, stop and ask.
- **Data only:** no shaders, scripts or native code.

## Versioning and migration

- `format: 1`, checked first. Another number is rejected, naming
  `made_with`. No migration code (the user's standing rule).
- The profile belongs to `format: 1`. A later format may change it, and a
  reader rejects formats it doesn't know.
- Preset files carry no version. An options change makes an old file fail
  with the setting's path, as a loose `.tree.yml` does today.

## Dependencies

None is added by this plan. Figures are from crates.io on 2026-11-24.
All licences are on `deny.toml`'s allow list.

| Crate | Version | Downloads (all / recent) | Licence | In tree? | For |
|---|---|---|---|---|---|
| `crc32fast` | 1.5.2 | — | MIT or Apache-2.0 | **Yes** (through `flate2`) | CRC-32, if we write our own zip code |
| `rawzip` | 0.5.3 | 0.9 M / 0.6 M | MIT | No | Zip, no dependencies |
| `zip` | 9.8.2 | 361 M / 112 M | MIT | No | Zip, with default features off |
| `blake3` | 1.8.7 | 238 M / 67 M | CC0-1.0 or Apache-2.0 | **Yes** (`bevy_asset`) | Content hashes |
| `flate2` | 1.1.10 | 891 M / 252 M | MIT or Apache-2.0 | **Yes** (`png`) | Only if question 3 accepts DEFLATE |

## Staged build plan

Names are **PB1–PB4** (preset bundles). They follow ST2, ST3 and ST6 and
don't run beside them.

| Stage | Covers | Needs | Done when |
|---|---|---|---|
| **PB1: container and crate** | `procgena_bundle`: choose the zip code (decision 11); the profile writer and the strict reader; path rules and limits; `procgena.yml`; `unpack`. A spike first on `serde_saphyr`'s alias limits. Fuzz the reader. | Question 3; a dependency, if one is chosen, through the coordinator | Tests write a folder and read it back from the file and from the folder with equal results, with every data offset a multiple of 64. They reject the hostile corpus with the right messages: traversal, symlink, duplicate, gap, overlap, mismatched headers, DEFLATE, zip64, data descriptor, comment, wrong `format`. The interop test opens a written bundle with `unzip -t`, Python's `zipfile`, Archive Utility and Explorer. |
| **PB2: Export** | The closure walk, the export dialog, the manifest from the project's `procgena.yml`, flattening, thumbnails | PB1; ST2 (registry, "used by"); ST1's `ref` metadata; ST8's export panel for the dialog | A headless script exports a project Ecotope and gets a bundle holding exactly its project presets, with no built-ins. Biome roots wait for `.biome.yml` files (B1/W0). |
| **PB3: Open and Import** | Open by unpacking; Import with the plan, conflicts, renames through `yamlpatch`, and seed-keeping `key` lines | PB1; ST2 (registry, Update's `yamlpatch`); ST3 (Library) | A headless script imports a bundle whose tree clashes with a project tree. Import renames it, the imported Scatter places the same copies as in the bundle, and the comments survive. |
| **PB4: textures** | Texture ids in the closure; the bundle's `textures.yml` merged on Import and Open; KTX2 sizes on Export | PB2, PB3; ST6 (catalogue, `ctt`) | A bundle's bark draws in another project at 128 px with no other size present |
| **Later** | Mount source or `#label` loader for games; HTTP range reading; wasm; bundles that require other bundles, with `bundle:key` references; models in `MeshScatter`; a `procgena bundle pack` command | Decisions 2 and 9 revisited; `MeshScatter` | — |

## Effects on other documents

Proposed only; ST0 is active, so none is edited here.

- **studio_plan.md:** `procgena.yml` gains `id`, `version`, `licence`
  and `authors`. The Library gains Open and Import, ST8's export panel
  gains "Export bundle", and open question 1 is answered by decision 3.
  ST6's "project as an asset source" has the startup conflict noted
  under "Bevy and games: later".
- **ARCHITECTURE.md:** rule 10 gains "presets travel in bundles: a
  subset of a project folder, in a stored, aligned zip". "Seeds" notes
  that a tree Scatter species' default child key is its preset's key, so
  tools that rename presets must write the key out.
- **ROADMAP.md:** PB1–PB4 after ST2, ST3 and ST6.
- **A risk ST2 already has, found here:** `ScatterTree` built from a
  preset calls `ChildKey::new(preset.key).expect(...)`
  (`procgena_tree/src/scatter.rs`), and the studio's `free_key` also
  panics on a bad stem. Built-in keys always pass, but user preset keys
  from file names may not, for example one longer than 32 bytes. ST2
  should require preset keys to be valid child keys, or turn these into
  errors.

## Questions for the user

All three were decided by the user on 2026-11-24, each as recommended:
Open unpacks into a new project folder, Import renames on conflict, and
the reader rejects compressed zips.

1. **Does Open unpack, or show the bundle read-only?** *Context:* the
   studio edits only folders (decision 1), and reading straight from a
   file needs the deferred mount source. *Options:*
   - **(a) unpack into a new project folder,** asking where: one code
     path, edits work at once, and it costs a copy on disk;
   - (b) a read-only view first, with "Unpack to edit": quick browsing
     with no disk writes, but a second way to read presets and textures
     in the studio;
   - (c) read-only, unpacking on the first edit: the same cost as (b),
     with a surprise prompt.

   *Recommendation:* (a) now. (b) comes cheaply once the mount source
   exists.

   *Long-term follow-up (user, 2026-11-24):* possibly never unpack, and
   work entirely in the zip: read through the mount source, and rewrite
   the archive on save. A simpler middle step: unpack into a temporary
   working folder the user doesn't pick or see, edit there with the
   normal folder code, and pack back into the `.procgena` on Save. This
   keeps one code path while the user deals only in the bundle file.
   This is an optimisation for later, not planned work.

2. **What should Import do by default on a conflict?** *Context:* a
   bundle's preset or texture has the same key as one in the project or a
   built-in, with different content. *Options:*
   - **(a) rename the imported item** to `<key>_<bundle id>`, shortened to
     a valid key, and rewrite its references, with Use mine and Replace
     offered per item;
   - (b) ask for every conflict, with no default;
   - (c) always use the project's copy.

   *Recommendation:* (a). The summary lists every rename before anything
   is written.

3. **Should the reader accept zips made by Finder or Explorer?**
   *Context:* those tools always compress, so their zips never fit the
   profile. Someone might zip a project folder by hand and rename it to
   `.procgena`. *Options:*
   - **(a) reject them,** with a message saying to unzip and open the
     folder, or to export from the studio. There is one reader, no DEFLATE
     and no decompression bombs, and every `.procgena` is range-readable;
   - (b) accept DEFLATE through a second, lenient reader with bomb limits.
     Hand-made zips then open, but it adds `flate2`, limits and a second
     code path, and range readers can't read those bundles in place.

   *Recommendation:* (a). The folder is already the hand-made form.
