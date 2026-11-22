#!/usr/bin/env bash
# Prepare the texture images from fetch.sh for clumps.
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
# Files go into one folder per material, named by the start of the texture id
# (snow_02 goes into assets/textures/snow/), as
# <material>/<id>-<size>-<map>-<format>.ktx2. The sizes are 512, 1k (1024)
# and 2k (2048) pixels per side; smaller sizes save memory, larger sizes keep
# more detail. The maps are:
#   color      the surface colour. Load as sRGB for correct lighting.
#   normal     surface lighting detail. Use OpenGL (+Y) normal map settings.
#   orh        three surface properties stored in separate image channels:
#                red    ambient occlusion: darker values shade cracks and hollows
#                green  roughness: light areas look matte, dark areas polished
#                blue   height: lighter values mark raised areas for blending
#                       textures; scaled from 0 to 1, not a physical distance
# Load normal and orh maps as linear data, without colour correction.
#
# Stalk cards (twigs, sporophytes) listed in tools/textures/stalks.txt go into
# assets/textures/stalks/<kind>-<map>-<format>.ktx2 at their source size,
# with these maps:
#   color      colour with transparency (straight alpha). Load as sRGB.
#   normal     lighting detail, OpenGL (+Y), when the source has one.
# Their sources are read from prior_art/ as described in stalks.txt.
#
# Each file includes mipmaps (smaller copies of the texture) to reduce
# shimmering when a clump is viewed from a distance. Run manifest.sh afterwards
# to list the files for download.
#
# Install ImageMagick 7 (`magick`), KTX-Software 4.4+ (`ktx`), and ctt-cli 0.6
# (`cargo install ctt-cli --version 0.6.1 --locked`) before running.
# Usage: tools/textures/convert.sh [SOURCE_DIR [ID...]]
# SOURCE_DIR defaults to prior_art/textures in the repo, where fetch.sh saves
# images. To convert only selected textures, supply a source directory followed
# by their ids; stalk cards are then skipped. Otherwise, every texture in
# tools/textures/sets.txt and every stalk card in stalks.txt is converted.

set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
src=${1:-$repo/prior_art/textures}
prior_art=$repo/prior_art
out=$repo/assets/textures
source "$repo/tools/textures/stalks.sh"
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
# colour images, linear for normal and orh maps), UASTC size optimisation
# (yes|no), and optionally the channels (rgb, the default, or rgba to keep
# transparency).
encode() {
    local png=$1 prefix=$2 space=$3 rdo=$4 channels=${5:-rgb}

    # Mark colour images as sRGB and keep normal and orh values as linear data.
    local format
    case "$channels-$space" in
        rgb-srgb) format=R8G8B8_SRGB ;;
        rgb-linear) format=R8G8B8_UNORM ;;
        rgba-srgb) format=R8G8B8A8_SRGB ;;
        *) echo "unsupported $channels-$space" >&2; exit 1 ;;
    esac
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
    material=${id%%_*}
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

        prefix=$out/$material/$id-$name
        encode "$base-color.png" "$prefix-color" srgb yes
        encode "$base-normal.png" "$prefix-normal" linear no
        encode "$base-orh.png" "$prefix-orh" linear no
    done
done

# Prepare each stalk card's colour image and any normal map at its source size,
# then write both GPU formats. Stalk cards are skipped when specific ids were
# supplied.
if [[ $# -lt 2 ]]; then
    while IFS=$'\t' read -r kind source path _; do
        prepare_stalk "$kind" "$source" "$prior_art/$path" "$stage/stalks"
        prefix=$out/stalks/$kind
        encode "$stage/stalks/$kind-color.png" "$prefix-color" srgb yes rgba
        if [[ -f $stage/stalks/$kind-normal.png ]]; then
            encode "$stage/stalks/$kind-normal.png" "$prefix-normal" linear no
        fi
    done < <(stalk_list "$repo")
fi

cp "$repo/tools/textures/LICENSE.md" "$out/LICENSE.md"
echo "Wrote $out"
