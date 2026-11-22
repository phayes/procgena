#!/usr/bin/env bash
# Download free-to-use textures for clumps from Poly Haven
# (https://polyhaven.com) and ambientCG (https://ambientcg.com). Both release
# their textures under CC0, so they can be used in personal or commercial
# projects without attribution. Choose textures, and where each comes from, in
# tools/textures/sets.txt.
#
# Each set has five images, 2048 pixels per side: colour; a normal map for
# the appearance of bumps; roughness for matte or polished areas; ambient
# occlusion for shading in cracks and hollows; and height, with lighter values
# for raised areas. Choose OpenGL (+Y) settings when using the normal maps.
#
# Images are saved in <DEST>/<id>/ as color.jpg, normal.jpg, roughness.jpg,
# occlusion.jpg and height.jpg, ready for convert.sh and images.sh. The
# accompanying info.json records the title, authors, source and licence.
#
# Some ambientCG sets, including several snow sets, have no ambient occlusion
# map. For those, occlusion.jpg is a plain white image, which adds no shading.
#
# Most people only need the finished files; tools/textures/download.sh fetches
# them ready-made. Use this script to add or remake textures.
#
# You can rerun this script; it refreshes every info.json. Poly Haven images
# that match the originals are kept, and missing or changed ones are
# downloaded again. Downloads are checked against Poly Haven's checksums; if
# a check fails, the downloaded file is removed and the script stops.
# ambientCG publishes no checksums, so its sets are downloaded only when one of
# their images is missing, and each archive is checked for damage before any
# image is replaced. Delete a set's images to download it again.
#
# Install curl, jq, unzip, ImageMagick 7 (`magick`), and either md5 (macOS) or
# md5sum before running.
# Usage: tools/textures/fetch.sh [DEST]
# DEST defaults to prior_art/textures in the repo.

set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
dest=${1:-$repo/prior_art/textures}
ua='Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36'
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

# Get a file fingerprint (MD5 checksum) to check it against the original download.
checksum() {
    if command -v md5sum >/dev/null; then md5sum "$1" | cut -d' ' -f1; else md5 -q "$1"; fi
}

# Download one Poly Haven set. Arguments: our id, then its Poly Haven id.
fetch_polyhaven() {
    local id=$1 source_id=$2

    # Save the texture's title and authors for manifest.sh and images.sh.
    curl -fsS -A "$ua" "https://api.polyhaven.com/info/$source_id" |
        jq -e --arg sid "$source_id" '{
            title: .name,
            authors: (.authors | keys),
            source: "polyhaven",
            sourceUrl: "https://polyhaven.com/a/\($sid)",
            license: "CC0-1.0"
        }' >"$dest/$id/info.json"

    local listing
    listing=$(curl -fsS -A "$ua" "https://api.polyhaven.com/files/$source_id")

    # Save colour, OpenGL normal, roughness, ambient occlusion and height maps
    # with the filenames convert.sh expects.
    local pair map name url md5 file
    for pair in Diffuse:color nor_gl:normal Rough:roughness AO:occlusion Displacement:height; do
        map=${pair%%:*} name=${pair#*:}
        url=$(jq -er --arg m "$map" '.[$m]["2k"].jpg.url' <<<"$listing")
        md5=$(jq -er --arg m "$map" '.[$m]["2k"].jpg.md5' <<<"$listing")
        file=$dest/$id/$name.jpg

        # Keep a matching file; otherwise download it and check for damage.
        if [[ -f $file && $(checksum "$file") == "$md5" ]]; then
            continue
        fi
        curl -fsS -A "$ua" -o "$file" "$url"
        if [[ $(checksum "$file") != "$md5" ]]; then
            rm -f "$file"
            echo "checksum mismatch for $url" >&2
            exit 1
        fi
        echo "downloaded $id/$name.jpg"
    done
}

# Download one ambientCG set. Arguments: our id, then its ambientCG asset id.
fetch_ambientcg() {
    local id=$1 source_id=$2

    # Save the texture's title for manifest.sh and images.sh. ambientCG does
    # not name individual authors, so the site is credited.
    curl -fsS -A "$ua" "https://ambientcg.com/api/v2/full_json?id=$source_id" |
        jq -e --arg sid "$source_id" '.foundAssets[0] | {
            title: .displayName,
            authors: ["ambientCG"],
            source: "ambientcg",
            sourceUrl: "https://ambientcg.com/a/\($sid)",
            license: "CC0-1.0"
        }' >"$dest/$id/info.json"

    # Keep a set that already has every image convert.sh and images.sh need.
    local name missing=false
    for name in color normal roughness occlusion height; do
        [[ -f $dest/$id/$name.jpg ]] || missing=true
    done
    if [[ $missing == false ]]; then
        return
    fi

    # Download the 2k JPEG archive and check it for damage before unpacking.
    local archive=$stage/$source_id.zip unpacked=$stage/$source_id
    curl -fsS -L -A "$ua" -o "$archive" "https://ambientcg.com/get?file=${source_id}_2K-JPG.zip"
    if ! unzip -tqq "$archive" >/dev/null; then
        echo "damaged download for $source_id" >&2
        exit 1
    fi
    unzip -qq -o "$archive" -d "$unpacked"
    # Some archives unpack as read-only folders, which the cleanup cannot remove.
    chmod -R u+w "$unpacked"

    # Copy colour, OpenGL normal, roughness, height and any ambient occlusion
    # map with the filenames convert.sh expects.
    local prefix=$unpacked/${source_id}_2K-JPG pair map file
    for pair in Color:color NormalGL:normal Roughness:roughness Displacement:height \
        AmbientOcclusion:occlusion; do
        map=${pair%%:*} name=${pair#*:}
        file=${prefix}_$map.jpg
        if [[ -f $file ]]; then
            cp "$file" "$dest/$id/$name.jpg"
            echo "downloaded $id/$name.jpg"
        elif [[ $name != occlusion ]]; then
            echo "$source_id has no $map map" >&2
            exit 1
        fi
    done

    # Without an ambient occlusion map, use white, which adds no shading.
    if [[ ! -f ${prefix}_AmbientOcclusion.jpg ]]; then
        magick "$dest/$id/color.jpg" -fill white -colorize 100 -colorspace Gray \
            -quality 95 "$dest/$id/occlusion.jpg"
        echo "wrote white $id/occlusion.jpg"
    fi
}

# Load the texture list; ignore comments and empty lines.
sets=()
while IFS= read -r line; do
    [[ -z $line || $line == \#* ]] || sets+=("$line")
done <"$repo/tools/textures/sets.txt"

# Download each set from its source; a bare id means Poly Haven.
for entry in "${sets[@]}"; do
    read -r id source source_id <<<"$entry"
    source=${source:-polyhaven}
    source_id=${source_id:-$id}
    mkdir -p "$dest/$id"
    case $source in
        polyhaven) fetch_polyhaven "$id" "$source_id" ;;
        ambientcg) fetch_ambientcg "$id" "$source_id" ;;
        *)
            echo "unknown source '$source' for $id in sets.txt" >&2
            exit 1
            ;;
    esac
done
echo "Images are in $dest"
