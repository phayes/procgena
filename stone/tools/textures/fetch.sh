#!/usr/bin/env bash
# Download free-to-use textures from Poly Haven (https://polyhaven.com),
# ambientCG (https://ambientcg.com), ShareTextures
# (https://www.sharetextures.com) and TextureCan (https://www.texturecan.com),
# and prepare textures downloaded by hand. All are released under CC0, so
# they can be used in personal or commercial projects without attribution.
# Choose textures, and where each comes from, in tools/textures/sets.txt.
# ShareTextures, TextureCan and hand-downloaded sets take their title,
# authors and licence from tools/textures/sources/<id>.json.
#
# Hand-downloaded ("manual") sets come from pages that scripts cannot
# download from. Download the files listed in tools/textures/sources/<id>.json
# from its sourceUrl, and save them with their original names in
# <DEST>/<id>/original/. This script then saves them under the names below
# and converts DirectX (-Y) normal maps to OpenGL (+Y). A set may list a
# specular map instead of roughness; it is inverted to make roughness.
#
# Each set has five images, 2048 pixels per side: colour; a normal map for
# the appearance of bumps; roughness for matte or polished areas; ambient
# occlusion for shading in cracks and hollows; and height, with lighter values
# for raised areas. Choose OpenGL (+Y) settings when using the normal maps.
# Sets that include a metalness map, where light areas are bare metal, also
# get metalness.jpg.
#
# Images are saved in <DEST>/<id>/ as color.jpg, normal.jpg, roughness.jpg,
# occlusion.jpg, height.jpg and, when available, metalness.jpg, ready for
# convert.sh and images.sh. The accompanying info.json records the title,
# authors, source and licence.
#
# Some ambientCG sets, metals in particular, have no ambient occlusion map.
# For those, occlusion.jpg is a plain white image, which adds no shading.
# Some ShareTextures sets have a specular map instead of roughness; their
# roughness.jpg is the inverted specular map, so shiny areas look polished.
#
# Most people only need the finished files; tools/textures/download.sh fetches
# them ready-made. Use this script to add or remake textures.
#
# You can rerun this script; it refreshes every info.json. Poly Haven images
# that match the originals are kept, and missing or changed ones are
# downloaded again. Downloads are checked against Poly Haven's checksums; if
# a check fails, the downloaded file is removed and the script stops.
# ambientCG, ShareTextures and TextureCan publish no checksums, so their sets are
# downloaded only when one of their images is missing, and each archive is
# checked for damage before any image is replaced. Delete a set's images to
# download it again. Hand-downloaded sets are prepared again on every run.
#
# Install curl, jq, unzip, ImageMagick 7 (`magick`), and either md5 (macOS) or
# md5sum before running.
# Usage: tools/textures/fetch.sh [DEST]
# DEST defaults to prior_art/polyhaven in the repo, which Git ignores.

set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
dest=${1:-$repo/prior_art/polyhaven}
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

    # Copy colour, OpenGL normal, roughness, height and any metalness and
    # ambient occlusion maps with the filenames convert.sh expects.
    local prefix=$unpacked/${source_id}_2K-JPG pair map file
    for pair in Color:color NormalGL:normal Roughness:roughness Displacement:height \
        Metalness:metalness AmbientOcclusion:occlusion; do
        map=${pair%%:*} name=${pair#*:}
        file=${prefix}_$map.jpg
        if [[ -f $file ]]; then
            cp "$file" "$dest/$id/$name.jpg"
            echo "downloaded $id/$name.jpg"
        elif [[ $name != metalness && $name != occlusion ]]; then
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

# Save the title, authors, source and licence from tools/textures/sources/<id>.json
# as info.json for manifest.sh and images.sh. Argument: our id.
copy_info() {
    local id=$1 info=$repo/tools/textures/sources/$1.json
    if [[ ! -f $info ]]; then
        echo "missing $info for $id" >&2
        exit 1
    fi
    jq -e '{title, authors, source, sourceUrl, license}' "$info" >"$dest/$id/info.json"
}

# Download one ShareTextures set. Arguments: our id, then the archive name
# before -2K.zip, for example amethyst_texture_1.
fetch_sharetextures() {
    local id=$1 source_id=$2
    copy_info "$id"

    # Keep a set that already has every image convert.sh and images.sh need.
    local name missing=false
    for name in color normal roughness occlusion height; do
        [[ -f $dest/$id/$name.jpg ]] || missing=true
    done
    if [[ $missing == false ]]; then
        return
    fi

    # Download the 2k archive and check it for damage before unpacking.
    local archive=$stage/$source_id.zip unpacked=$stage/$source_id
    curl -fsS -L -A "$ua" -o "$archive" \
        "https://files.sharetextures.com/file/Share-Textures/${source_id}-2K.zip"
    if ! unzip -tqq "$archive" >/dev/null; then
        echo "damaged download for $source_id" >&2
        exit 1
    fi
    unzip -qq -o "$archive" -d "$unpacked"
    # Some archives unpack as read-only folders, which the cleanup cannot remove.
    chmod -R u+w "$unpacked"

    # Copy colour, normal, ambient occlusion, height and roughness maps with
    # the filenames convert.sh expects. Images end in -<map>.jpg.
    local pair map file
    for pair in diffuse:color normal:normal ao:occlusion displacement:height roughness:roughness; do
        map=${pair%%:*} name=${pair#*:}
        file=$(find "$unpacked" -type f -name "*-$map.jpg" | head -1)
        if [[ -n $file ]]; then
            cp "$file" "$dest/$id/$name.jpg"
            echo "downloaded $id/$name.jpg"
        elif [[ $name != roughness ]]; then
            echo "$source_id has no $map map" >&2
            exit 1
        fi
    done

    # Some sets have a specular map instead of roughness. Treat bright,
    # shiny areas as polished: roughness is the inverted specular map.
    if [[ ! -f $dest/$id/roughness.jpg ]]; then
        file=$(find "$unpacked" -type f -name "*-specular.jpg" | head -1)
        if [[ -z $file ]]; then
            echo "$source_id has no roughness or specular map" >&2
            exit 1
        fi
        magick "$file" -colorspace Gray -negate -quality 95 "$dest/$id/roughness.jpg"
        echo "wrote $id/roughness.jpg from the specular map"
    fi
}

# Download one TextureCan set. Arguments: our id, then the number at the end
# of its page address, for example 249 for https://www.texturecan.com/details/249/.
fetch_texturecan() {
    local id=$1 source_id=$2
    copy_info "$id"

    # Keep a set that already has every image convert.sh and images.sh need.
    local name missing=false
    for name in color normal roughness occlusion height; do
        [[ -f $dest/$id/$name.jpg ]] || missing=true
    done
    if [[ $missing == false ]]; then
        return
    fi

    # Find the 2k archive on the texture's page; its address changes per texture.
    local page link
    page=$(curl -fsS -L -A "$ua" "https://www.texturecan.com/details/$source_id/")
    link=$(grep -o -E '/downloads/[^"]*_2k_[^"]*\.zip' <<<"$page" | head -1)
    if [[ -z $link ]]; then
        echo "no 2k download on TextureCan page $source_id" >&2
        exit 1
    fi

    # Download the archive and check it for damage before unpacking.
    local archive=$stage/texturecan_$source_id.zip unpacked=$stage/texturecan_$source_id
    curl -fsS -L -A "$ua" -o "$archive" "https://www.texturecan.com$link"
    if ! unzip -tqq "$archive" >/dev/null; then
        echo "damaged download for TextureCan $source_id" >&2
        exit 1
    fi
    unzip -qq -o "$archive" -d "$unpacked" -x '__MACOSX/*'
    # Some archives unpack as read-only folders, which the cleanup cannot remove.
    chmod -R u+w "$unpacked"

    # Save colour, OpenGL normal, roughness, ambient occlusion and height maps
    # as JPEGs with the filenames convert.sh expects. Images end in
    # _<map>_2k.jpg or _<map>_2k.png.
    local pair map file
    for pair in color:color normal_opengl:normal roughness:roughness ao:occlusion height:height; do
        map=${pair%%:*} name=${pair#*:}
        file=$(find "$unpacked" -type f \( -name "*_${map}_2k.jpg" -o -name "*_${map}_2k.png" \) | head -1)
        if [[ -z $file ]]; then
            echo "TextureCan $source_id has no $map map" >&2
            exit 1
        fi
        if [[ $name == color || $name == normal ]]; then
            magick "$file" -alpha off -depth 8 -quality 97 -sampling-factor 4:4:4 "$dest/$id/$name.jpg"
        else
            magick "$file" -alpha off -colorspace Gray -depth 8 -quality 97 "$dest/$id/$name.jpg"
        fi
        echo "downloaded $id/$name.jpg"
    done
}

# Prepare one set whose images were downloaded by hand, for sources that
# cannot be downloaded by script. Argument: our id. The delivered files must
# be saved, with their original names, in <DEST>/<id>/original/;
# tools/textures/sources/<id>.json lists them and gives the download page.
prepare_manual() {
    local id=$1 info=$repo/tools/textures/sources/$1.json
    copy_info "$id"
    local originals=$dest/$id/original

    # Use a specular map in place of roughness when the set has no roughness map.
    local rough_key=roughness
    if ! jq -e '.originals.roughness' "$info" >/dev/null; then
        rough_key=specular
    fi
    local keys=(color normal "$rough_key" occlusion height)

    # Stop with instructions if any delivered file is missing.
    local key name file
    for key in "${keys[@]}"; do
        file=$(jq -er --arg n "$key" '.originals[$n]' "$info")
        if [[ ! -f $originals/$file ]]; then
            echo "missing $originals/$file" >&2
            echo "download $id from $(jq -r .sourceUrl "$info") and save its files in $originals" >&2
            exit 1
        fi
    done

    # Save each map as a high-quality JPEG with the filename convert.sh
    # expects. Flip the green channel of DirectX (-Y) normal maps to OpenGL
    # (+Y), and invert a specular map so shiny areas become polished
    # (low roughness).
    local flip=()
    if [[ $(jq -r '.normalConvention' "$info") == directx ]]; then
        flip=(-channel G -negate +channel)
    fi
    for key in "${keys[@]}"; do
        name=$key
        file=$originals/$(jq -er --arg n "$key" '.originals[$n]' "$info")
        if [[ $key == specular ]]; then
            name=roughness
            magick "$file" -alpha off -colorspace Gray -negate -quality 97 "$dest/$id/$name.jpg"
        elif [[ $name == color ]]; then
            magick "$file" -alpha off -quality 97 -sampling-factor 4:4:4 "$dest/$id/$name.jpg"
        elif [[ $name == normal ]]; then
            magick "$file" -alpha off "${flip[@]}" -quality 97 -sampling-factor 4:4:4 \
                "$dest/$id/$name.jpg"
        else
            magick "$file" -alpha off -colorspace Gray -quality 97 "$dest/$id/$name.jpg"
        fi
        echo "prepared $id/$name.jpg"
    done
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
        sharetextures) fetch_sharetextures "$id" "$source_id" ;;
        texturecan) fetch_texturecan "$id" "$source_id" ;;
        manual) prepare_manual "$id" ;;
        *)
            echo "unknown source '$source' for $id in sets.txt" >&2
            exit 1
            ;;
    esac
done
echo "Images are in $dest"
