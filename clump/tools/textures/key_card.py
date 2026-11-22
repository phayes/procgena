#!/usr/bin/env python3
"""Turn a photo of reddish stalks on a light background into a stalk card.

Writes <PREFIX>.png and <PREFIX>-normal.png, the "generated" source images
that stalks.txt and stalks.sh expect:

- <PREFIX>.png: a square sRGB image with straight (not premultiplied) alpha.
  The stalks are cut out, standing on the bottom edge and centred
  horizontally. Partly transparent edge pixels take the colour of the nearest
  solid pixel, so the cut-out shows no light fringe from the old background.
- <PREFIX>-normal.png: an OpenGL (+Y) normal map that treats each stalk as a
  rounded tube. Transparent areas face straight out of the card.

The cut-out keys on how much redder than green each pixel is, so it suits
orange or red subjects (such as moss sporophytes) on grey, white or green
backgrounds. Use --crop-bottom to remove ground or moss along the bottom edge,
and --min-area to drop small specks the key leaves behind.

Needs numpy, scipy and Pillow.
Usage: tools/textures/key_card.py INPUT PREFIX [options]; see --help.
"""

import argparse

import numpy as np
from PIL import Image
from scipy import ndimage


def key_alpha(rgb, lo, hi):
    """Opacity from red minus green: lo or less is clear, hi or more is solid."""
    redness = rgb[..., 0] - rgb[..., 1]
    return np.clip((redness - lo) / (hi - lo), 0.0, 1.0)


def drop_specks(alpha, min_area):
    """Clear connected areas smaller than min_area pixels, keeping soft edges
    around the areas that remain."""
    labels, count = ndimage.label(alpha > 0.5)
    if count == 0:
        return alpha
    areas = ndimage.sum_labels(np.ones_like(alpha), labels, index=np.arange(1, count + 1))
    keep = np.isin(labels, np.flatnonzero(areas >= min_area) + 1)
    return alpha * ndimage.binary_dilation(keep, iterations=3)


def decontaminate(rgb, alpha, solid=0.9):
    """Give every pixel below `solid` opacity the colour of the nearest solid
    pixel, removing background colour mixed into the edges."""
    core = alpha >= solid
    if not core.any():
        return rgb
    _, (rows, cols) = ndimage.distance_transform_edt(~core, return_indices=True)
    out = rgb[rows, cols]
    out[core] = rgb[core]
    return out


def frame_square(rgb, alpha, margin):
    """Crop to the visible stalks and pad to a square, with the stalk bases on
    the bottom edge and the stalks centred horizontally."""
    rows = np.flatnonzero((alpha > 0.05).any(axis=1))
    cols = np.flatnonzero((alpha > 0.05).any(axis=0))
    if rows.size == 0:
        raise SystemExit("nothing left after keying; lower --lo or --hi")
    top, bottom, left, right = rows[0], rows[-1] + 1, cols[0], cols[-1] + 1
    height, width = bottom - top, right - left

    # Leave a margin above and to the sides; the bases touch the bottom edge.
    side = int(round(max(height, width) * (1.0 + 2.0 * margin)))
    out_rgb = np.zeros((side, side, 3), dtype=rgb.dtype)
    out_alpha = np.zeros((side, side), dtype=alpha.dtype)
    y0, x0 = side - height, (side - width) // 2
    out_rgb[y0:, x0:x0 + width] = rgb[top:bottom, left:right]
    out_alpha[y0:, x0:x0 + width] = alpha[top:bottom, left:right]

    # Fill the padding with the nearest stalk colour too, so resizing does
    # not darken the edges.
    return decontaminate(out_rgb, out_alpha, solid=0.05), out_alpha


def normal_map(alpha, sigma, strength):
    """OpenGL (+Y up) normals from the opacity, treated as a rounded height."""
    height = ndimage.gaussian_filter(alpha, sigma)
    d_row, d_col = np.gradient(height)
    # Image rows run downward, so the upward slope is -d_row.
    normal = np.stack([-strength * d_col, strength * d_row, np.ones_like(height)], axis=-1)
    normal /= np.linalg.norm(normal, axis=-1, keepdims=True)
    return normal * 0.5 + 0.5


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("input", help="photo to key, any format Pillow reads")
    parser.add_argument("prefix", help="output path without .png")
    parser.add_argument("--lo", type=float, default=0.16,
                        help="red minus green (0-1) at or below which pixels are clear")
    parser.add_argument("--hi", type=float, default=0.31,
                        help="red minus green (0-1) at or above which pixels are solid")
    parser.add_argument("--crop-bottom", type=int, default=0,
                        help="rows to remove from the bottom of the input before keying")
    parser.add_argument("--min-area", type=int, default=400,
                        help="drop solid areas smaller than this many input pixels")
    parser.add_argument("--margin", type=float, default=0.02,
                        help="space above and beside the stalks, as a fraction of the card")
    parser.add_argument("--size", type=int, default=1024, help="output width and height")
    args = parser.parse_args()

    # Key the photo and clean up the cut-out at the input resolution.
    rgb = np.asarray(Image.open(args.input).convert("RGB"), dtype=np.float32) / 255.0
    if args.crop_bottom:
        rgb = rgb[:-args.crop_bottom]
    alpha = drop_specks(key_alpha(rgb, args.lo, args.hi), args.min_area)
    rgb = decontaminate(rgb, alpha)
    rgb, alpha = frame_square(rgb, alpha, args.margin)

    # Resize colour and opacity separately; colour is valid everywhere.
    size = (args.size, args.size)
    color = Image.fromarray((rgb * 255.0 + 0.5).astype(np.uint8)).resize(size, Image.LANCZOS)
    opacity = Image.fromarray((alpha * 255.0 + 0.5).astype(np.uint8)).resize(size, Image.LANCZOS)
    color.putalpha(opacity)
    color.save(f"{args.prefix}.png")

    # Build the normal map at the output resolution.
    out_alpha = np.asarray(opacity, dtype=np.float32) / 255.0
    normal = normal_map(out_alpha, sigma=1.5, strength=4.0)
    Image.fromarray((normal * 255.0 + 0.5).astype(np.uint8)).save(f"{args.prefix}-normal.png")


if __name__ == "__main__":
    main()
