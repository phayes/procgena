#!/usr/bin/env bash
# Writes the texture images for exported models (glTF, GLB and OBJ files), which
# need ordinary JPEG images rather than the KTX2 files used for rendering.
# Also writes assets/textures/images/images_manifest.json, which lists every
# image with its download address in the public gs://ze_textures bucket, under
# clump/images/. Existing output is overwritten.
#
# Images are written as images/<material>/<id>-<size>-<map>.jpg, where the
# material is the start of the texture id (snow_02 goes into images/snow/), in
# three sizes, 512, 1k and 2k pixels per side. The maps are:
#   color      surface colour, encoded as sRGB
#   normal     lighting detail, using OpenGL (+Y) normal maps
#   orm        red shades cracks and hollows (ambient occlusion); green
#              controls roughness (light is matte, dark is polished); blue
#              is metalness, always zero for clump materials. Load as linear
#              data.
#
# Stalk cards from tools/textures/stalks.txt are written as
# images/stalks/<kind>-<map>.png at their source size: color (sRGB with
# straight-alpha transparency) and, when the source has one, normal (OpenGL
# +Y). Their sources are read from prior_art/ as described in stalks.txt.
#
# Each manifest entry gives the file's path, size and map, width and height in
# pixels, byte count, and a SHA-256 checksum to confirm a download is complete.
# Each texture also lists its title, authors, source page and licence.
#
# The script stops with an error if a source image is missing.
#
# Install ImageMagick 7 (`magick`), jq, and either shasum (macOS) or sha256sum.
# Usage: tools/textures/images.sh [SOURCE_DIR]
# SOURCE_DIR defaults to prior_art/textures, where fetch.sh saves the textures
# listed in tools/textures/sets.txt.
#
# Upload the result with:
#   gcloud storage rsync --recursive assets/textures/images gs://ze_textures/clump/images

set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
src=${1:-$repo/prior_art/textures}
prior_art=$repo/prior_art
out=$repo/assets/textures/images
base_url='https://storage.googleapis.com/ze_textures/clump/images/'
sizes=(512:512 1k:1024 2k:2048)
source "$repo/tools/textures/stalks.sh"

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

# Fit each map within px × px as JPEG and describe each file.
assets=$(for id in "${ids[@]}"; do
    material=${id%%_*}
    for map in color normal roughness occlusion; do
        if [[ ! -f $src/$id/$map.jpg ]]; then
            echo "missing $src/$id/$map.jpg" >&2
            exit 1
        fi
    done
    mkdir -p "$out/$material"
    for size in "${sizes[@]}"; do
        name=${size%%:*} px=${size#*:}
        prefix=$out/$material/$id-$name

        # Resize colour in linear light to preserve average brightness. Keep
        # full colour resolution (4:4:4) so normal directions are not blurred.
        magick "$src/$id/color.jpg" -colorspace RGB -filter Lanczos -resize "${px}x${px}" \
            -colorspace sRGB -strip -quality 92 "$prefix-color.jpg"
        magick "$src/$id/normal.jpg" -filter Lanczos -resize "${px}x${px}" \
            -strip -sampling-factor 4:4:4 -quality 95 "$prefix-normal.jpg"

        # Store occlusion in red, roughness in green and zero metalness in
        # blue for glTF. Keep full colour resolution (4:4:4) to preserve
        # detail in these separate surface properties.
        magick \
            \( "$src/$id/occlusion.jpg" -channel R -separate +channel \) \
            \( "$src/$id/roughness.jpg" -channel R -separate +channel \) \
            -filter Lanczos -resize "${px}x${px}" \
            \( -clone 0 -evaluate set 0 \) \
            -set colorspace sRGB -combine -alpha off -type TrueColor \
            -strip -sampling-factor 4:4:4 -quality 95 "$prefix-orm.jpg"

        for map in color normal orm; do
            describe "$material/$id-$name-$map.jpg" \
                "$(jq -nc --arg size "$name" --arg map "$map" '{size: $size, map: $map}')"
        done
    done | jq -s --arg id "$id" --arg material "$material" --slurpfile info "$src/$id/info.json" \
        '{id: $id, category: $material} + $info[0] + {files: .}'
done | jq -s '.')

# Write each stalk card's colour image and any normal map as PNGs, recording
# each in the manifest.
stalk_assets=$(while IFS=$'\t' read -r kind source path title; do
    prepare_stalk "$kind" "$source" "$prior_art/$path" "$out/stalks"
    info=$(stalk_info "$kind" "$source" "$title")
    for map in $(stalk_maps "$kind" "$out/stalks"); do
        describe "stalks/$kind-$map.png" "$(jq -nc --arg map "$map" '{map: $map}')"
    done | jq -s --arg id "$kind" --argjson info "$info" \
        '{id: $id, category: "stalks"} + $info + {files: .}'
done < <(stalk_list "$repo") | jq -s '.')

# Write the manifest with a short description of the files.
jq -n --arg base_url "$base_url" --argjson surface "$assets" --argjson stalks "$stalk_assets" '{
    version: 1,
    baseUrl: $base_url,
    pathPatterns: {
        surface: "<category>/<id>-<size>-<map>.jpg",
        stalks: "stalks/<id>-<map>.png"
    },
    sizes: {
        "512": "512 x 512 pixels",
        "1k": "1024 x 1024 pixels",
        "2k": "2048 x 2048 pixels"
    },
    maps: {
        color: "surface colour, sRGB, with straight-alpha transparency for stalk cards",
        normal: "surface bumps, OpenGL (+Y); load as linear data",
        orm: "R occlusion (shades hollows), G roughness (light is matte), B metalness (always zero); linear data"
    },
    assets: ($surface + $stalks)
}' >"$out/images_manifest.json"
cp "$repo/tools/textures/LICENSE.md" "$out/LICENSE.md"
echo "Wrote $out"
