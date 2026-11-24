//! Impostors on disk: a read-only folder shipped with a game, and a
//! writable cache folder that bakes are saved to.
//!
//! Each impostor is named after its key: `<key>.txt` with its layout and
//! bounds, and its colour and normal atlases as `<key>.albedo.png` and
//! `<key>.normal.png`, or, after [`compress_impostors`], as
//! `<key>.albedo.ktx2` and `<key>.normal.ktx2`. Files are written under
//! temporary names and renamed into place, so another process never reads
//! half a file.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormatSupport, CompressedImageFormats, ImageSampler, ImageSamplerDescriptor,
    ImageType,
};
use bevy::math::Vec3;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};

use crate::ImpostorSettings;
use crate::bake::{Impostor, Impostors};
use crate::key::ImpostorKey;
use crate::ktx2::encode_bc7;
use crate::material::ImpostorMaterial;
use crate::mips::{MipChains, mip_chains};

/// An impostor as stored: its layout, bounds and full-size atlas pixels
/// (RGBA, 8 bits per channel, rows from the top).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredImpostor {
    pub(crate) frames: u32,
    /// Pixels along each side of each atlas.
    pub(crate) size: u32,
    pub(crate) center: Vec3,
    pub(crate) radius: f32,
    pub(crate) albedo: Vec<u8>,
    pub(crate) normal: Vec<u8>,
}

/// An impostor read from disk, its atlases ready for the GPU with every
/// mip level.
struct LoadedImpostor {
    frames: u32,
    center: Vec3,
    radius: f32,
    albedo: Image,
    normal: Image,
}

/// An impostor's layout and bounds, from its `.txt` file.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    frames: u32,
    size: u32,
    center: Vec3,
    radius: f32,
}

/// Version of the `.txt` layout; files of another version are ignored.
const FORMAT: &str = "procgena_impostor 1";

/// The file holding `name`'s `suffix` in `dir`; `name` is a key as text.
fn path(dir: &Path, name: &str, suffix: &str) -> PathBuf {
    dir.join(format!("{name}.{suffix}"))
}

/// Reads the layout stored for `name` in `dir`, or `None` if it isn't
/// there, can't be read, or is of another format version.
fn read_layout(dir: &Path, name: &str) -> Option<Layout> {
    let text = fs::read_to_string(path(dir, name, "txt")).ok()?;
    let mut lines = text.lines();
    if lines.next()? != FORMAT {
        return None;
    }
    let mut field = |field: &str| -> Option<Vec<f32>> {
        let line = lines.next()?;
        let rest = line.strip_prefix(field)?.strip_prefix(' ')?;
        rest.split(' ').map(|v| v.parse().ok()).collect()
    };
    let frames = *field("frames")?.first()? as u32;
    let size = *field("size")?.first()? as u32;
    let center = field("center")?;
    let radius = *field("radius")?.first()?;
    Some(Layout {
        frames,
        size,
        center: Vec3::new(*center.first()?, *center.get(1)?, *center.get(2)?),
        radius,
    })
}

/// Writes `contents` to `target` under a temporary name, then renames it
/// into place.
fn write_atomically(target: &Path, contents: &[u8]) -> io::Result<()> {
    let mut temporary = target.as_os_str().to_owned();
    temporary.push(format!(".{}.tmp", std::process::id()));
    fs::write(&temporary, contents)?;
    fs::rename(&temporary, target)
}

/// Writes `layout` for `name` into `dir`. Write it after the atlases, so
/// a reader sees either the whole impostor or none of it.
fn write_layout(dir: &Path, name: &str, layout: Layout) -> io::Result<()> {
    let c = layout.center;
    let text = format!(
        "{FORMAT}\nframes {}\nsize {}\ncenter {} {} {}\nradius {}\n",
        layout.frames, layout.size, c.x, c.y, c.z, layout.radius
    );
    write_atomically(&path(dir, name, "txt"), text.as_bytes())
}

/// Decodes `name`'s PNG atlas with `suffix` in `dir`, if it is `size`
/// pixels square.
fn read_png(dir: &Path, name: &str, suffix: &str, size: u32) -> Option<Vec<u8>> {
    let image = image::open(path(dir, name, suffix)).ok()?.into_rgba8();
    (image.width() == size && image.height() == size).then(|| image.into_raw())
}

/// Reads the impostor stored as PNGs for `name` in `dir`, or `None` if it
/// isn't there or can't be read.
fn read_named(dir: &Path, name: &str) -> Option<StoredImpostor> {
    let layout = read_layout(dir, name)?;
    Some(StoredImpostor {
        frames: layout.frames,
        size: layout.size,
        center: layout.center,
        radius: layout.radius,
        albedo: read_png(dir, name, "albedo.png", layout.size)?,
        normal: read_png(dir, name, "normal.png", layout.size)?,
    })
}

/// Reads the impostor stored as PNGs for `key` in `dir`, or `None` if it
/// isn't there or can't be read.
#[cfg(test)]
fn read(dir: &Path, key: ImpostorKey) -> Option<StoredImpostor> {
    read_named(dir, &key.to_string())
}

/// Reads the impostor stored for `key` in `dir` and prepares its atlases
/// for the GPU: the compressed files when `bc7` is set and they are there,
/// otherwise the PNGs with mip levels made for them.
fn load(dir: &Path, key: ImpostorKey, bc7: bool) -> Option<LoadedImpostor> {
    let name = key.to_string();
    let layout = read_layout(dir, &name)?;
    let compressed = bc7
        .then(|| {
            let albedo = read_ktx2(dir, &name, "albedo.ktx2", layout.size, true)?;
            let normal = read_ktx2(dir, &name, "normal.ktx2", layout.size, false)?;
            Some((albedo, normal))
        })
        .flatten();
    let (albedo, normal) = match compressed {
        Some(atlases) => atlases,
        None => {
            let albedo = read_png(dir, &name, "albedo.png", layout.size)?;
            let normal = read_png(dir, &name, "normal.png", layout.size)?;
            let chains = mip_chains(&albedo, &normal, layout.size, layout.frames);
            atlas_images(layout.size, chains)
        }
    };
    Some(LoadedImpostor {
        frames: layout.frames,
        center: layout.center,
        radius: layout.radius,
        albedo,
        normal,
    })
}

/// Reads `name`'s compressed atlas with `suffix` in `dir`, if it is `size`
/// pixels square.
fn read_ktx2(dir: &Path, name: &str, suffix: &str, size: u32, srgb: bool) -> Option<Image> {
    let bytes = fs::read(path(dir, name, suffix)).ok()?;
    let mut image = Image::from_buffer(
        &bytes,
        ImageType::Extension("ktx2"),
        CompressedImageFormats::BC,
        srgb,
        ImageSampler::Descriptor(ImageSamplerDescriptor::linear()),
        RenderAssetUsages::RENDER_WORLD,
    )
    .ok()?;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    (image.width() == size && image.height() == size).then_some(image)
}

/// Writes `stored` for `key` into `dir` as PNGs, creating `dir` if needed.
/// The `.txt` comes last, so a reader sees either the whole impostor or
/// none of it.
pub(crate) fn write(dir: &Path, key: ImpostorKey, stored: &StoredImpostor) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let name = key.to_string();
    let encode = |suffix: &str, pixels: &[u8]| -> io::Result<()> {
        let mut png = Vec::new();
        image::write_buffer_with_format(
            &mut io::Cursor::new(&mut png),
            pixels,
            stored.size,
            stored.size,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(io::Error::other)?;
        write_atomically(&path(dir, &name, suffix), &png)
    };
    encode("albedo.png", &stored.albedo)?;
    encode("normal.png", &stored.normal)?;
    write_layout(dir, &name, stored_layout(stored))
}

fn stored_layout(stored: &StoredImpostor) -> Layout {
    Layout {
        frames: stored.frames,
        size: stored.size,
        center: stored.center,
        radius: stored.radius,
    }
}

/// Compresses every impostor saved as PNGs in `from` (a
/// [`cache_dir`](ImpostorSettings::cache_dir)) into `to`, ready to ship as
/// a [`shipped_dir`](ImpostorSettings::shipped_dir): both atlases as KTX2
/// files of BC7 texels with mip levels, beside a copy of each `.txt`.
/// `to` is created if needed, and may be `from` itself.
///
/// Compressed atlases take a quarter of the GPU memory, load without
/// decoding, and look smoother far away. They are used only on GPUs that
/// support BC7 (every desktop GPU); elsewhere a shipped folder that holds
/// only compressed files is skipped, and those impostors are baked.
///
/// Returns how many impostors were compressed. This takes a moment per
/// impostor, so run it offline or behind a loading screen, not every frame.
///
/// # Errors
///
/// Fails if a folder can't be read or written, or an impostor's atlases
/// aren't a whole number of 4 × 4 blocks (the settings'
/// `frames × frame_size` isn't a multiple of 4). Impostors whose files are
/// damaged or of another version are skipped.
pub fn compress_impostors(from: &Path, to: &Path) -> io::Result<usize> {
    fs::create_dir_all(to)?;
    let mut compressed = 0;
    for entry in fs::read_dir(from)? {
        let entry_path = entry?.path();
        if entry_path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let Some(name) = entry_path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(stored) = read_named(from, name) else {
            continue;
        };

        // Mip levels for both atlases, then each compressed into a file.
        let chains = mip_chains(&stored.albedo, &stored.normal, stored.size, stored.frames);
        for (suffix, levels, srgb) in [
            ("albedo.ktx2", &chains.albedo, true),
            ("normal.ktx2", &chains.normal, false),
        ] {
            let file = encode_bc7(levels, stored.size, srgb).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "impostor {name}: atlases of {} pixels aren't whole 4 × 4 blocks",
                        stored.size
                    ),
                )
            })?;
            write_atomically(&path(to, name, suffix), &file)?;
        }
        write_layout(to, name, stored_layout(&stored))?;
        compressed += 1;
    }
    Ok(compressed)
}

/// GPU images of an impostor's colour (sRGB) and normal (linear) atlases
/// from their mip chains, sampled smoothly, kept only on the GPU.
pub(crate) fn atlas_images(size: u32, chains: MipChains) -> (Image, Image) {
    (
        atlas_image(size, chains.albedo, true),
        atlas_image(size, chains.normal, false),
    )
}

/// A GPU image from `levels`, RGBA pixels from `size` pixels square down,
/// halving each time.
fn atlas_image(size: u32, levels: Vec<Vec<u8>>, srgb: bool) -> Image {
    let format = if srgb {
        TextureFormat::Rgba8UnormSrgb
    } else {
        TextureFormat::Rgba8Unorm
    };
    // Made from the full-size level, then given every level.
    let level_count = levels.len() as u32;
    let all_levels = levels.concat();
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        levels.into_iter().next().unwrap_or_default(),
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(all_levels);
    image.texture_descriptor.mip_level_count = level_count;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

/// Impostors being read from disk, and keys known not to be there.
#[derive(Resource, Default)]
pub(crate) struct DiskReads {
    reading: HashMap<ImpostorKey, Task<Option<LoadedImpostor>>>,
    missing: HashSet<ImpostorKey>,
}

impl DiskReads {
    /// Whether `key` should wait for the disk: it is being read, or hasn't
    /// been looked for yet, in which case this starts reading it from the
    /// shipped folder, then the cache folder. `bc7` says whether the GPU
    /// can use compressed atlases.
    pub(crate) fn wait_for(
        &mut self,
        key: ImpostorKey,
        settings: &ImpostorSettings,
        bc7: bool,
    ) -> bool {
        let dirs: Vec<PathBuf> = [&settings.shipped_dir, &settings.cache_dir]
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        if dirs.is_empty() || self.missing.contains(&key) {
            return false;
        }
        self.reading.entry(key).or_insert_with(|| {
            IoTaskPool::get().spawn(async move { dirs.iter().find_map(|dir| load(dir, key, bc7)) })
        });
        true
    }
}

/// Whether the GPU can sample BC7 atlases. False without a GPU.
pub(crate) fn supports_bc7(support: Option<&CompressedImageFormatSupport>) -> bool {
    support.is_some_and(|s| s.0.contains(CompressedImageFormats::BC))
}

/// Keeps impostors that finished reading from disk, and remembers keys that
/// weren't there so they are baked.
pub(crate) fn finish_reads(
    mut reads: ResMut<DiskReads>,
    mut impostors: ResMut<Impostors>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<ImpostorMaterial>>,
) {
    let reads = &mut *reads;
    let mut finished = Vec::new();
    for (key, task) in &mut reads.reading {
        if let Some(result) = block_on(poll_once(task)) {
            finished.push((*key, result));
        }
    }
    for (key, result) in finished {
        reads.reading.remove(&key);
        let Some(loaded) = result else {
            reads.missing.insert(key);
            continue;
        };
        let impostor = Impostor {
            albedo: images.add(loaded.albedo),
            normal: images.add(loaded.normal),
            frames: loaded.frames,
            coverage: key.bake().coverage,
            center: loaded.center,
            radius: loaded.radius,
        };
        impostors.keep(key, impostor, &mut materials);
    }
}

/// Baked impostors being finished: atlases read back from the GPU so far,
/// and mip levels being made and files written in the background.
#[derive(Resource, Default)]
pub struct ImpostorWrites {
    readbacks: HashMap<ImpostorKey, PendingSave>,
    finishing: Vec<(ImpostorKey, Task<Finished>)>,
}

/// A baked impostor waiting for its atlases to be read back.
struct PendingSave {
    impostor: Impostor,
    /// Pixels along each side of each atlas.
    size: u32,
    /// The colour and normal atlases read back so far.
    atlases: [Option<Vec<u8>>; 2],
}

/// A baked impostor's layout, mip levels, and whether its files were
/// written.
struct Finished {
    stored: StoredImpostor,
    chains: MipChains,
    written: io::Result<()>,
}

impl ImpostorWrites {
    /// How many baked impostors are still being finished or saved. Wait for
    /// 0 before exiting a pre-baking app.
    pub fn pending(&self) -> usize {
        self.readbacks.len() + self.finishing.len()
    }

    /// Whether `key`'s bake is being finished.
    pub(crate) fn finishing(&self, key: ImpostorKey) -> bool {
        self.readbacks.contains_key(&key) || self.finishing.iter().any(|(k, _)| *k == key)
    }
}

/// Starts finishing a bake: reads both atlases back from the GPU, then in
/// the background makes their mip levels and, with a
/// [`cache_dir`](ImpostorSettings::cache_dir), writes the files.
/// [`finish_writes`] then keeps the impostor, with new images holding
/// every mip level.
pub(crate) fn save(
    commands: &mut Commands,
    writes: &mut ImpostorWrites,
    key: ImpostorKey,
    impostor: &Impostor,
    size: u32,
) {
    writes.readbacks.insert(
        key,
        PendingSave {
            impostor: impostor.clone(),
            size,
            atlases: [None, None],
        },
    );
    for (index, atlas) in [&impostor.albedo, &impostor.normal].into_iter().enumerate() {
        commands.spawn(Readback::texture(atlas.clone())).observe(
            move |event: On<ReadbackComplete>,
                  mut commands: Commands,
                  mut writes: ResMut<ImpostorWrites>,
                  settings: Res<ImpostorSettings>| {
                // Readbacks already in flight may still report after the
                // first, once the entity is gone; only the first is used.
                commands.entity(event.entity).try_despawn();
                let Some(pending) = writes.readbacks.get_mut(&key) else {
                    return;
                };
                pending.atlases[index] = Some(unpad(&event.data, pending.size));
                if pending.atlases.iter().any(Option::is_none) {
                    return;
                }
                let Some(pending) = writes.readbacks.remove(&key) else {
                    return;
                };
                let [Some(albedo), Some(normal)] = pending.atlases else {
                    return;
                };
                let stored = StoredImpostor {
                    frames: pending.impostor.frames,
                    size: pending.size,
                    center: pending.impostor.center,
                    radius: pending.impostor.radius,
                    albedo,
                    normal,
                };
                let dir = settings.cache_dir.clone();
                let task = IoTaskPool::get().spawn(async move {
                    let written = match dir {
                        Some(dir) => write(&dir, key, &stored),
                        None => Ok(()),
                    };
                    let chains =
                        mip_chains(&stored.albedo, &stored.normal, stored.size, stored.frames);
                    Finished {
                        stored,
                        chains,
                        written,
                    }
                });
                writes.finishing.push((key, task));
            },
        );
    }
}

/// Keeps finished bakes, with images holding every mip level, so waiting
/// requests are answered. Logs any files that couldn't be written.
pub(crate) fn finish_writes(
    mut writes: ResMut<ImpostorWrites>,
    mut impostors: ResMut<Impostors>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<ImpostorMaterial>>,
) {
    writes
        .finishing
        .retain_mut(|(key, task)| match block_on(poll_once(task)) {
            None => true,
            Some(finished) => {
                if let Err(error) = finished.written {
                    error!("procgena_impostor: cannot save impostor {key}: {error}");
                }
                let stored = finished.stored;
                let (albedo, normal) = atlas_images(stored.size, finished.chains);
                let impostor = Impostor {
                    albedo: images.add(albedo),
                    normal: images.add(normal),
                    frames: stored.frames,
                    coverage: key.bake().coverage,
                    center: stored.center,
                    radius: stored.radius,
                };
                impostors.keep(*key, impostor, &mut materials);
                false
            }
        });
}

/// Copies an atlas read back from the GPU into tightly packed rows,
/// dropping the padding the GPU adds to each row.
pub(crate) fn unpad(data: &[u8], size: u32) -> Vec<u8> {
    let row = size as usize * 4;
    let padded = row.div_ceil(256) * 256;
    (0..size as usize)
        .flat_map(|y| &data[y * padded..y * padded + row])
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty folder for one test.
    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("procgena_impostor_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn stored(size: u32, shade: u8) -> StoredImpostor {
        let pixels = (0..size * size * 4)
            .map(|i| (i as u8).wrapping_add(shade))
            .collect();
        StoredImpostor {
            frames: 2,
            size,
            center: Vec3::new(0.5, 3.25, -1.0),
            radius: 4.5,
            albedo: pixels,
            normal: vec![shade; (size * size * 4) as usize],
        }
    }

    /// The names of the files in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn impostors_round_trip_through_their_files() {
        let dir = folder("round_trip");
        let key = ImpostorKey::test(1);
        assert_eq!(read(&dir, key), None, "nothing is stored yet");
        let impostor = stored(8, 7);
        write(&dir, key, &impostor).unwrap();
        assert_eq!(read(&dir, key), Some(impostor));

        // No temporary files are left behind.
        let names = names(&dir);
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
        assert_eq!(names.len(), 3);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn damaged_or_foreign_files_are_ignored() {
        let dir = folder("damaged");
        let key = ImpostorKey::test(2);
        write(&dir, key, &stored(4, 1)).unwrap();

        // A layout from another format version.
        let meta = path(&dir, &key.to_string(), "txt");
        let text = fs::read_to_string(&meta).unwrap();
        fs::write(&meta, text.replace(FORMAT, "procgena_impostor 0")).unwrap();
        assert_eq!(read(&dir, key), None);

        // An atlas of the wrong size.
        write(&dir, key, &stored(4, 1)).unwrap();
        let other = ImpostorKey::test(3);
        write(&dir, other, &stored(8, 1)).unwrap();
        fs::copy(
            path(&dir, &other.to_string(), "albedo.png"),
            path(&dir, &key.to_string(), "albedo.png"),
        )
        .unwrap();
        assert_eq!(read(&dir, key), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn loaded_pngs_gain_mip_levels() {
        let dir = folder("png_mips");
        let key = ImpostorKey::test(4);
        // Views of 32 pixels: levels of 32, 16 and 8 pixels a view.
        write(&dir, key, &stored(64, 3)).unwrap();
        for bc7 in [false, true] {
            let loaded = load(&dir, key, bc7).expect("the PNGs are read");
            assert_eq!(loaded.radius, 4.5);
            for image in [&loaded.albedo, &loaded.normal] {
                assert_eq!(image.texture_descriptor.mip_level_count, 3);
                assert_eq!(
                    image.data.as_ref().unwrap().len(),
                    (64 * 64 + 32 * 32 + 16 * 16) * 4
                );
            }
            assert_eq!(
                loaded.albedo.texture_descriptor.format,
                TextureFormat::Rgba8UnormSrgb
            );
            assert_eq!(
                loaded.normal.texture_descriptor.format,
                TextureFormat::Rgba8Unorm
            );
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn compressed_impostors_are_preferred_where_the_gpu_supports_them() {
        let cache = folder("compress_from");
        let shipped = folder("compress_to");
        let key = ImpostorKey::test(5);
        write(&cache, key, &stored(64, 3)).unwrap();
        fs::write(cache.join("notes.md"), "not an impostor").unwrap();
        assert_eq!(compress_impostors(&cache, &shipped).unwrap(), 1);
        let name = key.to_string();
        assert_eq!(
            names(&shipped),
            [
                format!("{name}.albedo.ktx2"),
                format!("{name}.normal.ktx2"),
                format!("{name}.txt"),
            ]
        );

        // Used compressed, with every mip level, when BC7 is supported.
        let loaded = load(&shipped, key, true).expect("the compressed files are read");
        assert_eq!(
            loaded.albedo.texture_descriptor.format,
            TextureFormat::Bc7RgbaUnormSrgb
        );
        assert_eq!(
            loaded.normal.texture_descriptor.format,
            TextureFormat::Bc7RgbaUnorm
        );
        assert_eq!(loaded.albedo.texture_descriptor.mip_level_count, 3);
        assert_eq!(loaded.center, Vec3::new(0.5, 3.25, -1.0));

        // Skipped otherwise, since there are no PNGs beside them.
        assert!(load(&shipped, key, false).is_none());

        // Atlases that aren't whole blocks can't be compressed.
        let odd = ImpostorKey::test(6);
        write(&cache, odd, &stored(6, 3)).unwrap();
        assert!(compress_impostors(&cache, &shipped).is_err());
        fs::remove_dir_all(&cache).unwrap();
        fs::remove_dir_all(&shipped).unwrap();
    }

    #[test]
    fn padded_rows_are_packed() {
        // 2 pixels a row is 8 bytes, padded to 256.
        let mut data = vec![0u8; 512];
        data[..8].copy_from_slice(&[1; 8]);
        data[256..264].copy_from_slice(&[2; 8]);
        assert_eq!(unpad(&data, 2), [[1u8; 8], [2; 8]].concat());
    }
}
