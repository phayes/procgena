# Saved-file format: preset bundles

Status: **proposed, not decided** (planned 2026-11-24). Nothing here is
the user's decision yet. Each choice that needs one is listed under
"Questions for the user". No code was changed or run.

Written from ARCHITECTURE.md ("Seeds", rule 10 "One preset and file
pattern", "Bringing your own assets", "Edits"),
[studio_plan.md](../../procgena/studio_plan.md) ("Preset registry and the
project folder", "Documents and the edit model", "The texture library",
"Stages", open question 1), [ground-materials](ground-materials.md)
(texture sizes, level 1 as the default), `procgena_core::preset`,
`plant/crates/procgena_plant/src/files.rs`, `procgena_tree/src/scatter/file.rs`,
`procgena_biome/src/ecotope/file.rs`, `tree/textures.yml`, the
`bevy_asset` 0.20.1 source, `Cargo.lock`, crates.io (2026-11-24), and the
prior art listed under "Prior art".

One correction to the brief: plant no longer has `.plant.ron` or
`.field.ron` files. `plant/crates/procgena_plant/src/files.rs` reads `.plant.yml` and
`.plant_scatter.yml` through `procgena_core::preset`, like every other
kind.

## Bottom line

- **A bundle is a project folder, zipped.** The unpacked form is the
  project folder that studio_plan.md already decided (`procgena.yml`,
  `presets/<kind>/<key>.<kind>.yml`, `textures/`). The packed form is the
  same tree in one zip file. There is one layout and one reader with two
  back ends, so the folder form is the development form, and git diffs
  stay readable. Proposed extension: **`.procgena`**.
- **Zip, written with every entry STORED (no compression).** Textures are
  BC7 KTX2 already supercompressed with Zstandard, and GLB models are
  binary. Together they outweigh the YAML by tens to thousands of times,
  so compressing the archive saves a few tens of KiB at most. The user's instinct is right
  that compression is pointless here. That argues for STORE, not for tar.
  Zip keeps what tar lacks: an index at the end (the central directory),
  so one preset is read without reading the rest; a compression method per
  entry; and opening by double-click on every desktop OS. The reader
  also accepts DEFLATE, so a folder zipped with Finder or Explorer still
  opens.
- **The manifest is the project's `procgena.yml`**, extended with a
  bundle id, a version, a licence and the bundles it needs. It holds **no
  entry list and no hashes**. The zip's central directory is the entry
  list, the paths give each preset's kind and key, and zip's CRC-32
  checks each entry. A hand-kept file list would go stale in the folder
  form. A list generated when packing is an alternative, given under
  question 4.
- **Bundles are a read-only layer in the preset registry**: project first,
  then mounted bundles, then built-ins. This extends the layered registry
  that ST2 proposes. Editing a bundle's preset works like editing a
  built-in: "Save as new" into the project, or "Unpack to project".
- **In Bevy, a bundle is mounted, not loaded with a `#` label.** One
  asset source, `procgena://`, is registered at startup. Bundles are
  mounted into it at run time as `procgena://<bundle id>/<path>`. Every
  existing loader (`.tree.yml`, KTX2, GLB) then reads entries by path, and
  only what is asked for is read. A `#label` loader also works, because
  Bevy 0.20 can hand an in-memory reader to a nested loader, but it reads
  and decodes the whole bundle at once. See "Loading in Bevy".
- **Generated meshes are never stored.** A bundle holds inputs: presets,
  textures, and your own models for a future `MeshScatter`. Meshes are
  always regenerated from options and seeds. Thumbnails and impostor bakes
  stay in `.cache/`, unless question 6 decides otherwise.
- **The reader is strict, and safe on untrusted files.** It checks
  paths, entry and size limits, and allowed file types, and accepts no
  symlinks, encryption or duplicate names. See "Size and security".
- **No new dependency is added by this plan.** The proposed crate is
  `zip` (MIT, about 361 million downloads) with default features off.
  `rawzip` (MIT, no dependencies) is the fallback. Both go through the
  coordinator at B1. See "Dependencies".
- **Versioning:** `format: 1` in `procgena.yml`. A reader rejects any
  other number with a clear message. Old formats are not supported, by
  the user's standing rule.

## Requirements

| # | The user's requirement | Where it is met |
|---|---|---|
| 1 | One container file, zip or similar | Zip, STORE-only writes; "Container" |
| 2 | One or more YAML or JSON preset files inside | `presets/<kind>/<key>.<kind>.yml`, today's F3 files unchanged; YAML only (question 7) |
| 3 | Its own textures and models | `textures/` with its `textures.yml`; `models/` with GLB files; "Textures and models" |
| 4 | Weigh tar seriously, since the content is already compressed | "Container": the comparison and the size estimate |

## Container

### What a bundle holds, by size

| Content | Typical size | Already compressed? |
|---|---|---|
| A preset file | 0.2–5 KiB of YAML | No. Deflate would shrink it 3–5×, which saves a few KiB in total. |
| A texture map at 128 px (the level 1 default, ground-materials.md) | ≈ 21 KiB BC7 with mips, before Zstandard | Yes. KTX2 Zstandard supercompression; little is left for a second pass. |
| A texture set at 512 px, 1k and 2k (3 maps, as ST6 imports) | ≈ 21 MiB with mips, before Zstandard | Yes, as above. |
| A GLB model | 50 KiB – 10 MiB | Partly. The binary buffers deflate somewhat; meshopt or Draco GLBs are already compressed. |

A small bundle of 20 presets and one texture set at 512 px holds about
50 KiB of YAML beside about 1 MiB of KTX2 (less after Zstandard).
Deflating the YAML saves some 35–40 KiB, a few percent of this bundle and
well under 1 % of one with 1k or 2k textures. **The archive's own
compression saves tens of KiB at most.** So the choice turns on access,
tooling and safety, not on size.

### Comparison

| | Zip, STORE (proposed) | Zip, DEFLATE | tar | tar.zst | Custom (GLB- or PCK-like) | Plain folder |
|---|---|---|---|---|---|---|
| Read one preset without reading the rest | **Yes**: the central directory at the end gives each entry's offset | Yes | Header walk: a seek per entry, as tar has no index | **No**: the stream must be decompressed from the start | Yes, with our own index | Yes |
| Compression per entry | Per entry (STORE or DEFLATE) | Per entry | None | Whole archive, which helps nothing here | Our choice | n/a |
| Streaming write (to a pipe or socket) | Yes, with data descriptors | Yes | **Yes**, simplest of all | Yes | Yes | n/a |
| Streaming read (no seeking) | Possible through local headers, though the central directory is authoritative | Same | **Yes** | Yes | Depends | n/a |
| mmap or zero-copy slices | **Yes**: a stored entry is a byte range | No | Yes for one entry, after the header walk | No | Yes | Yes |
| wasm (the plant web demo) | Fetch whole, then slice: Bevy's web reader fetches whole files (`io/wasm.rs`) | Fetch whole, inflate in Rust | Same, then walk the headers | Fetch whole, decompress all | Same as zip | Many small fetches; a server must list folders |
| Append or update in place | Append is possible (new entries plus a new central directory, leaving dead bytes); **rewrite whole** is proposed | Same | Append is possible; replacing an entry is not | Rewrite | Our code | Edit files |
| Studio's comment-keeping Update (`yamlpatch`) | Patch the text, then rewrite the archive to a temp file and rename | Same | Same | Same | Same | **Patch the file in place** |
| Git diffs | Binary (a `diff=zip` textconv helps) | Binary | Binary | Binary | Binary | **Text** |
| Opens by double-click (Windows, macOS, Linux) | **Yes** | Yes | Linux and macOS; Windows 11 only lately | Needs tools | No | Yes |
| Path and metadata rules | UTF-8 flag, no owners, optional timestamps | Same | ustar's 100-byte name limit (PAX lifts it); uid, gid and mtime must be zeroed for repeatable output | Same as tar | Ours | OS rules |
| Attack surface | Path traversal, symlinks, duplicate names, local and central headers that disagree, zip64 | Same, **plus decompression bombs** | Path traversal, symlinks and hard links, device files | Same as tar, plus bombs | Our own bugs, with no outside fuzzing | Symlinks |
| Crates, and already in our tree? | `zip`, `rawzip`: neither is in the tree | `zip` with flate2; `flate2` is in the tree (through `png`) | `tar`: not in the tree | `tar`, plus either the C `zstd` crate or `ruzstd` (in the tree, through `bevy_image`) for writing; `ruzstd`'s encoder is reported at its fastest level, and sources disagree on the others | None | None |

**Why not tar:** its strengths are streaming and simplicity, and neither
matters here. Bundles are written by the studio to a file, not piped
anywhere. Its weaknesses do matter: there is no index, so the studio's
Library, which lists a mounted bundle's presets, must walk every header;
it has no compression per entry; it carries Unix owners and devices that
must be zeroed or rejected; and Windows users can't open it without
tools. tar.zst loses random access, its main cost, in exchange for a
size gain that rounds to zero on supercompressed KTX2.

**Why not a custom format:** Godot's `.pck`, Unreal's `.pak` and `.glb`
are custom because they serve one engine's fast loading. We would write
our own packer, viewer and fuzz tests, and gain only what aligned zip
already gives.

**Why STORE rather than DEFLATE for writing:** the writer never has to
choose; a stored entry can be sliced straight out of an mmap or a
fetched buffer; and a bundle the studio wrote has no decompression bomb
by construction. The reader still accepts DEFLATE for zips made by hand
(see "Size and security" for the limits that apply then).

**Alignment:** not proposed for format 1. USDZ aligns stored entries to
64 bytes so files can be used in place, but our KTX2 files are Zstandard
supercompressed and must be decoded anyway, so no reader of ours gains
from alignment today. It can be added later without changing the format,
because it only pads the extra field of a local header. Every zip reader
ignores that padding.

### Prior art

| Format | Container | What we take from it |
|---|---|---|
| [USDZ](https://openusd.org/dev/spec_usdz.html) | Zip with no compression; every file 64-byte aligned through the local header's extra field; an allowed list of file types | STORE-only, the allowed list, and alignment as a later option |
| [.conda](https://prefix-dev.github.io/rattler-book/deep-dive-package-format) | An outer **uncompressed** zip holding `metadata.json` (format version only) and inner `info-*.tar.zst` and `pkg-*.tar.zst` | The closest real answer to "tar or zip": they chose an indexed, uncompressed zip on the outside, so the metadata is read without decompressing the payload. Our payload is already compressed, so we need no inner tar at all. |
| Android APK (zipalign), Java JAR, Python wheel | Zip; APKs align stored entries so they can be mmapped | Zip is the usual choice for a package of mixed files |
| [Minecraft resource and data packs](https://minecraft.fandom.com/wiki/Tutorials/Creating_a_resource_pack) | A folder **or** a zip of it, identified by `pack.mcmeta` with a `pack_format` number; data in `data/<namespace>/` | **One layout in two forms**, a root file with a format number, and namespaces to avoid clashes |
| [OpenDocument](https://docs.oasis-open.org/office/v1.2/OpenDocument-v1.2-part3.html) (OpenRaster is understood to package the same way; not checked here) | Zip; a `mimetype` entry first and stored, so the file type sits at a fixed offset; `META-INF/manifest.xml` lists the files present | A known first entry (we use `procgena.yml`). **We avoid** a manifest that must list every file: a [2012 LibreOffice thread](https://listarchives.libreoffice.org/global/users/2012/msg06175.html) found a file failing to open because its list and its contents differed. |
| [.unitypackage](https://docs.rs/crate/unity-pack/0.1.0/source/README.md) | tar.gz; one folder per asset, named by GUID, holding `pathname`, `asset` and `asset.meta` | Ids that survive renames. Its cost is that nothing can be read without decompressing the whole archive. |
| [Godot `.pck`](https://docs.godotengine.org/en/stable/tutorials/export/exporting_pcks.html) | Custom: a header, then a directory of path, offset, size and MD5 | Index plus offsets is what the zip central directory already gives us. Its format version changed in 4.3 and 4.5, breaking third-party tools: a reason not to own a binary format. |
| [Unreal `.pak`](https://dev.epicgames.com/documentation/unreal-engine/oodle-data) | Custom index with a mount point; Oodle compression per block | Mounting at a root path is the model for our `procgena://<bundle id>/` source |
| glTF `.glb` | One file: a 12-byte header, then a JSON chunk and a binary chunk, each 4-byte aligned | Used inside bundles for models, as is. Not a container for many presets. |
| `.blend` | Blender's own block file, which describes its own structs so old files still read | Shows what full backward compatibility costs; we don't need it (the user's rule) |
| [Bevy asset meta files](https://docs.rs/bevy/latest/bevy/asset/meta/struct.AssetMeta.html) | `<file>.meta` beside each asset, with `meta_format_version` and loader settings | We write none. When a reader answers "not found" for a `.meta`, Bevy 0.20.1 picks the loader by extension with its default settings (`server/mod.rs`), so our mount source answers `read_meta` that way. |
| [`bevy_archive_reader`](https://docs.rs/crate/bevy_archive_reader/0.1.0/source/README.md), [`bevy_mod_pakfile`](https://docs.rs/crate/bevy_mod_pakfile/0.1.0/source/src/lib.rs) | Bevy `AssetReader`s over a zip and a Quake PAK (Bevy 0.17 era) | Proof that an archive asset source is the usual pattern; too young or old to depend on, but worth reading for B3 |

## Inside the container

### Layout

```
alpine_pack.procgena            (or the folder alpine_pack/)
  procgena.yml                  # first entry, stored: the manifest
  README.md                     # optional, shown in the studio
  LICENSE                       # optional; the licence named in procgena.yml
  presets/
    tree/        tall_pine.tree.yml
    tree_scatter/ pine_belt.tree_scatter.yml
    ecotope/     alpine_floor.ecotope.yml
    flower/      edelweiss.flower.yml
  textures/
    textures.yml                # this bundle's texture catalogue
    bark/alpine_bark/           # BC7 KTX2 at the sizes it ships
      color_128.ktx2  normal_128.ktx2  orh_128.ktx2 …
  models/
    cairn.glb                   # your own meshes, for MeshScatter (later)
```

- **The same tree as the project folder** in studio_plan.md. Packing a
  project leaves out `.cache/`, `.gitignore`, `.git/`, and texture source
  images. Session state never lived there.
- **Presets are today's F3 files, unchanged:** `base`, `name`,
  `description`, `group`, and the options under the kind's key. The key
  comes from the file name and the kind from the folder and the
  extension. If the two disagree, the file is reported as a problem.
- **Kinds come from paths, so a new kind needs no format change.** The
  `.flower.yml` kind that ST0 is splitting out of fruit is simply
  `presets/flower/`. A kind folder this procgena doesn't know (from a
  newer release, or a misspelling) is reported as a problem and skipped.
  The rest of the bundle still opens.
- **The allowed file types** are `.yml`, `.ktx2`, `.glb`, `.md` and
  `LICENSE`, plus `.png` only for thumbnails if question 6 allows them.
  Anything else is a problem, reported by the reader and left unread.

### The manifest: `procgena.yml`

```yaml
format: 1                       # the bundle format; anything else is rejected
id: alpine_pack                 # bundle id: lower case, digits, underscores; unique among mounted bundles
name: Alpine pack               # display name
version: 0.3.0                  # the author's version, shown and compared; free text
description: Pines, edelweiss and a scree floor for alpine Biomes.
licence: CC0-1.0                # SPDX id; required to pack
authors: [Jo Example]
made_with: 0.1.0                # procgena version that packed it; for messages only
requires:                       # other bundles whose presets or textures this one names
  - id: rock_basics
    version: 0.2.0              # proposed: informational, warned on mismatch
```

What the brief asked the manifest to hold, and where each item went:

| Item | Proposed | Why |
|---|---|---|
| Format version | **Stored**: `format` | The one field every reader checks first |
| Preset kinds | **Derived** from `presets/<kind>/` | A copy in the manifest could disagree with the files |
| Entry list | **Derived** from the zip central directory, or the folder listing | ODF's mandatory list is a known source of "corrupt file" errors, and a hand-edited folder would break it on every save. A list generated at pack time is question 4. |
| Hashes | **Omitted**. Zip's CRC-32 checks each entry on read; content hashes for caches (thumbnails, `TextureCatalogue`) are computed on load with `blake3`, already in the tree through `bevy_asset` | Hashes in a folder's manifest go stale on every edit. Signing is out of scope (question 4). |
| Identity, licence, dependencies | **Stored** | Nothing else can supply them |

The project folder's `procgena.yml` from studio_plan.md gains these fields
too, and keeps its project-only parts (the game profile of open question
8). A project's `id` defaults to its folder name. Packing fails without
a licence.

### References

Today a reference is a bare key. A Scatter's `tree: pine_small`, a file's
`base: pine_medium` and an Ecotope's `stone: boulder` all name built-in
presets, and the reader looks them up with `XPreset::named`. Textures are
ids such as `bark_brown_02` (`BarkOptions::kind`, a crate's
`textures.yml`). ST2 proposes a registry layered as project over
built-in, and leaves key clashes as its open question 1. This plan builds
on that and doesn't settle it. It proposes:

- **Inside a bundle, a bare key looks in the bundle first, then the
  bundles it `requires`, then the built-ins.** Never the project. A
  bundle must mean the same thing in every project.
- **Outside a bundle, a project uses a bundle's preset by bare key** when
  only one layer has it. When several do, it writes `alpine_pack:tall_pine`.
  Whether a clash is an error, a warning, or always qualified is the same
  question as studio_plan.md's open question 1, so the two are settled
  together (question 3 here).
- **Textures follow the same rule.** Texture ids resolve through the
  bundle's `textures/textures.yml` first, so a bundle's `alpine_bark`
  can't be shadowed by a project texture.
- **Models are paths**, relative to the bundle root (`models/cairn.glb`).
- **Seeds must not see the qualifier.** A tree Scatter's species with no
  `key` takes the preset's key as its `ChildKey`
  (`procgena_tree/src/scatter/file.rs`), and that key salts the seed
  (ARCHITECTURE.md "Seeds"). Proposed rule: the default child key is the
  **unqualified** key (`tall_pine`, not `alpine_pack:tall_pine`), so
  moving a preset between the project and a bundle reseeds nothing. If two
  species in one Scatter then share a key, the existing "a parent rejects
  two children whose keys share a salt" rule asks for an explicit `key`.
- **Specimen pools are keyed by an options hash, not a preset key**, so
  where a preset lives never splits or merges pools. The World's
  per-pool override paths (`temperate_forest/pine_floor/pines/scots_pine`)
  start with a Biome preset key. A Biome from a bundle shows there
  unqualified, unless it clashes.

### Exporting a bundle from a project

The studio's export (with ST8's export panel) takes the presets you pick
and adds what they need: their `base` chain, the presets they reference,
and the textures and models those name. Built-in presets are not copied,
because every procgena has them. But built-ins change between procgena
versions, and nothing promises compatibility, so a bundle that
builds on `pine_medium` may look different after an upgrade. Whether
export should flatten built-in bases into full settings is question 5.

## Loading

### Engine-free: `procgena_bundle`

A new engine-free crate, `core/crates/procgena_bundle`, owns the format:

- `Bundle::open(path)` reads a folder or a `.procgena` file;
  `Bundle::from_bytes(Vec<u8>)` reads one already in memory (wasm, tests).
  Both read the central directory or folder listing once, check every
  path and limit, read `procgena.yml`, and list presets by kind and key.
  No entry is read yet.
- `bundle.read(path) -> Bytes`: one entry. A stored entry is a slice of
  the mmap or buffer; a deflated one is inflated within its limits.
- `pack(project_dir, selection, out)` writes STORE-only, `procgena.yml`
  first, entries sorted by path, with fixed timestamps, so the same
  input makes the same bytes. `unpack(bundle, dir)` refuses to overwrite
  existing files and writes nothing outside `dir`.
- **The registry** (`procgena_core::preset`, ST2) gets a bundle layer
  through a small trait (a source of named preset texts), so
  `procgena_core` does not depend on the zip crate. That trait would
  also suit the project folder.

It stays out of `procgena_core` so that only crates that read bundles
build the zip dependency. `procgena_core` rebuilds most of the workspace
(CLAUDE.md).

### Loading in Bevy

Facts from the `bevy_asset` 0.20.1 source:

- **Asset sources are fixed when `AssetPlugin` is built**
  (`AssetApp::register_asset_source` says so, and the server holds an
  immutable `Arc<AssetSources>`). A source per bundle file is therefore
  impossible after startup, so one source must take mounts at run time.
- **A loader can run a nested loader on an in-memory reader**
  (`load_value_from_reader`, `load_erased_value_from_reader`), and add the
  result as a labeled sub-asset (`add_loaded_labeled_asset`). The
  `#label` approach therefore works too.
- **The wasm reader fetches whole files** (`io/wasm.rs`, `fetch_bytes`):
  no HTTP ranges. On the web a bundle is one download either way.

The two ways to load:

| | **Mount source (proposed)**: `procgena://alpine_pack/presets/tree/tall_pine.tree.yml` | **Bundle loader**: `alpine_pack.procgena#presets/tree/tall_pine.tree.yml` |
|---|---|---|
| How | `ProcgenaBundlePlugin` registers the `procgena` source before `DefaultPlugins`. `BundleMounts` (a resource over `Arc<RwLock<…>>`) mounts bundles by id at any time: from a path, or from bytes on wasm. Its `AssetReader` serves entries; `read_meta` answers "not found"; `read_directory` lists entries. | A loader for `.procgena` reads the archive and runs each entry's own loader by extension on an in-memory reader, then adds the result as a labeled sub-asset |
| What is read | Only what is asked for | Every entry, every texture decoded, when the bundle loads |
| Existing loaders (`.tree.yml`, KTX2, GLB) | Unchanged; they read by path | Run nested; must be registered for the bundle loader to find them |
| An entry that loads another by relative path | Works: relative paths resolve in the same source | Breaks: they point at paths that exist only inside the archive. Nothing needs this in format 1 (GLB only, no `.gltf` side files), but a later format might. |
| Hot reload while editing | Packed bundles don't change. Folder-form mounts go through the same reader, so watching them means the source must emit `AssetSourceEvent`s itself: **a PB4 item to confirm**. Until then, the studio edits project files through its own watcher (studio_plan.md "Outside edits reload"). | Reload the whole bundle |
| Startup order | The plugin must be added before `AssetPlugin`; Bevy logs an error otherwise | No constraint |
| One handle for "the bundle" | `BundleMounts` events, plus a `Bundle` asset for the manifest | Natural: the bundle is one asset |

Mounting is proposed because it reads lazily, so a game pays only for
the presets and texture sizes it uses, and it reuses every loader as it
is. Its cost is a plugin-order rule and a little locking. The `#label` form can be added later as a convenience for
small, self-contained bundles. Question 2 asks the user.

Presets in a mounted bundle also reach the **registry**, not only Bevy
assets, because Scatter and Ecotope files resolve references by key
through the registry. Mounting a bundle therefore registers its presets
with the registry layer (engine-free) and its texture set with the
`TextureCatalogue` (ST6/GM2) as one more source. The Bevy texture loader
then fetches `procgena://<id>/textures/...`.

## Textures and models

- **Textures are stored as BC7 KTX2 with mipmaps and Zstandard**, exactly
  as ST6 imports them with `ctt`. Bevy loads them with the `ktx2` and
  `zstd_rust` features our Bevy crates already enable. PNG is not stored:
  it would need conversion at load time and is about 4× larger in GPU
  memory.
- **Which sizes ship** is the author's choice at export: 128 px (the level
  1 default, ground-materials.md) is always included, and 512, 1k and
  2k are optional. A missing size falls back to the largest one present.
- **The web:** BC7 runs on desktop WebGPU (`texture-compression-bc`), but
  not on most phones or on WebGL2. UASTC copies come from the Python tool
  at publishing time (studio_plan.md decision 7). Whether bundles may
  carry them is question 8. The plant web demo loads no texture files
  today (`plant/web/src` loads no images; its textures are made on the
  GPU), so it is not blocked.
- **Models are GLB** (glTF binary, one file, buffers inside), for the
  `MeshScatter` of "Bringing your own assets", which doesn't exist yet.
  Plain `.gltf` with side files is refused, to keep one model in one
  entry.
- **Generated meshes are never stored.** They are deterministic from
  options and seeds (ARCHITECTURE.md "Seeds"), regenerate in
  milliseconds to seconds, and any stored copy would go stale when a
  generator changes. Export to OBJ or glTF stays a separate output
  (rule 11), not part of a bundle.

## Size and security

A bundle is untrusted input: people will download them. Rules for the
reader, applied in both forms:

- **Paths:** relative, `/` separators, UTF-8, at most 255 bytes, each part
  matching `[a-z0-9_.-]+` with no `.` or `..` parts. The only exceptions
  are the root files `README.md` and `LICENSE`. No backslashes, drive
  letters, leading `/` or NUL. No two names that differ only in case, as
  they would collide when unpacked on macOS or Windows. This blocks [Zip
  Slip](https://security.snyk.io/research/zip-slip-vulnerability).
- **No links, devices or encryption.** An entry whose external attributes
  mark a symlink is rejected. The `zip` crate itself had a symlink
  traversal advisory, fixed in 2.3.0
  ([SNYK-RUST-ZIP-9460813](https://security.snyk.io/vuln/SNYK-RUST-ZIP-9460813)),
  so we don't rely on its extraction helpers.
- **One source of truth:** names and sizes in each local header must
  match the central directory, entries must not overlap, and a name may
  appear only once. This blocks overlapping-entry zip bombs and readers
  that disagree about what a file holds.
- **Limits** (proposed, as constants with clear errors): 10 000 entries;
  YAML ≤ 1 MiB per file; any entry ≤ 256 MiB; total ≤ 2 GiB, so zip64 is
  never needed for writing (the reader accepts zip64 records within the
  limits); DEFLATE entries ≤ 100:1 ratio, with the declared size enforced
  while inflating, never trusted. Only methods STORE (0) and DEFLATE (8).
- **YAML:** the existing `serde_saphyr` reader parses each file.
  Alias-expansion bombs ("billion laughs") must be limited there. Its
  budget options are to be confirmed in the B1 spike, and if it has none,
  that is a stop-and-ask.
- **Hand-zipped files:** `__MACOSX/`, `.DS_Store`, `Thumbs.db` and
  folder entries are skipped silently. A wrapping folder (`alpine_pack/`
  as the only top-level entry, which Finder makes) is accepted as the
  root. Data descriptors are accepted. Anything else outside the allowed
  list is a problem, reported and not read.
- **Data only:** bundles carry no shaders, scripts or native code, so
  opening one never runs anything.

## Versioning and migration

- `format: 1` in `procgena.yml`, checked before anything else. Another
  number is rejected with "made by a newer or older procgena", plus its
  `made_with`. No migration code, per the user's rule. The user may ask
  for it later.
- **Preset files carry no version** (F3 as it is). When an options type
  changes, an old file fails with a message naming the setting's path,
  exactly as a loose `.tree.yml` does today. `made_with` is only for
  that message.
- The bundle's own `version` is the author's. Procgena compares it only to
  warn when a `requires` entry differs.

## Dependencies

None is added by this plan. Proposed for B1, through the coordinator
(root `Cargo.toml`, `Cargo.lock`, `deny.toml`). Figures are from crates.io
on 2026-11-24.

| Crate | Version | Downloads (all time / recent) | Licence | In our tree now? | Notes |
|---|---|---|---|---|---|
| **`zip`** (proposed) | 9.8.2 | 361 M / 112 M | MIT | No | Default features pull in AES, bzip2, LZMA, xz, PPMd, zstd and time: use `default-features = false` plus deflate (flate2 back end; `flate2` is already in the tree through `png`). Required dependencies include `crc32fast` (already in the tree), `indexmap`, `memchr`, `typed-path`, `zeroize`, `getrandom` and `nix`/`libc`. Its docs say it works on wasm with features trimmed. **Confirm in the spike:** the deflate feature name in 9.x, whether `getrandom` and `nix` build for `wasm32-unknown-unknown`, and `with_alignment` if alignment is ever wanted. |
| **`rawzip`** (fallback) | 0.5.3 | 0.9 M / 0.6 M | MIT | No | No dependencies; reads and writes; no compression built in (pair with `flate2` for DEFLATE). Young (0.x) but small enough to audit. A good fit if `zip` doesn't build for wasm. |
| `rc-zip` | 5.4.1 | 2.6 M / 0.8 M | MIT or Apache-2.0 | No | Sans-I/O reader; pulls `chrono`, `encoding_rs`, `chardetng`, `winnow`. Read only. |
| `async_zip` | 0.0.19 | 8.3 M / 2.6 M | MIT | No | 0.0.x; Bevy's readers are async, but an in-memory slice reader needs no async zip. |
| `tar` | 0.4.46 | 302 M / 86 M | MIT or Apache-2.0 | No | Mature; for the tar row of the comparison. |
| `zstd` | 0.14.5 | 501 M / 135 M | BSD-3-Clause | No | C library; only tar.zst writing would need it. |
| `ruzstd` | 0.9.1 | 84 M / 23 M | MIT | **Yes** (through `bevy_image`) | Pure Rust decoder; its encoder is reported at the "fastest" level. Already decodes our KTX2. |
| `flate2` | 1.1.10 | 891 M / 252 M | MIT or Apache-2.0 | **Yes** (through `png`) | DEFLATE for `zip` |
| `blake3` | 1.8.7 | 238 M / 67 M | CC0-1.0 or Apache-2.0 | **Yes** (through `bevy_asset`) | Content hashes for caches |
| `memmap2` | 0.9.11 | 448 M / 127 M | MIT or Apache-2.0 | **Yes** (in `Cargo.lock`) | Optional mmap of packed bundles on desktop; plain reads are fine at our sizes |

Every licence above is on `deny.toml`'s allow list. **Where it lands:**
`procgena_bundle` (new) depends on the zip crate; `procgena_core_bevy`
(or a `bundle` feature of it) holds the mount source; generators don't
see it. Plant's wasm build sees it only if the plant web demo loads
bundles, which nothing here requires (question 9).

## Staged build plan

Bundles reuse ST2's registry and project folder and ST6's texture
catalogue, so they follow those milestones and don't run beside them.
Names are **PB0–PB5** (preset bundles).

| Stage | Covers | Needs | Done when |
|---|---|---|---|
| **PB0** (this document) | Decisions | — | The user answers the questions below |
| **PB1: format and crate** | `procgena_bundle`: open folder or zip, path rules and limits, `procgena.yml` fields, `pack` and `unpack`; a spike first on `zip` with trimmed features (desktop and `wasm32` check) and on `serde_saphyr`'s alias limits; fuzz the reader with malformed zips | ST2's project folder and `procgena.yml` (the format defines the folder's fields, so it may start as soon as ST2's file layout is fixed); the dependency through the coordinator | Tests pack a project, read it back zipped and as a folder with equal results, reject the hostile corpus (traversal, symlink, duplicate, overlap, bomb, wrong `format`), and read a Finder-made zip |
| **PB2: registry layer** | Bundle layer in ST2's registry; resolution order and qualified keys; child keys from unqualified keys; `requires` | PB1; ST2's registry and the key-clash decision (question 3) | A project Ecotope uses `alpine_pack:tall_pine`, its seeds match the same preset loaded from the project, and clashes are reported as decided |
| **PB3: textures** | A bundle's `textures.yml` as a `TextureCatalogue` source; export with chosen sizes | PB2; ST6 (catalogue, `ctt` import) | A bundle made in one project draws its bark in another project and in a game, at 128 px with no other size present |
| **PB4: Bevy mount** | `ProcgenaBundlePlugin`, `BundleMounts`, the `procgena://` source, mount from bytes on wasm | PB1 (PB2 for references) | A headless test mounts a bundle at run time and loads a tree preset, a KTX2 texture and a GLB from it, then unmounts it |
| **PB5: studio** | Open and mount bundles in the Library (a bundle filter, read-only badge, "Save as new", "Unpack to project"); the export dialog with the dependency closure | ST3 (Library), ST8 (export panel) | A headless script exports three presets with their textures, opens the bundle in a fresh project, and finds them in the Library with "used by" |
| **Later** | Models in `MeshScatter`; alignment; a `#label` loader; signed bundles | `MeshScatter` (ARCHITECTURE.md "Extensibility") | — |

## Effects on other documents

Proposed only. ST0 is active in `procgena/`, so neither file is edited
here.

- **studio_plan.md:** "The project folder" gains `id`, `version`,
  `licence`, `requires` in `procgena.yml`, and a pointer here; "The
  registry" gains the bundle layer between project and built-in; open
  question 1 is answered together with question 3 here; ST8's export
  gains "Export bundle".
- **ARCHITECTURE.md:** rule 10 gains one line, "presets travel in
  bundles: a project folder, zipped", and "Seeds" gains the rule that a
  default child key is the unqualified preset key.
- **ROADMAP.md:** adds PB1–PB5 after ST2 and ST6.
- **studio_plan.md ST6** says the studio "registers the project as one
  more asset source". Bevy fixes sources when `AssetPlugin` is built, so
  a project opened or switched at run time can't be a source of its own.
  The `procgena://` mount source covers this: the open project mounts
  under its `id` like any bundle. ST6 and PB4 should share that one
  source.

## Questions for the user

1. **Is the bundle what the studio saves into, or what it exports?**
   *Context:* you asked for "a unified saved-file format". Studio_plan.md
   already decided a git-friendly project folder, which the studio edits
   with comment-keeping updates. *Options:* (a) **the folder is the
   working form and the `.procgena` file is for sharing**: the studio
   saves into the folder; export packs it; opening a bundle mounts it
   read-only. (b) **The studio can also open a `.procgena` file as the
   project itself**, rewriting the whole archive on every save (to a
   temp file, then renamed). *What changes:* (b) gives one file to hand
   around but loses git diffs and in-place patching. It also costs a
   rewrite of tens of MB on every Update once textures are in, and
   outside-edit reload stops working. The format is the same either way.
   *Recommendation:* (a), with (b) possible later without a format change.

2. **How do Bevy games load bundles?** *Context:* Bevy fixes asset sources
   at startup, but a loader can run other loaders on in-memory bytes.
   *Options:* (a) **mount source** `procgena://<id>/<path>`: lazy, reuses
   every loader, also serves the open project (ST6); the plugin must be
   added before `DefaultPlugins`. (b) **bundle loader**
   `pack.procgena#<path>`: one asset, no plugin-order rule; everything
   read and decoded at once; entries can't load other entries by path.
   (c) both. *Recommendation:* (a) now, and (b) later only if
   games ask for it.

3. **What happens when keys clash?** (Settled together with studio_plan.md's
   open question 1.) *Context:* project, bundles and built-ins all have
   keys, and built-ins grow with each release, so a clash can appear
   after an upgrade. Default child keys come from preset keys, so the
   answer must not change seeds. *Options:* (a) **qualify only when ambiguous**:
   bare keys work, a clash is a problem in the Problems panel, and
   `bundle:key` resolves it; (b) **always qualify** references to bundle
   presets: never ambiguous, but noisier files, and moving a preset into
   a bundle edits every reference; (c) **the nearer layer wins**, with a
   warning (project over bundles over built-in): no errors, but an
   upgrade can silently change what a file means. *Recommendation:*
   (a), with child keys always unqualified.

4. **Should packing write an entry list with hashes?** *Context:* the zip
   already indexes and checks its entries with CRC-32. A list gives a
   content hash for the whole bundle (cache keys, "is this the same
   bundle") and a place for signatures later. *Options:* (a) **no list**
   (proposed); (b) the packer writes `bundle.lock.yml` (path, size,
   blake3) into the zip only, never in the folder; (c) a list in
   `procgena.yml` in both forms, kept by the studio. *What changes:* (b)
   adds a file the folder lacks, so the two forms differ slightly, and the
   reader must check it. (c) drifts whenever a file is hand-edited.
   *Recommendation:* (a) now. Choose (b) if signing or sharing through a
   registry ever comes up.

5. **Should export flatten built-in bases?** *Context:* a bundle preset
   with `base: pine_medium` stores only its differences, and built-ins
   may change in any release, since no compatibility is promised. *Options:*
   (a) **keep `base`**: small, readable files that follow improvements
   to built-ins, but may drift; (b) **flatten**: write full settings with
   no `base`, so the bundle looks the same in every release until a
   setting is renamed, at the cost of larger files that hide intent;
   (c) a checkbox at export. *Recommendation:* (c), defaulting to (a) for
   readability. Flattening is right for bundles meant to look identical
   for years.

6. **May bundles carry thumbnails and impostor bakes?** *Context:*
   thumbnails are cached by content hash, never committed (decided).
   Shipping them lets the Library show a bundle's presets before
   anything is generated. Impostor bakes are large and GPU-specific in
   quality. *Options:* (a) neither; (b) **one small thumbnail per preset**
   (`thumbnails/<kind>/<key>.png`, about 256 px, optional); (c) thumbnails
   and bakes. *Recommendation:* (b). Bakes are regenerated.

7. **YAML only, or JSON too?** *Context:* your requirement says "YAML or
   JSON". JSON is valid YAML 1.2, so `serde_saphyr` already reads a
   JSON-shaped file named `.tree.yml`. *Options:* (a) **`.yml` only**, with
   JSON content accepted inside; (b) also accept `.tree.json` names.
   *What changes:* (b) doubles every extension, loader and file-watch
   rule, and the comment-keeping Update has nothing to keep in JSON.
   *Recommendation:* (a).

8. **Which texture encodings may a bundle carry?** *Context:* BC7 KTX2 is
   what the studio makes, and it works on desktop GPUs and desktop
   WebGPU, but not on most phones. *Options:* (a) **BC7 KTX2 only**;
   (b) BC7 plus optional UASTC KTX2 beside it (the Python tool can make
   them), picked by the loader per device; (c) PNG sources as well, so
   others can re-encode. *Recommendation:* (a) for PB3, and (b) when a
   mobile or web game needs it. (c) bloats bundles; keep sources in the
   author's project.

9. **Must the plant web demo load bundles?** *Context:* it is the only
   wasm target. A bundle there means one HTTP fetch and slicing in
   memory, which works with either zip crate if it builds for wasm.
   *Options:* (a) **not required**: `procgena_bundle` is kept out of
   plant's wasm build; (b) required: PB1's spike must pass `wasm32`, which
   may favour `rawzip`. *Recommendation:* (a), but run the `wasm32` check in
   the spike anyway, since it is cheap and keeps (b) open.

10. **Which zip library?** *Context:* none of these is in the tree; each
    goes through the coordinator. The figures are in "Dependencies".
    *Options:* (a) **`zip`** (361 M downloads, MIT) with default features off
    and deflate only: the most used and fuzzed, but it has a symlink
    advisory in its history and required dependencies whose wasm build is
    unconfirmed; (b) **`rawzip` plus `flate2`** (`rawzip` has no
    dependencies, MIT, 0.9 M downloads, 0.x): small and auditable, but
    young; (c) **our own reader and writer** on `flate2` and `crc32fast`,
    both already in the tree, so no new crate: a STORE writer plus a strict
    reader is a few hundred lines, but zip64, data descriptors and fuzzing
    are ours to get right. *Recommendation:* (a), with (b) if the PB1 spike
    finds that (a) doesn't build for `wasm32` and question 9 needs it.

11. **What extension?** *Options:* `.procgena` (clear, searchable, long like
    `.unitypackage`), `.pgpack`, or `.pgz` (the brief's example, but "z"
    suggests compression we don't use). *Recommendation:* `.procgena`.
