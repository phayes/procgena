# shellcheck shell=bash
# Stalk card texture preparation and metadata helpers shared by convert.sh,
# images.sh and manifest.sh. Source this file from Bash; it defines functions
# without processing any files until a function is called.

# List stalk textures from tools/textures/stalks.txt in their original order.
# Output fields are tab-separated: kind, source, path, title. Blank lines and
# comments are skipped. Argument: repository root.
stalk_list() {
    local repo=$1 kind source path title
    while read -r kind source path title; do
        [[ -z $kind || $kind == \#* ]] && continue
        printf '%s\t%s\t%s\t%s\n' "$kind" "$source" "$path" "$title"
    done <"$repo/tools/textures/stalks.txt"
}

# Print the map names a prepared stalk texture has: "color normal" when a
# normal map exists, otherwise "color". Arguments: kind, folder of prepared PNGs.
stalk_maps() {
    local kind=$1 dir=$2
    if [[ -f $dir/$kind-normal.png ]]; then echo color normal; else echo color; fi
}

# Write <out>/<kind>-color.png and, when the source has one, <kind>-normal.png.
# Outputs are 8-bit PNGs at the source dimensions. Colour uses sRGB with
# straight alpha (RGB is not multiplied by opacity), with colour filled in
# behind transparent pixels to avoid dark fringes when filtered. Normal maps
# use OpenGL (+Y) directions, stored as linear data without transparency.
# Arguments: kind, source name, absolute source path, output folder.
prepare_stalk() {
    local kind=$1 source=$2 src=$3 out=$4
    mkdir -p "$out"
    rm -f "$out/$kind-normal.png"
    case $source in
        nobiax)
            bleed_color "$src/diffuse.tga" "$out/$kind-color.png"
            magick "$src/normal.tga" -alpha off -type TrueColor "PNG24:$out/$kind-normal.png"
            ;;
        generated)
            bleed_color "$src.png" "$out/$kind-color.png"
            if [[ -f $src-normal.png ]]; then
                magick "$src-normal.png" -alpha off -type TrueColor "PNG24:$out/$kind-normal.png"
            fi
            ;;
        *)
            echo "unknown stalk source $source" >&2
            return 1
            ;;
    esac
}

# Copy a card image to an 8-bit PNG, filling transparent areas with nearby
# colours to reduce dark edges when textures are filtered or viewed from afar.
# Preserves the original transparency and visible colours. The input should use
# sRGB colour; the output uses straight alpha (colour separate from transparency).
# Arguments: input image path, output PNG path.
bleed_color() {
    local src=$1 dst=$2

    # Layer increasingly detailed copies to fill the background with nearby
    # colours. Restore the original colours wherever alpha is nonzero, then
    # restore alpha.
    local size s
    size=$(magick identify -format '%wx%h!' "$src")
    local args=("$src" -alpha set -write mpr:card +delete
        mpr:card -resize '1x1!' -alpha off -alpha set -resize "$size")
    for s in 2 4 8 16 32 64 128 256 512; do
        args+=('(' mpr:card -resize "${s}x${s}!" -resize "$size" ')'
            -compose over -composite)
    done
    args+=('(' mpr:card '(' mpr:card -alpha extract -threshold 0 ')'
            -compose copyalpha -composite ')' -compose over -composite
        '(' mpr:card -alpha extract ')' -compose copyalpha -composite
        -type TrueColorAlpha "PNG32:$dst")
    magick "${args[@]}"
}

# Print attribution and licence metadata for one stalk texture as JSON.
# Arguments: kind, source name, title. Unknown sources return an error.
stalk_info() {
    local kind=$1 source=$2 title=$3
    case $source in
        nobiax)
            jq -nc --arg title "$title" '{
                title: $title,
                authors: ["Nobiax (yughues)"],
                source: "nobiax",
                sourceUrl: "https://nobiax.deviantart.com",
                license: "CC0-1.0"
            }'
            ;;
        generated)
            jq -nc --arg title "$title" '{
                title: $title,
                authors: ["procgena_clump"],
                source: "generated",
                sourceUrl: "tools/textures/LICENSE.md",
                license: "see LICENSE.md"
            }'
            ;;
        *)
            echo "unknown stalk source $source" >&2
            return 1
            ;;
    esac
}
