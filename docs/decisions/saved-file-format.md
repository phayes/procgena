# Saved-file format: preset bundles

Status: **partly decided.** The user answered the first round of questions
on 2026-11-24 (see "Decisions"). The **container is not decided**: the
user is not sure zip is right, so "Container" reviews every candidate
again, for lazy reading and later HTTP range requests, and question 1 asks
the user to choose. Anything still marked "proposed" waits for the user.
No code was changed or run.

Written from ARCHITECTURE.md ("Seeds", rule 10 "One preset and file
pattern", "Bringing your own assets"),
[studio_plan.md](../../procgena/studio_plan.md) ("Preset registry and the
project folder", "Documents and the edit model", "The texture library",
"Stages", open question 1), [ground-materials](ground-materials.md)
(texture sizes, level 1 as the default), `procgena_core::preset`,
`plant/src/files.rs`, `procgena_tree/src/scatter.rs` and
`scatter/file.rs`, `procgena_biome/src/ecotope/file.rs`,
`tree/textures.yml`, the `bevy_asset` 0.20.1 source, `Cargo.lock`,
crates.io (2026-11-24), and the sources linked under "Prior art".

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
10. **The extension is `.procgena`** (Q11).

**Not decided:** the container (Q10). It is question 1 below, with the zip
library nested under it.

## Bottom line

- **A bundle is a subset of a project folder, packed into one file.** The
  layout inside is the project folder's (`procgena.yml`,
  `presets/<kind>/<key>.<kind>.yml`, `textures/`), so packing and
  unpacking are copies, and the folder stays the git-friendly working
  form.
- **Container (proposed, the user to choose): zip with every entry
  stored uncompressed, under a strict writer profile.** That means
  `procgena.yml` first, then every preset file, then thumbnails, then
  textures and models; no archive comment; and no extra fields. The
  second round found that **every indexed format reads one preset for the
  same cost**: one seek on a local file, which is the decided use, and
  about two HTTP round trips later. Formats with the index at the front
  (ASAR, a PMTiles-style custom format) save one round trip only when a
  server refuses suffix ranges. In exchange they lose double-click opening
  on every OS, zipping a folder by hand, and readers that others have
  already fuzzed. tar has no index. Compressed streams (tar.zst, seekable
  zstd, 7z) gain nothing on textures that are already compressed. SQLite
  stores large blobs as chains of linked pages, which is slow over
  ranges, and adds a C library.
- **Export gathers a closure; Open unpacks; Import copies with
  renames.** Export follows `base`, preset references, texture ids and
  model paths from the chosen roots and copies everything but built-ins.
  "Open" unpacks into a new project folder (proposed; question 2).
  "Import" copies into the current project. It skips identical items and,
  by default, renames conflicting ones, rewriting `base:`, references and
  texture ids to match. It writes an explicit tree Scatter `key` where a
  rename would otherwise change a seed.
- **The manifest is the project's `procgena.yml`**, with `format: 1`, an
  id, name, version, licence, authors and `made_with`, and no entry list.
- **The reader is strict and safe on untrusted files** (see "Size and
  security"). Generated meshes are never stored.
- **No new dependency is added by this plan.** The library choice is
  nested under question 1.

## Container

### What the criteria mean here

- **Lazy reading:** list a bundle's presets and read one without reading
  the whole file. For the decided use, the studio's Open and Import of a
  local file, this costs one or two seeks in any format with an index.
  Read whole, a bundle is a few MiB. Lazy reading matters for HTTP range
  requests later: browsing a bundle library online, or a game streaming
  presets.
- **Round trips** are counted for HTTP with ranges, the later case, under
  stated assumptions:
  - **Suffix ranges work.** `Range: bytes=-N` returns the last N bytes
    without knowing the file's size. RFC 9110 defines it;
    [Cloudflare documents it](https://developers.cloudflare.com/cache/reference/range-requests/);
    S3 and CloudFront support ranges but no result here confirmed suffix
    ranges; GitHub Pages is unverified.
  - **Suffix ranges don't work.** Then a front range `bytes=0-N` comes
    first. Its `Content-Range` gives the total size, so a format with its
    index at the end needs one more trip.
    [ReplayWeb.page](https://www.github.com/webrecorder/replayweb.page/issues/246)
    sends a HEAD first, which costs the same.
  - **The index outgrows the first window.** One more trip in every format.
  - Requests that don't depend on each other go out in parallel, so
    "trips" counts the sequential ones.
- **Not a criterion any more:** updating the packed file in place. Under
  decision 1 a bundle is written once by Export and never edited, so
  SQLite's main strength doesn't count.

### Sizes, for scale

| Content | Typical size | Already compressed? |
|---|---|---|
| A preset file | 0.2–5 KiB of YAML | No |
| A texture map at 128 px (the level 1 default) | ≈ 21 KiB BC7 with mips, before Zstandard | Yes (KTX2 Zstandard) |
| A texture set at 512 px, 1k and 2k (3 maps) | ≈ 21 MiB with mips, before Zstandard | Yes |
| A GLB model | 50 KiB – 10 MiB | Partly |

A bundle of 20 presets and one 512 px set holds about 50 KiB of YAML
beside about 1 MiB of KTX2. Deflating the YAML would save 35–40 KiB.
Compression doesn't decide the container.

### Round trips

Each column assumes the writer orders `procgena.yml` and all YAML first,
and that the index fits the first fetch (64 KiB), unless the cell says
otherwise.

| Format | Index location | Trips to the index: suffix ranges work / don't | Trips to one entry after the index | Manifest and every preset, from nothing |
|---|---|---|---|---|
| **Zip, stored, our profile** | End (central directory, 22-byte end record) | **1 / 2** | **1**: the local header is 30 bytes plus name and extra field. Exact when our writer adds no extra field; otherwise over-fetch ~1 KiB. | **1 / 2**: a front window and the tail (in parallel when suffix ranges work). The front window's local headers give the YAML, and the central directory confirms it. |
| Zip with a generated index as its first entry | Front (and end) | 1 / 1 | 1, exact | 1 |
| tar, plain | None | n/a: walk the 512-byte headers, one trip per entry before the one wanted | k sequential trips for the k-th entry | 1 if all YAML fits the first window |
| tar + TOC at the end ([eStargz](https://github.com/containerd/stargz-snapshotter/blob/2713db58dc69454d4ed5cffdd277560e5c52aa25/docs/estargz.md): TOC as the last entry, 51-byte gzip footer) | End | 1 / 2 | 1 | 1 |
| tar + separate index ([SOCI](https://aws.amazon.com/blogs/containers/under-the-hood-lazy-loading-container-images-with-seekable-oci-and-aws-fargate)) | A second file | 1 (other file) | 1 | 1, but two files to share |
| [Seekable zstd](https://github.com/facebook/zstd/blob/dev/contrib/seekable_format/README.md) around a tar | End (seek table in a skippable frame) maps only uncompressed offsets; a file index still needs a TOC inside | 2 / 3 | 1, plus decompressing the frame | 2 |
| [ASAR](https://github.com/electron/asar) | Front: an 8-byte size, then a JSON header with offsets | **1 / 1** (+1 if the JSON outgrows the window; its size is in the first 16 bytes) | 1, exact | 1 |
| Custom, index at the front ([PMTiles](https://gist.github.com/DerZade/cda63bccd24b3999b04e429955f61bfb): 127-byte header, root directory within the first 16 KiB. Godot `.pck` is described with a directory of path, offset, size and MD5. Its offset storage changed in 4.3 and a new pack version came in 4.5; where its directory sits was not checked.) | Front | **1 / 1** | 1, exact | 1 |
| SQLite (`sqlar`) over an HTTP VFS ([sql.js-httpvfs](https://npmjs.com/package/sql.js-httpvfs)) | Pages: B-tree | 1 for page 1, then the B-tree depth (about 2–3 sequential) | A blob larger than a page continues in a **linked list of overflow pages**, each found from the one before (SQLite's file-format document, from memory; not fetched in this review). Read-ahead hides some of this; a 1 MiB texture at 4 KiB pages is a 256-page chain. | 3–4 |
| [7z](https://py7zr.readthedocs.io/en/latest/archive_format.html), non-solid, stored | A 32-byte start header points to the header at the end, often itself compressed | 2 / 2 | 1; solid archives need their whole block decompressed from its start | 2 |
| Unreal `.pak` | End: fixed footer, then index (details from community tools, unverified) | 1–2 / 2–3 | 1 | 2 |

**What the table says:** with suffix ranges, zip and the front-index
formats tie: one trip for the index, one per entry, and one for "manifest
and all presets" when YAML comes first. Without suffix ranges, zip needs
one trip more, both to reach the index and to get manifest and presets.

**When a host ignores `Range` altogether** and returns the whole file
with a 200, as [MDN](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Range)
allows and one report says a CDN does under gzip:
- a front-index format with YAML first lets the client read the index
  and every preset from the first bytes of the stream, then stop;
- zip must read to the end for its central directory, or trust local
  headers before seeing it, against the rule that the central directory
  is the truth.

This is the one case where a front index clearly wins. On a local file, all indexed formats
cost one seek. tar without an index and SQLite's blob chains are the only
real losers on access.

### The rest of the criteria

| Format | Opens by double-click / CLI | Writing | Alignment | Implementation and dependencies | Folder form | Verdict |
|---|---|---|---|---|---|---|
| **Zip, stored** | **Windows, macOS, Linux** / `zip`, `unzip`, `7z` | One pass to a seekable file; sizes known up front, as nothing is compressed | Pad the local extra field ([USDZ](https://openusd.org/dev/spec_usdz.html): 64 bytes; zipalign: 4) | A crate (`zip` or `rawzip`) or our own ~400 lines on `flate2` and `crc32fast`, both already in the tree | Paths are entries; a hand-zipped folder opens (DEFLATE read) | **Proposed** |
| Zip + generated front index | Same | Two passes, or offsets computed up front | Same | Zip, plus our index and a check that it matches the central directory: two sources of truth to keep in step | Same; hand-zipped bundles lack the index and fall back to the tail | Saves one trip only without suffix ranges; reopens decision 4 |
| tar | Linux, macOS; recent Windows (from memory) / `tar` | One pass, streaming | 512-byte blocks | `tar` crate (not in tree) | Paths are entries | No index |
| tar + TOC (eStargz-like) | As tar; gzip per file adds nothing for us | One pass, TOC last | — | Our TOC convention on `tar` | Same | A zip central directory by another name, with worse tools |
| tar + SOCI-style sidecar | As tar | Two files | — | Same | Two files to share | Breaks "one file" |
| Seekable zstd | `zstd -d` decompresses it, then `tar` | Frames per entry | — | `zstd-framed` (MIT/Unlicense, 0.3 M downloads) or `zeekstd` (BSD-2, 72 k), plus tar | A file index on top | Compresses incompressible data; two layers |
| ASAR | No / `npx @electron/asar` | Header first: all sizes known first, then data | None built in | `asar` crate (Apache/MIT, 54 k downloads), or ~200 lines of our own | JSON tree mirrors folders | Front index, but Electron's quirks (Pickle sizes, offsets as strings) and no OS support |
| **Custom front index** | No / our `procgena pack` and `unpack` only | Header and index first, then data | Ours to choose | ~300–500 lines of our own, plus fuzzing and a CLI; no dependency | Path-keyed index | **The alternative** if range reads without suffix ranges matter more than tooling |
| SQLite `sqlar` | DB browsers / [`sqlite3 -A`](https://sqlite.org/sqlar.html) | Transactions; in-place update (no longer needed) | Pages | `rusqlite` (MIT, 166 M downloads) with bundled C SQLite, not in tree; a C build | `sqlar` is path → blob, compressed when smaller | Slow for large blobs over ranges; adds a C build |
| 7z | 7-Zip; recent Windows Explorer (from memory) / `7z` | Header last; solid by default | — | `sevenz-rust2` (Apache-2.0, 2.6 M downloads) | Paths | Complex, built for solid compression |
| Unreal `.pak`, Godot `.pck` | Engine tools only | Custom | Custom | Ours | — | Prior art only |

Others considered:
- Parquet keeps its index in a footer, and Zarr v3 shards keep theirs at
  a configurable end. Cloud-Optimized GeoTIFF puts its directories at the
  front. All three are read over ranges in production, but none is a file
  archive. This line is from memory, not checked in this review.
- WACZ is the closest real use of zip over ranges:
  [WACZ](https://specs.webrecorder.net/wacz/1.2.0/) is a stored zip of web
  archives, and ReplayWeb.page reads it lazily in the browser with range
  requests. It reads the end record, then the central directory, then
  each entry's local header. Its hosts must serve the file without gzip,
  or ranges break.
- `.conda` packages are an uncompressed outer zip, so a reader can get at
  the metadata without decompressing the payload.

### Recommendation

**Zip, stored, with a strict writer profile** (proposed):

1. `procgena.yml` is the first entry, then every `.yml` sorted by path,
   then thumbnails, then textures and models.
2. Every entry is STORED; no archive comment; no extra fields in local
   headers, so an entry's data starts at `offset + 30 + name length`
   (whether the `zip` crate writes it this way is a spike item; if not,
   use `rawzip` or our own writer); no data descriptors (the writer
   seeks back); no zip64 (bundles stay under 2 GiB).
3. Readers treat the central directory as the truth, and accept DEFLATE
   and data descriptors from hand-made zips within the limits below.

This keeps every tool people already have and costs one extra round trip
only on servers that refuse suffix ranges. The custom front-index format
is the real alternative, and question 1 puts both to the user.

## Inside the container

### Layout

```
alpine_pack.procgena            (or the folder it unpacks to)
  procgena.yml                  # first entry, stored: the manifest
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

- **Preset kinds** come from the paths. **The entry list** is the
  container's own index (decision 4). **Hashes:** each entry's CRC-32 is
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
4. **Write** a `.procgena` file with the writer profile. The same input
   gives the same bytes: fixed timestamps, sorted paths.

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
which only the deferred mount source would give. That is question 2.

### Import into the current project

1. **Choose** what to import: all, the bundle's `roots`, or a pick; the
   closure inside the bundle comes along.
2. **Plan each item** against the project and the built-ins:
   - **New key:** copied.
   - **Same key, identical content:** skipped, and the project's copy is
     used. Presets are compared as settings trees after parsing, so
     comments and layout don't count. Textures are compared by `blake3`.
   - **Same key, different content, or a built-in's key:** a conflict.
     Under decision 3, a project preset with a built-in's key is a problem
     to qualify, so Import avoids creating one. The qualifier's syntax is
     ST2's to fix along with studio_plan.md's open question 1; `builtin:<key>`
     is proposed. Each conflict offers **Rename**. The default name is
     `<key>_<bundle id>`, which can be edited. Like every preset key,
     the new name must be a valid child key: `[a-z][a-z0-9_]*`, at most
     32 bytes (`procgena_core::seed`). So it is shortened and numbered
     (`_2`, `_3`) when needed.
     **Use mine** (skip it and point the imported files at the project's
     copy), or **Replace** (overwrite the project's copy, showing its
     "used by" first).
3. **Rewrite references to renamed items** inside the imported files:
   `base:`, preset keys in Scatter, Ecotope and Biome files, and texture
   ids. Edits go through `yamlpatch`, as ST2's Update does, so the
   bundle's comments survive.
4. **Keep seeds.** A tree Scatter species with no `key` takes its tree
   preset's key as its child key, and the child key salts its seed
   (`procgena_tree/src/scatter/file.rs`, `scatter.rs`; ARCHITECTURE.md
   "Seeds"). When Import renames that tree, it writes `key: <old key>`
   into the species entry, so the Scatter places exactly as it did in the
   bundle. Writing `key:` into an item of a list is an edit that ST2's
   `yamlpatch` spike doesn't cover (it covers replace, insert a nested key
   and remove), so PB3 starts with a spike of its own. No other reader
   takes a child key from a preset key today. Every `ChildKey::new` call
   site was checked: the others build keys from map keys, fixed stems or
   numbered names. In detail:
   - Ecotope Scatters and ground layers are keyed by their map keys;
   - stone classes name their own `key`;
   - dead wood and mushroom references resolve to options.

   Any reader that adds such a default later must be added to this step.
5. **Write** new files whole, each with a first comment line
   `# Imported from alpine_pack 0.3.0` (proposed). Show a summary, then
   reload the registry.

## Loading

### Engine-free: `procgena_bundle`

A new engine-free crate, `core/crates/procgena_bundle`:

- `Bundle::open(path)` reads a `.procgena` file or a folder;
  `Bundle::from_bytes` reads one in memory. Both read the index once,
  check every path and limit, read `procgena.yml`, and list presets by
  kind and key. No other entry is read yet.
- `bundle.read(path)` reads one entry, within its limits.
- `export(project, roots, options, out)`, `unpack(bundle, dir)` and
  `plan_import` / `apply_import` hold the logic above, so the studio and a
  future CLI share it.

It stays out of `procgena_core` so that only crates that read bundles
build the container code. `procgena_core` rebuilds most of the workspace.
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
  the web would need our own reader.

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

Bundles are untrusted input. The reader enforces:

- **Paths:** relative, `/` separators, UTF-8, at most 255 bytes, each
  part matching `[a-z0-9_.-]+` with no `.` or `..` parts. The exceptions
  are the root files `README.md` and `LICENSE`. No backslashes, drive
  letters, leading `/` or NUL. No two names that differ only in case.
  This blocks [Zip Slip](https://security.snyk.io/research/zip-slip-vulnerability).
  Unpack and Import also refuse to write outside the target folder.
- **No links, devices or encryption.** The `zip` crate had a symlink
  traversal advisory, fixed in 2.3.0
  ([SNYK-RUST-ZIP-9460813](https://security.snyk.io/vuln/SNYK-RUST-ZIP-9460813)),
  so we don't use its extraction helpers.
- **One source of truth:** local headers must agree with the index
  (names and sizes); entries must not overlap; each name appears once.
- **Limits** (proposed constants): 10 000 entries; YAML ≤ 1 MiB a file;
  any entry ≤ 256 MiB; total ≤ 2 GiB. Only STORE and DEFLATE. A DEFLATE
  entry may expand at most 100:1, and its declared size is enforced while
  inflating.
- **YAML alias bombs:** `serde_saphyr`'s limits are to be confirmed in
  the PB1 spike; if it has none, stop and ask.
- **Hand-zipped files:** `__MACOSX/`, `.DS_Store`, `Thumbs.db` and
  folder entries are skipped. A single wrapping folder is accepted as the
  root. Data descriptors are accepted.
- **Data only:** no shaders, scripts or native code.

These rules are written for zip. A custom format needs the same path,
overlap and limit checks, minus the compression ones.

## Versioning and migration

- `format: 1`, checked first. Another number is rejected, naming
  `made_with`. No migration code (the user's standing rule).
- Preset files carry no version. An options change makes an old file fail
  with the setting's path, as a loose `.tree.yml` does today.

## Dependencies

None is added by this plan. Figures are from crates.io on 2026-11-24.
All licences are on `deny.toml`'s allow list.

| Crate | Version | Downloads (all / recent) | Licence | In tree? | For |
|---|---|---|---|---|---|
| `zip` | 9.8.2 | 361 M / 112 M | MIT | No | Zip: use `default-features = false` plus deflate. To confirm in the spike: the deflate feature's name, and whether it writes local headers with no extra field. |
| `rawzip` | 0.5.3 | 0.9 M / 0.6 M | MIT | No | Zip, no dependencies; pair with `flate2` |
| `flate2` | 1.1.10 | 891 M / 252 M | MIT or Apache-2.0 | **Yes** (`png`) | DEFLATE for hand-zipped reads |
| `crc32fast` | 1.5.2 | — | MIT or Apache-2.0 | **Yes** | Our own zip code |
| `blake3` | 1.8.7 | 238 M / 67 M | CC0-1.0 or Apache-2.0 | **Yes** (`bevy_asset`) | Content hashes |
| `asar` | 0.3.0 | 54 k / 24 k | Apache-2.0 or MIT | No | ASAR |
| `tar` | 0.4.46 | 302 M / 86 M | MIT or Apache-2.0 | No | tar rows |
| `zstd-framed` / `zeekstd` | 0.1.1 / 0.6.3 | 0.3 M / 72 k | MIT or Unlicense / BSD-2-Clause | No | Seekable zstd |
| `rusqlite` | 0.43.0 | 166 M / 68 M | MIT | No (bundles C SQLite) | SQLite |
| `sevenz-rust2` | 0.25.0 | 2.6 M / 1.6 M | Apache-2.0 | No | 7z |
| `ruzstd` | 0.9.1 | 84 M / 23 M | MIT | **Yes** (`bevy_image`) | Already decodes our KTX2 |

## Staged build plan

Names are **PB1–PB4** (preset bundles). They follow ST2, ST3 and ST6 and
don't run beside them.

| Stage | Covers | Needs | Done when |
|---|---|---|---|
| **PB1: container and crate** | `procgena_bundle`: the chosen container (question 1), the writer profile, the strict reader, path rules and limits, `procgena.yml`, `unpack`. A spike first: the library choice, then `serde_saphyr` alias limits. Fuzz the reader. | The user's answer to question 1; the dependency through the coordinator | Tests pack a folder and read it back from the file and from the folder with equal results. They reject the hostile corpus (traversal, symlink, duplicate, overlap, bomb, wrong `format`) and read a zip made by Finder. |
| **PB2: Export** | The closure walk, the export dialog, the manifest from the project's `procgena.yml`, flattening, thumbnails | PB1; ST2 (registry, "used by"); ST1's `ref` metadata; ST8's export panel for the dialog | A headless script exports a project Ecotope and gets a bundle holding exactly its project presets, with no built-ins. Biome roots wait for `.biome.yml` files (B1/W0). |
| **PB3: Open and Import** | Open by unpacking; Import with the plan, conflicts, renames through `yamlpatch`, and seed-keeping `key` lines | PB1; ST2 (registry, Update's `yamlpatch`); ST3 (Library) | A headless script imports a bundle whose tree clashes with a project tree. Import renames it, the imported Scatter places the same copies as in the bundle, and the comments survive. |
| **PB4: textures** | Texture ids in the closure; the bundle's `textures.yml` merged on Import and Open; KTX2 sizes on Export | PB2, PB3; ST6 (catalogue, `ctt`) | A bundle's bark draws in another project at 128 px with no other size present |
| **Later** | Mount source or `#label` loader for games; HTTP range reading; wasm; bundles that require other bundles, with `bundle:key` references; models in `MeshScatter`; alignment | Decisions 2 and 9 revisited; `MeshScatter` | — |

## Effects on other documents

Proposed only; ST0 is active, so none is edited here.

- **studio_plan.md:** `procgena.yml` gains `id`, `version`, `licence`
  and `authors`. The Library gains Open and Import, ST8's export panel
  gains "Export bundle", and open question 1 is answered by decision 3.
  ST6's "project as an asset source" has the startup conflict noted
  under "Bevy and games: later".
- **ARCHITECTURE.md:** rule 10 gains "presets travel in bundles: a
  subset of a project folder, packed". "Seeds" notes that a tree Scatter
  species' default child key is its preset's key, so tools that rename
  presets must write the key out.
- **ROADMAP.md:** PB1–PB4 after ST2, ST3 and ST6.
- **A risk ST2 already has, found here:** `ScatterTree` built from a
  preset calls `ChildKey::new(preset.key).expect(...)`
  (`procgena_tree/src/scatter.rs`), and the studio's `free_key` also
  panics on a bad stem. Built-in keys always pass, but user preset keys
  from file names may not, for example one longer than 32 bytes. ST2
  should require preset keys to be valid child keys, or turn these into
  errors.

## Questions for the user

1. **Which container?** *Context:* the second round compared every
   candidate for lazy reading and later HTTP range requests. On a local
   file, which is how the studio uses bundles today, every indexed format
   costs one seek. Over HTTP, zip and a front-index format tie when
   servers honour suffix ranges. Zip needs one more round trip when they
   don't. When a host ignores ranges altogether, only a front index can
   stop after the first bytes. *Options:*
   - **(a) Zip, stored, with the strict writer profile.** It opens by
     double-click everywhere, people can zip a folder themselves, and the
     format and its readers are proven in use, WACZ already over ranges.
     Its index is at the end. Its attack surface is wide (zip64, local
     headers against the central directory), which the strict reader
     narrows.
   - **(b) A custom format with the index at the front,** in the style of
     PMTiles: magic, version and index in the first 16 KiB, with data
     after. One round trip to the index on any server, exact ranges, and
     no dependency. But no OS or third-party tool opens it, so we must
     ship pack, unpack and list commands. It is ~300–500 lines of our own
     to write and fuzz, and its format is ours to keep stable.
   - **(c) ASAR:** a front index in an existing format, but with Electron's
     quirks and no OS support.
   - **(d) SQLite `sqlar`:** queries and in-place updates (no longer
     needed), slow blob reads over ranges, and a bundled C library.

   *Nested, if (a): which zip code?*
   - (a1) the `zip` crate, default features off: the most used, but with
     unconfirmed control over local extra fields;
   - (a2) `rawzip` plus `flate2`: no dependencies, young;
   - (a3) our own: a stored writer and a strict reader on `flate2` and
     `crc32fast` (both already in the tree), so no new crate, but the
     fuzzing is ours.

   *Recommendation:* (a), and between (a1) and (a3), whichever the PB1
   spike shows can write the profile exactly. (b) is the honest
   alternative if serving bundles from arbitrary static hosts matters
   more than tooling.

2. **Does Open unpack, or show the bundle read-only?** *Context:* the
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

3. **What should Import do by default on a conflict?** *Context:* a
   bundle's preset or texture has the same key as one in the project or a
   built-in, with different content. *Options:*
   - **(a) rename the imported item** to `<key>_<bundle id>` and rewrite
     its references, with Use mine and Replace offered per item;
   - (b) ask for every conflict, with no default;
   - (c) always use the project's copy.

   *Recommendation:* (a). The summary lists every rename before anything
   is written.
