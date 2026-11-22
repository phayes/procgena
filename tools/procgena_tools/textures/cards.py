"""Prepare transparent card images (leaves, stalks) as PNGs for encoding."""

from __future__ import annotations

from pathlib import Path

from .config import Card, Project, Recolor
from .util import ToolError, image_size, magick

# Copies of the leaf in a clump image: scale, angle in degrees, and where the
# leaf's bottom centre lands on a 1024 x 1024 canvas. Later copies are drawn
# over earlier ones.
CLUMP_COPIES = (
    (0.50, -65, 440, 960), (0.50, 65, 584, 960),
    (0.45, -15, 400, 620), (0.45, 20, 630, 600), (0.40, 0, 512, 520),
    (0.55, -35, 470, 1000), (0.55, 35, 554, 1000), (0.60, 0, 512, 1010),
)


def png_out(kind: str, path: Path) -> list[str]:
    """ImageMagick arguments that write `path` as a PNG24 or PNG32 `kind`.

    The file carries no date or time chunks, so the same pixels always give
    the same bytes and the same manifest checksum.
    """
    return ["-define", "png:exclude-chunk=date,time", f"{kind}:{path}"]


def prepare_card(project: Project, card: Card, out: Path) -> None:
    """Write `<out>/<kind>-color.png` and, when the card has one, `<kind>-normal.png`.

    Colour is 8-bit sRGB with straight alpha, with colour filled in behind
    transparent pixels so filtering shows no dark fringe. Normal maps are
    OpenGL (+Y) linear data without transparency. Raises ToolError if a source
    file is missing.
    """
    out.mkdir(parents=True, exist_ok=True)
    color = out / f"{card.kind}-color.png"
    normal = out / f"{card.kind}-normal.png"
    normal.unlink(missing_ok=True)
    src = project.prior_art / card.path

    if card.source == "ez-tree":
        _need(src, card)
        magick(src, "-type", "TrueColorAlpha", *png_out("PNG32", color))
    elif card.source == "nobiax":
        _need(src / "diffuse.tga", card)
        bleed_color(src / "diffuse.tga", color)
        magick(src / "normal.tga", "-alpha", "off", "-type", "TrueColor", *png_out("PNG24", normal))
    else:
        colour_src = src.parent / f"{src.name}.png"
        _need(colour_src, card)
        bleed_color(colour_src, color)
        normal_src = src.parent / f"{src.name}-normal.png"
        if normal_src.is_file():
            magick(normal_src, "-alpha", "off", "-type", "TrueColor", *png_out("PNG24", normal))

    if card.recolor:
        regrade_green(color, card.recolor)


def _need(path: Path, card: Card) -> None:
    """Stop with a setup hint when a card's source file is missing."""
    if not path.exists():
        raise ToolError(f"missing {path} for card {card.kind}; see the cards notes in textures.yml")


def bleed_color(src: Path, dst: Path) -> None:
    """Copy an sRGB image to an 8-bit PNG, filling transparent areas with nearby colours.

    Visible colours and transparency are unchanged; the fill avoids dark edges
    when the texture is filtered or viewed from afar. The output has straight
    alpha (colour separate from transparency).
    """
    width, height = image_size(src)
    size = f"{width}x{height}!"

    # Layer increasingly detailed copies to fill the background with nearby
    # colours, restore the original colours wherever alpha is nonzero, then
    # restore alpha.
    args: list[str | Path] = [
        src, "-alpha", "set", "-write", "mpr:card", "+delete",
        "mpr:card", "-resize", "1x1!", "-alpha", "off", "-alpha", "set", "-resize", size,
    ]
    for s in (2, 4, 8, 16, 32, 64, 128, 256, 512):
        args += ["(", "mpr:card", "-resize", f"{s}x{s}!", "-resize", size, ")", "-compose", "over", "-composite"]
    args += [
        "(", "mpr:card", "(", "mpr:card", "-alpha", "extract", "-threshold", "0", ")",
        "-compose", "copyalpha", "-composite", ")", "-compose", "over", "-composite",
        "(", "mpr:card", "-alpha", "extract", ")", "-compose", "copyalpha", "-composite",
        "-type", "TrueColorAlpha", *png_out("PNG32", dst),
    ]
    magick(*args)


def regrade_green(png: Path, recolor: Recolor) -> None:
    """Recolour a colour PNG in place to green, keeping its transparency.

    Each pixel's brightness picks a colour from the dark-mid-light ramp.
    Greyish pixels, such as twigs and stems, keep a muted version of their own
    colour: `twig_level` is an ImageMagick -level range on inverted saturation
    (higher values select only greyer pixels) and `twig_modulate` the twig
    colour's -modulate brightness,saturation percentages.
    """
    magick(
        png, "-write", "mpr:leaf", "+delete",
        "(", "mpr:leaf", "-alpha", "off", "-colorspace", "gray", "-level", "0%,60%",
        "(", "-size", "1x128", f"gradient:{recolor.dark}-{recolor.mid}",
        "-size", "1x128", f"gradient:{recolor.mid}-{recolor.light}", "-append", "-rotate", "90", ")",
        "-clut", ")",
        "(", "mpr:leaf", "-alpha", "off", "-modulate", recolor.twig_modulate, ")",
        "(", "mpr:leaf", "-alpha", "off", "-colorspace", "HSL", "-channel", "G", "-separate", "+channel",
        "-negate", "-level", recolor.twig_level, "-blur", "0x1", ")",
        "-composite",
        "(", "mpr:leaf", "-alpha", "extract", ")", "-alpha", "off", "-compose", "copyalpha", "-composite",
        "-type", "TrueColorAlpha", *png_out("PNG32", png),
    )


def prepare_clump(kind: str, folder: Path) -> Path:
    """Write `<folder>/<kind>-clump.png`, a 256 x 256 spray of leaves on one card.

    Needs `<folder>/<kind>-color.png` from prepare_card. Leaves fan out from the
    bottom centre, so the card attaches to a branch like a single leaf. Returns
    the clump image's path.
    """
    src = folder / f"{kind}-color.png"
    width, height = image_size(src)

    # Scale and rotate each copy around the leaf's bottom centre, then place it.
    args: list[str | Path] = ["-size", "1024x1024", "xc:none"]
    for scale, angle, x, y in CLUMP_COPIES:
        args += [
            "(", src, "-alpha", "set", "-virtual-pixel", "transparent",
            "-define", "distort:viewport=1024x1024+0+0",
            "-distort", "SRT", f"{width // 2},{height - 1} {scale:.2f} {angle} {x},{y}", ")",
            "-compose", "over", "-composite",
        ]

    # Shrink to 256 x 256, then fill transparent areas with nearby colours.
    raw = folder / f"{kind}-clump-raw.png"
    clump = folder / f"{kind}-clump.png"
    magick(*args, "-filter", "Lanczos", "-resize", "256x256", *png_out("PNG32", raw))
    bleed_color(raw, clump)
    raw.unlink()
    return clump
