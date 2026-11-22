"""The `convert` command: turn source images into KTX2 GPU textures."""

from __future__ import annotations

import tempfile
from pathlib import Path

from .cards import prepare_card, prepare_clump
from .config import SIZES, Project, SurfaceSet
from .util import ToolError, log, magick, require, run

# Vulkan formats that tell the UASTC encoder what the PNG holds.
KTX_FORMATS = {
    ("rgb", "srgb"): "R8G8B8_SRGB",
    ("rgb", "linear"): "R8G8B8_UNORM",
    ("rgba", "srgb"): "R8G8B8A8_SRGB",
}


def convert(project: Project, sources: Path, assets: Path, only: str | None, ids: list[str]) -> None:
    """Write KTX2 textures for the chosen sets and cards into `assets`.

    `only` limits the work to "surfaces", "cards" or "clumps" (clump images
    without replacing the regular card textures). `ids` picks sets and card
    kinds by id; empty means all. Existing outputs are overwritten.
    """
    require("magick", "ktx", "ctt")
    surfaces, cards = select(project, ids)
    if only == "clumps" and not project.clump_images:
        raise ToolError(f"{project.name} has no clump images")

    with tempfile.TemporaryDirectory() as stage_dir:
        stage = Path(stage_dir)
        if only in (None, "surfaces"):
            for surface in surfaces:
                convert_surface(project, surface, sources / surface.id, assets, stage)
        if only in (None, "cards", "clumps"):
            for card in cards:
                prepare_card(project, card, stage / "cards")
                prefix = assets / project.card_category / card.kind
                if only != "clumps":
                    encode(stage / "cards" / f"{card.kind}-color.png", prefix, "color", "srgb", "rgba", rdo=True)
                    if "normal" in project.card_maps(card):
                        encode(stage / "cards" / f"{card.kind}-normal.png", prefix, "normal", "linear", "rgb", rdo=False)
                if project.clump_images:
                    clump = prepare_clump(card.kind, stage / "cards")
                    encode(clump, prefix, "clump", "srgb", "rgba", rdo=True)
    log(f"Wrote {assets}")


def select(project: Project, ids: list[str]):
    """The surface sets and cards named in `ids`, or all of them when it is empty."""
    if not ids:
        return project.sets, project.cards
    known = {s.id for s in project.sets} | {c.kind for c in project.cards}
    unknown = [i for i in ids if i not in known]
    if unknown:
        raise ToolError(f"unknown ids for {project.name}: {', '.join(unknown)}")
    return [s for s in project.sets if s.id in ids], [c for c in project.cards if c.kind in ids]


def convert_surface(project: Project, surface: SurfaceSet, src: Path, assets: Path, stage: Path) -> None:
    """Write every size and map of one surface set as KTX2 files."""
    for name in project.surface_maps:
        if not (src / f"{name}.jpg").is_file():
            raise ToolError(f"missing {src / name}.jpg; run `textures fetch {project.name}` first")

    for size, px in SIZES:
        base = stage / f"{surface.id}-{size}"
        resize = ["-filter", "Lanczos", "-resize", f"{px}x{px}"]

        # Fit colour and normal within px x px, keeping the image's shape.
        # Resize colour in linear light to preserve average brightness.
        magick(
            src / "color.jpg", "-colorspace", "RGB", *resize, "-colorspace", "sRGB",
            "-alpha", "off", "-type", "TrueColor", f"PNG24:{base}-color.png",
        )
        magick(src / "normal.jpg", *resize, "-alpha", "off", "-type", "TrueColor", f"PNG24:{base}-normal.png")

        prefix = assets / surface.category / f"{surface.id}-{size}"
        encode(Path(f"{base}-color.png"), prefix, "color", "srgb", "rgb", rdo=True)
        encode(Path(f"{base}-normal.png"), prefix, "normal", "linear", "rgb", rdo=False)

        if project.layout == "orh":
            # Occlusion in red, roughness in green, height in blue, each read from
            # its source's red channel. Height is stretched to 0-1, clipping the
            # darkest and lightest 0.5%.
            magick(
                "(", src / "occlusion.jpg", "-channel", "R", "-separate", "+channel", ")",
                "(", src / "roughness.jpg", "-channel", "R", "-separate", "+channel", ")",
                "(", src / "height.jpg", "-channel", "R", "-separate", "+channel", "-contrast-stretch", "0.5%x0.5%", ")",
                *resize, "-set", "colorspace", "sRGB", "-combine",
                "-alpha", "off", "-type", "TrueColor", f"PNG24:{base}-orh.png",
            )
            encode(Path(f"{base}-orh.png"), prefix, "orh", "linear", "rgb", rdo=False)
        else:
            # Greyscale roughness in all three channels, including green, where
            # glTF and Bevy read it.
            magick(src / "roughness.jpg", *resize, "-alpha", "off", "-type", "TrueColor", f"PNG24:{base}-roughness.png")
            encode(Path(f"{base}-roughness.png"), prefix, "roughness", "linear", "rgb", rdo=True)


def encode(png: Path, prefix: Path, map_name: str, space: str, channels: str, rdo: bool) -> None:
    """Write `<prefix>-<map>-uastc.ktx2` and `<prefix>-<map>-bc7.ktx2` from a PNG.

    `space` is "srgb" for colour and "linear" for data maps; `channels` is
    "rgb" or "rgba". `rdo` trades a little UASTC quality for smaller files.
    Both files have mipmaps and Zstandard compression.
    """
    ktx_format = KTX_FORMATS.get((channels, space))
    if ktx_format is None:
        raise ToolError(f"unsupported {channels} {space} image {png}")
    prefix.parent.mkdir(parents=True, exist_ok=True)
    uastc = prefix.parent / f"{prefix.name}-{map_name}-uastc.ktx2"
    bc7 = prefix.parent / f"{prefix.name}-{map_name}-bc7.ktx2"

    run(
        "ktx", "create", "--format", ktx_format, "--assign-tf", space,
        "--encode", "uastc", "--uastc-quality", "3", *(["--uastc-rdo"] if rdo else []), "--zstd", "19",
        "--generate-mipmap", "--mipmap-filter", "lanczos4", png, uastc,
    )
    run(
        "ctt", png, "-o", bc7, "--format", "bc7", "--quality", "slow",
        "--input-color-space", space, "--output-color-space", space,
        "--mipmap", "--mipmap-filter", "lanczos3", "--zstd=19",
    )
