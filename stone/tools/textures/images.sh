#!/usr/bin/env bash
# Writes the texture images for exported models (glTF, GLB and OBJ files), which
# need ordinary JPEG images rather than the KTX2 files used for rendering.
# Also writes assets/textures/images/images_manifest.json, which lists every
# image with its download address in the public gs://ze_textures bucket, under
# stone/images/. Existing output is overwritten.
#
# Images are written as images/rock/<id>-<size>-<map>.jpg in three sizes, 512,
# 1k and 2k pixels per side. The maps are:
#   color      surface colour, encoded as sRGB
#   normal     lighting detail, using OpenGL (+Y) normal maps
#   orm        red shades cracks and hollows (ambient occlusion); green
#              controls roughness (light is matte, dark is polished); blue
#              is metalness (light is bare metal), zero for sets without a
#              metalness map such as rock. Load as linear data.
#
# Each manifest entry gives the file's path, size and map, width and height in
# pixels, byte count, and a SHA-256 checksum to confirm a download is complete.
# Each texture also lists its title, authors, source page and licence.
#
# The script stops with an error if a source image is missing.
#
# Install ImageMagick 7 (`magick`), jq, and either shasum (macOS) or sha256sum.
# Usage: tools/textures/images.sh [SOURCE_DIR]
# SOURCE_DIR defaults to prior_art/polyhaven, where fetch.sh saves the textures
# listed in tools/textures/sets.txt.
#
# Upload the result with:
#   gcloud storage rsync --recursive assets/textures/images gs://ze_textures/stone/images

set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
src=${1:-$repo/prior_art/polyhaven}
out=$repo/assets/textures/images
base_url='https://storage.googleapis.com/ze_textures/stone/images/'
sizes=(512:512 1k:1024 2k:2048)

# Get a file's SHA-256 checksum.
sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# Describe one written image as a JSON object. Arguments: path inside $out,
# then extra fields as a JSON object.
describe() {
    local rel=$1 extra=$2 file=$out/$1
    local width height
    read -r width height < <(magick identify -format '%w %h\n' "$file" | head -1)
    jq -nc --arg path "$rel" --argjson extra "$extra" --argjson width "$width" \
        --argjson height "$height" --argjson bytes "$(wc -c <"$file" | tr -d ' ')" \
        --arg sha256 "$(sha256 "$file")" \
        '{path: $path} + $extra + {width: $width, height: $height, bytes: $bytes, sha256: $sha256}'
}

# Load the texture ids, the first word of each line; ignore comments and
# empty lines.
ids=()
while IFS= read -r line; do
    [[ -z $line || $line == \#* ]] || ids+=("${line%%[[:space:]]*}")
done <"$repo/tools/textures/sets.txt"

mkdir -p "$out/rock"

# Fit each map within px × px as JPEG and describe each file.
assets=$(for id in "${ids[@]}"; do
    for map in color normal roughness occlusion; do
        if [[ ! -f $src/$id/$map.jpg ]]; then
            echo "missing $src/$id/$map.jpg" >&2
            exit 1
        fi
    done
    for size in "${sizes[@]}"; do
        name=${size%%:*} px=${size#*:}
        prefix=$out/rock/$id-$name

        # Resize colour in linear light to preserve average brightness. Keep
        # full colour resolution (4:4:4) so normal directions are not blurred.
        magick "$src/$id/color.jpg" -colorspace RGB -filter Lanczos -resize "${px}x${px}" \
            -colorspace sRGB -strip -quality 92 "$prefix-color.jpg"
        magick "$src/$id/normal.jpg" -filter Lanczos -resize "${px}x${px}" \
            -strip -sampling-factor 4:4:4 -quality 95 "$prefix-normal.jpg"

        # Store occlusion in red, roughness in green and metalness in blue for
        # glTF. Sets without a metalness map get zero metalness. Keep full
        # colour resolution (4:4:4) to preserve detail in these separate
        # surface properties.
        metal_map=() metal_zero=()
        if [[ -f $src/$id/metalness.jpg ]]; then
            metal_map=('(' "$src/$id/metalness.jpg" -channel R -separate +channel ')')
        else
            metal_zero=('(' -clone 0 -evaluate set 0 ')')
        fi
        magick \
            \( "$src/$id/occlusion.jpg" -channel R -separate +channel \) \
            \( "$src/$id/roughness.jpg" -channel R -separate +channel \) \
            "${metal_map[@]}" \
            -filter Lanczos -resize "${px}x${px}" \
            "${metal_zero[@]}" \
            -set colorspace sRGB -combine -alpha off -type TrueColor \
            -strip -sampling-factor 4:4:4 -quality 95 "$prefix-orm.jpg"

        for map in color normal orm; do
            describe "rock/$id-$name-$map.jpg" \
                "$(jq -nc --arg size "$name" --arg map "$map" '{size: $size, map: $map}')"
        done
    done | jq -s --arg id "$id" --slurpfile info "$src/$id/info.json" \
        '{id: $id, category: "rock"} + $info[0] + {files: .}'
done | jq -s '.')

# Write the manifest with a short description of the files.
jq -n --arg base_url "$base_url" --argjson assets "$assets" '{
    version: 1,
    baseUrl: $base_url,
    pathPattern: "<category>/<id>-<size>-<map>.jpg",
    sizes: {
        "512": "512 x 512 pixels",
        "1k": "1024 x 1024 pixels",
        "2k": "2048 x 2048 pixels"
    },
    maps: {
        color: "surface colour, sRGB",
        normal: "surface bumps, OpenGL (+Y); load as linear data",
        orm: "R occlusion (shades hollows), G roughness (light is matte), B metalness (light is metal, zero for rock); linear data"
    },
    assets: $assets
}' >"$out/images_manifest.json"
cp "$repo/assets/textures/LICENSE.md" "$out/LICENSE.md"
echo "Wrote $out"
