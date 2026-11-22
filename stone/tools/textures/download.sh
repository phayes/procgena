#!/usr/bin/env bash
# Download the ready-made rock textures listed in assets/textures/assets_manifest.json
# from the public gs://ze_textures bucket. Use this instead of fetch.sh and
# convert.sh when you only need the finished KTX2 files, for example to run the
# viewer or the texture tests.
#
# By default it downloads the 512 and 1k sizes in both formats (BC7 and UASTC).
# The 2k size is large, so it is downloaded only when you ask for it.
# Files are saved in assets/textures/rock/ unless you choose another folder.
#
# You can rerun this script: files that match the manifest's SHA-256 checksum
# are kept, and missing or changed files are downloaded again. Each download is
# checked before it replaces anything; if the check fails, the script stops.
#
# With --images, it downloads the JPEG images listed in
# assets/textures/images/images_manifest.json instead, for exporting rocks as
# OBJ or glTF files. They are saved in assets/textures/images/rock/, and
# --formats does not apply.
#
# Install curl, jq, and either shasum (macOS) or sha256sum before running.
# Usage: tools/textures/download.sh [--images] [--sizes 512,1k,2k] [--formats bc7,uastc] [--dest DIR]
# --dest names the folder that receives rock/ (or images/rock/ with --images);
# it defaults to assets/textures.

set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
dest=$repo/assets/textures
sizes=512,1k
formats=bc7,uastc
images=false
ua='Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36'

# Read the options.
while [[ $# -gt 0 ]]; do
    case $1 in
        --sizes) sizes=$2; shift 2 ;;
        --formats) formats=$2; shift 2 ;;
        --dest) dest=$2; shift 2 ;;
        --images) images=true; shift ;;
        *) echo "unknown option $1" >&2; exit 1 ;;
    esac
done

# Choose the KTX2 textures or the JPEG images.
if $images; then
    manifest=$repo/assets/textures/images/images_manifest.json
    dest=$dest/images
    formats=
else
    manifest=$repo/assets/textures/assets_manifest.json
fi

# Get a file's SHA-256 checksum.
sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# List the chosen files as "path sha256" lines.
base_url=$(jq -er '.baseUrl' "$manifest")
files=$(jq -r --arg sizes "$sizes" --arg formats "$formats" '
    ($sizes | split(",")) as $s | ($formats | split(",")) as $f
    | .assets[].files[]
    | select((.size | IN($s[])) and (.format == null or (.format | IN($f[]))))
    | "\(.path) \(.sha256)"' "$manifest")
if [[ -z $files ]]; then
    echo "no files match --sizes $sizes${formats:+ --formats $formats}" >&2
    exit 1
fi

# Download each missing or changed file to a temporary name, check it, then move it into place.
kept=0 downloaded=0
while read -r path sum; do
    file=$dest/$path
    if [[ -f $file && $(sha256 "$file") == "$sum" ]]; then
        kept=$((kept + 1))
        continue
    fi
    mkdir -p "$(dirname "$file")"
    curl -fsS -A "$ua" -o "$file.part" "$base_url$path"
    if [[ $(sha256 "$file.part") != "$sum" ]]; then
        rm -f "$file.part"
        echo "checksum mismatch for $base_url$path" >&2
        exit 1
    fi
    mv "$file.part" "$file"
    downloaded=$((downloaded + 1))
done <<<"$files"
echo "Downloaded $downloaded files and kept $kept up-to-date files in $dest/rock"
