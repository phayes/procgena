//! Loading the texture sets of a [`TextureList`] into texture arrays: one
//! array image per map (colour, normal, and the surface map), with one
//! layer per set, so a shader can pick any set by its layer number.
//!
//! Start a load with [`TextureArrayLoad::start`], then call
//! [`poll`](TextureArrayLoad::poll) each frame until it gives
//! [`TextureArrayState::Ready`]. Every set must have files of the chosen
//! size and compression under the given root folder.

use bevy::asset::{LoadState, RenderAssetUsages};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{
    TextureDataOrder, TextureViewDescriptor, TextureViewDimension,
};
use procgena_core::texture::{TextureCompression, TextureList, TextureSize};

/// Texture sets loaded as arrays: layer `i` of each array is set
/// `sets[i]`.
#[derive(Debug, Clone)]
pub struct TextureArrays {
    /// The colour images (sRGB).
    pub colour: Handle<Image>,
    /// The normal maps (OpenGL +Y, linear).
    pub normal: Handle<Image>,
    /// The third map of the list's layout: packed occlusion, roughness
    /// and height (`orh`), or roughness on its own.
    pub surface: Handle<Image>,
    /// The set ids, by layer.
    pub sets: Vec<String>,
    /// The size every layer was loaded at.
    pub size: TextureSize,
    /// The GPU memory the three arrays take, in bytes, mipmaps included.
    pub bytes: usize,
}

impl TextureArrays {
    /// The layer of set `id`, if it was loaded.
    pub fn layer(&self, id: &str) -> Option<u32> {
        self.sets.iter().position(|set| set == id).map(|i| i as u32)
    }
}

/// Where a [`TextureArrayLoad`] has got to.
#[derive(Debug, Clone)]
pub enum TextureArrayState {
    /// Some files are still loading.
    Loading,
    /// Every file loaded and the arrays are built.
    Ready(TextureArrays),
    /// A file failed to load, or the files don't match in size or format.
    Failed(String),
}

/// Texture sets being loaded into arrays.
#[derive(Debug)]
pub struct TextureArrayLoad {
    sets: Vec<String>,
    size: TextureSize,
    /// Each set's file per map, in map order.
    files: [Vec<Handle<Image>>; 3],
}

impl TextureArrayLoad {
    /// Starts loading `sets` from `list` at `size` and `compression`, from
    /// the asset folder `root` (an asset path such as
    /// `procgena_terrain://textures`).
    ///
    /// # Errors
    ///
    /// The first id that `list` doesn't have.
    pub fn start(
        asset_server: &AssetServer,
        root: &str,
        list: &TextureList,
        sets: &[String],
        size: TextureSize,
        compression: TextureCompression,
    ) -> Result<Self, String> {
        let maps = list.layout().maps();
        let mut files: [Vec<Handle<Image>>; 3] = Default::default();
        for id in sets {
            for (k, map) in maps.iter().enumerate() {
                let path = list
                    .path(id, size, map, compression)
                    .ok_or_else(|| format!("no texture set {id}"))?;
                files[k].push(asset_server.load(format!("{root}/{path}")));
            }
        }
        Ok(Self {
            sets: sets.to_vec(),
            size,
            files,
        })
    }

    /// Checks the files, and once they have all loaded, stacks them into
    /// arrays added to `images`. Call it until it stops giving
    /// [`TextureArrayState::Loading`]; after that, drop the load, which
    /// frees the single files.
    pub fn poll(
        &self,
        asset_server: &AssetServer,
        images: &mut Assets<Image>,
    ) -> TextureArrayState {
        // Wait for every file, and stop at the first failure.
        for handle in self.files.iter().flatten() {
            match asset_server.get_load_state(handle) {
                Some(LoadState::Loaded) => {}
                Some(LoadState::Failed(error)) => {
                    return TextureArrayState::Failed(error.to_string());
                }
                _ => return TextureArrayState::Loading,
            }
        }

        // Stack each map's files into one array.
        let mut arrays = Vec::with_capacity(3);
        let mut bytes = 0;
        for files in &self.files {
            let layers: Option<Vec<&Image>> = files.iter().map(|h| images.get(h)).collect();
            let Some(layers) = layers else {
                return TextureArrayState::Loading;
            };
            match stack_layers(&layers) {
                Ok(array) => {
                    bytes += array.data.as_ref().map_or(0, Vec::len);
                    arrays.push(array);
                }
                Err(error) => return TextureArrayState::Failed(error),
            }
        }
        let [colour, normal, surface] = <[Image; 3]>::try_from(arrays).expect("three maps");
        TextureArrayState::Ready(TextureArrays {
            colour: images.add(colour),
            normal: images.add(normal),
            surface: images.add(surface),
            sets: self.sets.clone(),
            size: self.size,
            bytes,
        })
    }
}

/// Stacks `layers` into one array image, in order, with a repeating,
/// filtered sampler. The array's pixels are kept for the GPU only, not on
/// the CPU. Every layer must have the same size, format and
/// number of mipmaps, and keep its pixels on the CPU (the default when
/// loading).
///
/// # Errors
///
/// A message naming the problem, if there are no layers, or they don't
/// match, or a layer's pixels aren't on the CPU.
pub fn stack_layers(layers: &[&Image]) -> Result<Image, String> {
    let first = layers.first().ok_or("no layers to stack")?;
    let descriptor = &first.texture_descriptor;

    // Lay each layer's mipmaps one after the other.
    let mut data = Vec::new();
    for (i, layer) in layers.iter().enumerate() {
        let other = &layer.texture_descriptor;
        if other.size != descriptor.size
            || other.format != descriptor.format
            || other.mip_level_count != descriptor.mip_level_count
            || other.size.depth_or_array_layers != 1
        {
            return Err(format!(
                "layer {i} is {}×{} {:?} with {} mipmaps, but layer 0 is {}×{} {:?} with {}",
                other.size.width,
                other.size.height,
                other.format,
                other.mip_level_count,
                descriptor.size.width,
                descriptor.size.height,
                descriptor.format,
                descriptor.mip_level_count,
            ));
        }
        let pixels = layer
            .data
            .as_ref()
            .ok_or_else(|| format!("layer {i} has no pixels on the CPU"))?;
        data.extend_from_slice(pixels);
    }

    // One image with as many layers, read as an array.
    let mut array = (*first).clone();
    array.data = Some(data);
    array.data_order = TextureDataOrder::LayerMajor;
    // The array is only drawn, so its pixels leave the CPU once uploaded.
    array.asset_usage = RenderAssetUsages::RENDER_WORLD;
    array.texture_descriptor.size.depth_or_array_layers = layers.len() as u32;
    array.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    array.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    Ok(array)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    fn layer(value: u8, size: u32) -> Image {
        Image::new_fill(
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[value, value, value, 255],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        )
    }

    #[test]
    fn layers_stack_in_order_as_an_array() {
        let (a, b) = (layer(10, 4), layer(20, 4));
        let array = stack_layers(&[&a, &b]).unwrap();
        assert_eq!(array.texture_descriptor.size.depth_or_array_layers, 2);
        assert_eq!(array.data_order, TextureDataOrder::LayerMajor);
        let data = array.data.unwrap();
        assert_eq!(data.len(), 2 * 4 * 4 * 4);
        assert_eq!((data[0], data[64]), (10, 20));
        let view = array.texture_view_descriptor.unwrap();
        assert_eq!(view.dimension, Some(TextureViewDimension::D2Array));
    }

    #[test]
    fn layers_of_different_sizes_do_not_stack() {
        let (a, b) = (layer(10, 4), layer(20, 8));
        assert!(stack_layers(&[&a, &b]).unwrap_err().contains("layer 1"));
        assert!(stack_layers(&[]).is_err());
    }
}
