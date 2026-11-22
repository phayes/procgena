#!/usr/bin/env bash
# Writes assets/textures/assets_manifest.json, a list of every clump texture
# file that convert.sh made, for apps that download textures from the public
# gs://ze_textures bucket, under clump/. Run it after convert.sh, and upload
# the manifest with the textures so the two stay in step. Existing output is
# overwritten.
#
# Each file is listed with the address to download it from, its texture id,
# map and format (and size, for surface textures), its width and height in
# pixels, its byte count, and a SHA-256 checksum to confirm a download is
# complete. Each texture also lists its category, title, authors, source page
# and licence. Surface textures (category snow or moss, the start of the id)
# appear first, in the order of tools/textures/sets.txt, then stalk cards
# (category stalks) in the order of tools/textures/stalks.txt.
#
# The script stops with an error if any expected file is missing.
#
# Install jq, KTX-Software 4.4+ (`ktx`), and either shasum (macOS) or sha256sum.
# Usage: tools/textures/manifest.sh [SOURCE_DIR]
# SOURCE_DIR defaults to prior_art/textures, where fetch.sh saves each
# texture's info.json.
#
# Upload the textures, the manifest and the licence with:
#   gcloud storage rsync --recursive assets/textures/snow gs://ze_textures/clump/snow
#   gcloud storage rsync --recursive assets/textures/moss gs://ze_textures/clump/moss
#   gcloud storage rsync --recursive assets/textures/leaves gs://ze_textures/clump/leaves
#   gcloud storage rsync --recursive assets/textures/stalks gs://ze_textures/clump/stalks
#   gcloud storage cp assets/textures/assets_manifest.json assets/textures/LICENSE.md gs://ze_textures/clump/

set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
src=${1:-$repo/prior_art/textures}
out=$repo/assets/textures
base_url='https://storage.googleapis.com/ze_textures/clump/'
sizes=(512 1k 2k)
maps=(color normal orh)
formats=(bc7 uastc)
source "$repo/tools/textures/stalks.sh"

# Get a file's SHA-256 checksum.
sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# Load the texture ids, the first word of each line; ignore comments and
# empty lines.
ids=()
while IFS= read -r line; do
    [[ -z $line || $line == \#* ]] || ids+=("${line%%[[:space:]]*}")
done <"$repo/tools/textures/sets.txt"

# Describe one KTX2 file as a JSON object. Arguments: path inside $out, then
# extra fields as a JSON object. Stops if the file is missing.
describe_file() {
    local rel=$1 extra=$2 file=$out/$1
    if [[ ! -f $file ]]; then
        echo "missing $file" >&2
        exit 1
    fi

    # Read the real dimensions from the KTX2 header.
    local header
    header=$(ktx info --format json "$file" | jq -c '.header')
    jq -nc --arg path "$rel" --argjson extra "$extra" --argjson header "$header" \
        --argjson bytes "$(wc -c <"$file" | tr -d ' ')" --arg sha256 "$(sha256 "$file")" \
        '{path: $path} + $extra + {width: $header.pixelWidth, height: $header.pixelHeight,
          bytes: $bytes, sha256: $sha256}'
}

# Combine each surface texture's details from fetch.sh with its file list.
surface_assets=$(for id in "${ids[@]}"; do
    material=${id%%_*}
    for size in "${sizes[@]}"; do
        for map in "${maps[@]}"; do
            for format in "${formats[@]}"; do
                describe_file "$material/$id-$size-$map-$format.ktx2" \
                    "$(jq -nc --arg size "$size" --arg map "$map" --arg format "$format" \
                        '{size: $size, map: $map, format: $format}')"
            done
        done
    done | jq -s --arg id "$id" --arg material "$material" \
        --slurpfile info "$src/$id/info.json" \
        '{id: $id, category: $material} + $info[0] + {files: .}'
done | jq -s '.')

# Include the colour image for every stalk card, plus its normal map when
# convert.sh made one, with attribution and licence details.
stalk_assets=$(while IFS=$'\t' read -r kind source _ title; do
    info=$(stalk_info "$kind" "$source" "$title")
    stalk_card_maps=(color)
    [[ -f $out/stalks/$kind-normal-bc7.ktx2 ]] && stalk_card_maps+=(normal)
    for map in "${stalk_card_maps[@]}"; do
        for format in "${formats[@]}"; do
            describe_file "stalks/$kind-$map-$format.ktx2" \
                "$(jq -nc --arg map "$map" --arg format "$format" '{map: $map, format: $format}')"
        done
    done | jq -s --arg id "$kind" --argjson info "$info" \
        '{id: $id, category: "stalks"} + $info + {files: .}'
done < <(stalk_list "$repo") | jq -s '.')

# Write the manifest with a short description of the maps and formats.
jq -n --arg base_url "$base_url" --argjson surface "$surface_assets" \
    --argjson stalks "$stalk_assets" '{
    version: 1,
    baseUrl: $base_url,
    pathPatterns: {
        surface: "<category>/<id>-<size>-<map>-<format>.ktx2",
        stalks: "stalks/<id>-<map>-<format>.ktx2"
    },
    sizes: {
        "512": "512 x 512 pixels",
        "1k": "1024 x 1024 pixels",
        "2k": "2048 x 2048 pixels"
    },
    maps: {
        color: "surface colour, with transparency for stalk cards; load as sRGB",
        normal: "surface bumps, OpenGL (+Y); load as linear data",
        orh: "R occlusion (shades hollows), G roughness (light is matte), B height (0-1, light is raised); linear data"
    },
    formats: {
        bc7: "BC7 with Zstandard compression, for GPUs that support BC7, commonly desktops",
        uastc: "Basis Universal UASTC with Zstandard compression; the loader converts it to a format the GPU supports"
    },
    assets: ($surface + $stalks)
}' >"$out/assets_manifest.json"
echo "Wrote $out/assets_manifest.json"
