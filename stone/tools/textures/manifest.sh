#!/usr/bin/env bash
# Writes assets/textures/assets_manifest.json, a list of every rock texture file
# that convert.sh made, for apps that download textures from the public
# gs://ze_textures bucket, under stone/. Run it after convert.sh, and upload the
# manifest with the textures so the two stay in step. Existing output is
# overwritten.
#
# Each file is listed with the address to download it from, its texture id,
# size, map and format, its width and height in pixels, its byte count, and a
# SHA-256 checksum to confirm a download is complete. Each texture also lists
# its title, authors, source page and licence. Textures appear in the order of
# tools/textures/sets.txt.
#
# The script stops with an error if any expected file is missing.
#
# Install jq, KTX-Software 4.4+ (`ktx`), and either shasum (macOS) or sha256sum.
# Usage: tools/textures/manifest.sh [SOURCE_DIR]
# SOURCE_DIR defaults to prior_art/polyhaven, where fetch.sh saves each
# texture's info.json.
#
# Upload the textures, the manifest and the licence with:
#   gcloud storage rsync --recursive assets/textures/rock gs://ze_textures/stone/rock
#   gcloud storage cp assets/textures/assets_manifest.json assets/textures/LICENSE.md gs://ze_textures/stone/

set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
src=${1:-$repo/prior_art/polyhaven}
out=$repo/assets/textures
base_url='https://storage.googleapis.com/ze_textures/stone/'
sizes=(512 1k 2k)
maps=(color normal orh)
formats=(bc7 uastc)

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

# Describe every file of one texture as a JSON object per line.
describe_files() {
    local id=$1
    for size in "${sizes[@]}"; do
        for map in "${maps[@]}"; do
            for format in "${formats[@]}"; do
                local rel=rock/$id-$size-$map-$format.ktx2
                local file=$out/$rel
                if [[ ! -f $file ]]; then
                    echo "missing $file" >&2
                    exit 1
                fi

                # Read the real dimensions from the KTX2 header.
                local header
                header=$(ktx info --format json "$file" | jq -c '.header')
                jq -nc --arg path "$rel" --arg size "$size" --arg map "$map" \
                    --arg format "$format" --argjson header "$header" \
                    --argjson bytes "$(wc -c <"$file" | tr -d ' ')" --arg sha256 "$(sha256 "$file")" \
                    '{path: $path, size: $size, map: $map, format: $format,
                      width: $header.pixelWidth, height: $header.pixelHeight,
                      bytes: $bytes, sha256: $sha256}'
            done
        done
    done
}

# Combine each texture's details from fetch.sh with its file list.
assets=$(for id in "${ids[@]}"; do
    describe_files "$id" | jq -s --arg id "$id" --slurpfile info "$src/$id/info.json" \
        '{id: $id, category: "rock"} + $info[0] + {files: .}'
done | jq -s '.')

# Write the manifest with a short description of the maps and formats.
jq -n --arg base_url "$base_url" --argjson assets "$assets" '{
    version: 1,
    baseUrl: $base_url,
    pathPattern: "<category>/<id>-<size>-<map>-<format>.ktx2",
    sizes: {
        "512": "512 x 512 pixels",
        "1k": "1024 x 1024 pixels",
        "2k": "2048 x 2048 pixels"
    },
    maps: {
        color: "surface colour; load as sRGB",
        normal: "surface bumps, OpenGL (+Y); load as linear data",
        orh: "R occlusion (shades hollows), G roughness (light is matte), B height (0-1, light is raised); linear data"
    },
    formats: {
        bc7: "BC7 with Zstandard compression, for GPUs that support BC7, commonly desktops",
        uastc: "Basis Universal UASTC with Zstandard compression; the loader converts it to a format the GPU supports"
    },
    assets: $assets
}' >"$out/assets_manifest.json"
echo "Wrote $out/assets_manifest.json"
