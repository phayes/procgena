#!/usr/bin/env bash
# Prepare the texture images from fetch.sh for the rock generator.
# Outputs are KTX2 files: compressed textures that use less graphics memory.
# Existing output files are overwritten.
#
# Every texture is written in two formats:
#   uastc  Basis Universal UASTC: a compatible loader converts these images to
#          a format supported by the target GPU. For glTF, use KHR_texture_basisu;
#          for Bevy, enable `basis-universal`.
#   bc7    BC7: usable directly by GPUs that support it, commonly on desktops.
#          Check support before choosing it for mobile devices or glTF viewers.
# Both formats use Zstandard (zstd) compression to reduce file size; your
# loader needs to support this compression.
#
# Files go into one folder, assets/textures/rock/, named
# <id>-<size>-<map>-<format>.ktx2. The sizes are 512, 1k (1024) and 2k (2048)
# pixels per side; smaller sizes save memory, larger sizes keep more detail.
# The maps are:
#   color      the rock's colour. Load as sRGB for correct lighting.
#   normal     surface lighting detail. Use OpenGL (+Y) normal map settings.
#   orh        three surface properties stored in separate image channels:
#                red    ambient occlusion: darker values shade cracks and hollows
#                green  roughness: light areas look matte, dark areas polished
#                blue   height: lighter values mark raised areas for blending
#                       textures; scaled from 0 to 1, not a physical distance
# Load normal and orh maps as linear data, without colour correction.
#
# Each file includes mipmaps (smaller copies of the texture) to reduce
# shimmering when a rock is viewed from a distance. Run manifest.sh afterwards
# to list the files for download.
#
# Install ImageMagick 7 (`magick`), KTX-Software 4.4+ (`ktx`), and ctt-cli 0.6
# (`cargo install ctt-cli --version 0.6.1 --locked`) before running.
# Usage: tools/textures/convert.sh [SOURCE_DIR [ID...]]
# SOURCE_DIR defaults to prior_art/polyhaven in the repo, where fetch.sh saves
# images. To convert only selected textures, supply a source directory followed
# by their ids. Otherwise, every texture in tools/textures/sets.txt is converted.

set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
src=${1:-$repo/prior_art/polyhaven}
out=$repo/assets/textures
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

# Convert the selected textures, or the full list if no ids were supplied.
sets=("${@:2}")
if [[ ${#sets[@]} -eq 0 ]]; then
    while IFS= read -r line; do
        [[ -z $line || $line == \#* ]] || sets+=("${line%%[[:space:]]*}")
    done <"$repo/tools/textures/sets.txt"
fi
sizes=(512:512 1k:1024 2k:2048)

# Save a prepared PNG in both UASTC and BC7 formats. Arguments: PNG path,
# output path without the format and extension, colour handling (srgb for
# colour images, linear for normal and orh maps), and UASTC size
# optimisation (yes|no).
encode() {
    local png=$1 prefix=$2 space=$3 rdo=$4

    # Mark colour images as sRGB and keep normal and orh values as linear data.
    local format
    if [[ $space == srgb ]]; then format=R8G8B8_SRGB; else format=R8G8B8_UNORM; fi
    local rdo_args=()
    if [[ $rdo == yes ]]; then rdo_args=(--uastc-rdo); fi
    mkdir -p "$(dirname "$prefix")"
    ktx create --format "$format" --assign-tf "$space" \
        --encode uastc --uastc-quality 3 "${rdo_args[@]}" --zstd 19 \
        --generate-mipmap --mipmap-filter lanczos4 \
        "$png" "$prefix-uastc.ktx2"

    # Use the same colour handling for the BC7 version.
    ctt "$png" -o "$prefix-bc7.ktx2" --format bc7 --quality slow \
        --input-color-space "$space" --output-color-space "$space" \
        --mipmap --mipmap-filter lanczos3 --zstd=19
}

for id in "${sets[@]}"; do
    for size in "${sizes[@]}"; do
        name=${size%%:*} px=${size#*:}
        base=$stage/$id-$name

        # Resize the images for this output size using temporary PNG files.
        # Resize colour in linear light to preserve average brightness.
        magick "$src/$id/color.jpg" -colorspace RGB -filter Lanczos -resize "${px}x${px}" \
            -colorspace sRGB -alpha off -type TrueColor "PNG24:$base-color.png"
        magick "$src/$id/normal.jpg" -filter Lanczos -resize "${px}x${px}" \
            -alpha off -type TrueColor "PNG24:$base-normal.png"

        # Store occlusion in red, roughness in green and height in blue. Read
        # each source's red channel as greyscale data. Stretch height to 0-1,
        # clipping the darkest and lightest 0.5% before resizing.
        magick \
            \( "$src/$id/occlusion.jpg" -channel R -separate +channel \) \
            \( "$src/$id/roughness.jpg" -channel R -separate +channel \) \
            \( "$src/$id/height.jpg" -channel R -separate +channel -contrast-stretch 0.5%x0.5% \) \
            -filter Lanczos -resize "${px}x${px}" -set colorspace sRGB -combine \
            -alpha off -type TrueColor "PNG24:$base-orh.png"

        prefix=$out/rock/$id-$name
        encode "$base-color.png" "$prefix-color" srgb yes
        encode "$base-normal.png" "$prefix-normal" linear no
        encode "$base-orh.png" "$prefix-orh" linear no
    done
done

cp "$repo/tools/textures/LICENSE.md" "$out/LICENSE.md"
echo "Wrote $out"
