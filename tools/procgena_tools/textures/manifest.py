"""The `manifest` and `images` commands: describe published files for download."""

from __future__ import annotations

import json
import shutil
from pathlib import Path

from .cards import prepare_card
from .config import BUCKET_GS, BUCKET_URL, FORMATS, SIZES, Project, SurfaceSet
from .util import ToolError, file_hash, image_size, log, magick, require, run, write_json

SIZE_NOTES = {name: f"fits within {px} x {px} pixels" for name, px in SIZES}
FORMAT_NOTES = {
    "bc7": "BC7 with Zstandard compression, for GPUs that support BC7, commonly desktops",
    "uastc": "Basis Universal UASTC with Zstandard compression; the loader converts it to a format the GPU supports",
}
MAP_NOTES = {
    "color": "surface colour, with straight-alpha transparency for cards; load as sRGB",
    "normal": "surface lighting detail, OpenGL (+Y) convention; load as linear data",
    "orh": "R occlusion (shades hollows), G roughness (light is matte), B height (0-1, light is raised); linear data",
    "orm": "R occlusion (shades hollows), G roughness (light is matte), B metalness (light is metal, zero without a metalness map); linear data",
    "roughness": "how matte the surface is, identical in R, G and B; load as linear data",
    "clump": "256 x 256 spray of leaves with transparency, for distant trees; load as sRGB",
}


def write_manifest(project: Project, sources: Path, assets: Path) -> None:
    """Write `assets/assets_manifest.json`, listing every KTX2 file `convert` made.

    Surface attribution comes from each set's `sources/<id>/info.json`, written
    by `fetch`. Raises ToolError if any expected file is missing.
    """
    require("ktx")
    surface_maps = surface_texture_maps(project)

    # Describe each surface set's files, all sizes, maps and formats.
    entries = []
    for surface in project.sets:
        files = []
        for size, _ in SIZES:
            for map_name in surface_maps:
                for fmt in FORMATS:
                    rel = f"{surface.category}/{surface.id}-{size}-{map_name}-{fmt}.ktx2"
                    files.append(describe_ktx(assets, rel, {"size": size, "map": map_name, "format": fmt}))
        entries.append(asset_entry(surface.id, surface.category, read_info(sources, surface), files))

    # Describe each card's files; cards have one size.
    for card in project.cards:
        files = []
        for map_name in card_texture_maps(project, card):
            for fmt in FORMATS:
                rel = f"{project.card_category}/{card.kind}-{map_name}-{fmt}.ktx2"
                files.append(describe_ktx(assets, rel, {"map": map_name, "format": fmt}))
        entries.append(asset_entry(card.kind, project.card_category, card.attribution, files))

    # Write the manifest with short notes on the paths, maps and formats.
    patterns = {}
    if project.sets:
        patterns["surfaces"] = "<category>/<id>-<size>-<map>-<format>.ktx2"
    if project.cards:
        patterns["cards"] = "<category>/<id>-<map>-<format>.ktx2"
    maps = set(surface_maps if project.sets else ())
    for card in project.cards:
        maps.update(card_texture_maps(project, card))
    manifest = {
        "version": 1,
        "baseUrl": f"{BUCKET_URL}{project.bucket}/",
        "pathPatterns": patterns,
        "sizes": SIZE_NOTES,
        "maps": {name: note for name, note in MAP_NOTES.items() if name in maps},
        "formats": FORMAT_NOTES,
        "assets": entries,
    }
    write_json(assets / "assets_manifest.json", manifest)
    log(f"Wrote {assets / 'assets_manifest.json'}")
    print_upload(project, assets, folders=categories(project), extra=["assets_manifest.json", "LICENSE.md"])


def write_images(project: Project, sources: Path, assets: Path) -> None:
    """Write JPEG and PNG images for exported models, and their manifest.

    Surface sets become `images/<category>/<id>-<size>-<map>.jpg`; cards become
    `images/<category>/<kind>-<map>.png` at their source size. Existing output
    is overwritten. Raises ToolError if a source image is missing.
    """
    require("magick")
    out = assets / "images"
    entries = []

    # Surface sets: fit each map within px x px as JPEG.
    for surface in project.sets:
        src = sources / surface.id
        needed = ["color", "normal", "roughness"] + (["occlusion"] if project.layout == "orh" else [])
        for name in needed:
            if not (src / f"{name}.jpg").is_file():
                raise ToolError(f"missing {src / name}.jpg; run `textures fetch {project.name}` first")
        files = []
        for size, px in SIZES:
            prefix = out / surface.category / f"{surface.id}-{size}"
            prefix.parent.mkdir(parents=True, exist_ok=True)
            for map_name in write_surface_images(project, src, prefix, px):
                rel = f"{surface.category}/{surface.id}-{size}-{map_name}.jpg"
                files.append(describe_image(out, rel, {"size": size, "map": map_name}))
        entries.append(asset_entry(surface.id, surface.category, read_info(sources, surface), files))

    # Cards: colour and any normal map as PNGs at their source size.
    for card in project.cards:
        folder = out / project.card_category
        prepare_card(project, card, folder)
        files = [
            describe_image(out, f"{project.card_category}/{card.kind}-{m}.png", {"map": m})
            for m in project.card_maps(card)
        ]
        entries.append(asset_entry(card.kind, project.card_category, card.attribution, files))

    # Write the manifest with short notes on the paths and maps.
    patterns = {}
    if project.sets:
        patterns["surfaces"] = "<category>/<id>-<size>-<map>.jpg"
    if project.cards:
        patterns["cards"] = "<category>/<id>-<map>.png"
    maps = {"color", "normal"} | ({"orm"} if project.layout == "orh" else {"roughness"})
    manifest = {
        "version": 1,
        "baseUrl": f"{BUCKET_URL}{project.bucket}/images/",
        "pathPatterns": patterns,
        "sizes": SIZE_NOTES,
        "maps": {name: note for name, note in MAP_NOTES.items() if name in maps},
        "assets": entries,
    }
    write_json(out / "images_manifest.json", manifest)
    shutil.copyfile(project.license_file, out / "LICENSE.md")
    log(f"Wrote {out}")
    print_upload(project, assets, folders=["images"], extra=[])


def write_surface_images(project: Project, src: Path, prefix: Path, px: int) -> list[str]:
    """Write one size of a surface set's export images and return their map names."""
    resize = ["-filter", "Lanczos", "-resize", f"{px}x{px}"]

    # Resize colour in linear light to preserve average brightness. Keep full
    # colour resolution (4:4:4) so normal directions are not blurred.
    magick(
        src / "color.jpg", "-colorspace", "RGB", *resize, "-colorspace", "sRGB",
        "-strip", "-quality", "92", f"{prefix}-color.jpg",
    )
    magick(src / "normal.jpg", *resize, "-strip", "-sampling-factor", "4:4:4", "-quality", "95", f"{prefix}-normal.jpg")

    if project.layout == "roughness":
        # Greyscale roughness in all three channels, including green, where glTF reads it.
        magick(src / "roughness.jpg", *resize, "-type", "TrueColor", "-strip", "-quality", "92", f"{prefix}-roughness.jpg")
        return ["color", "normal", "roughness"]

    # Occlusion in red, roughness in green and metalness in blue for glTF; sets
    # without a metalness map get zero metalness.
    metal = src / "metalness.jpg"
    metal_map = ["(", metal, "-channel", "R", "-separate", "+channel", ")"] if metal.is_file() else []
    metal_zero = [] if metal.is_file() else ["(", "-clone", "0", "-evaluate", "set", "0", ")"]
    magick(
        "(", src / "occlusion.jpg", "-channel", "R", "-separate", "+channel", ")",
        "(", src / "roughness.jpg", "-channel", "R", "-separate", "+channel", ")",
        *metal_map, *resize, *metal_zero,
        "-set", "colorspace", "sRGB", "-combine", "-alpha", "off", "-type", "TrueColor",
        "-strip", "-sampling-factor", "4:4:4", "-quality", "95", f"{prefix}-orm.jpg",
    )
    return ["color", "normal", "orm"]


def surface_texture_maps(project: Project) -> tuple[str, ...]:
    """KTX2 maps written for each surface set."""
    return ("color", "normal", "orh") if project.layout == "orh" else ("color", "normal", "roughness")


def card_texture_maps(project: Project, card) -> tuple[str, ...]:
    """KTX2 maps written for a card, including its clump image when enabled."""
    return project.card_maps(card) + (("clump",) if project.clump_images else ())


def categories(project: Project) -> list[str]:
    """Output folders under assets/textures/, in first-use order."""
    names = [s.category for s in project.sets] + ([project.card_category] if project.cards else [])
    return list(dict.fromkeys(names))


def read_info(sources: Path, surface: SurfaceSet) -> dict:
    """Attribution saved by `fetch` in `sources/<id>/info.json`."""
    path = sources / surface.id / "info.json"
    if not path.is_file():
        raise ToolError(f"missing {path}; run `textures fetch` first")
    return json.loads(path.read_text(encoding="utf-8"))


def asset_entry(asset_id: str, category: str, info: dict, files: list[dict]) -> dict:
    """One manifest asset: its id, category, attribution and files."""
    return {"id": asset_id, "category": category, **info, "files": files}


def describe_ktx(assets: Path, rel: str, extra: dict) -> dict:
    """Manifest entry for a KTX2 file, with dimensions read from its header."""
    file = assets / rel
    if not file.is_file():
        raise ToolError(f"missing {file}; run `textures convert` first")
    header = json.loads(run("ktx", "info", "--format", "json", file))["header"]
    return describe(file, rel, extra, header["pixelWidth"], header["pixelHeight"])


def describe_image(out: Path, rel: str, extra: dict) -> dict:
    """Manifest entry for a JPEG or PNG image."""
    file = out / rel
    width, height = image_size(file)
    return describe(file, rel, extra, width, height)


def describe(file: Path, rel: str, extra: dict, width: int, height: int) -> dict:
    """Manifest entry: path, extra fields, dimensions, byte count and SHA-256."""
    return {
        "path": rel,
        **extra,
        "width": width,
        "height": height,
        "bytes": file.stat().st_size,
        "sha256": file_hash(file, "sha256"),
    }


def print_upload(project: Project, assets: Path, folders: list[str], extra: list[str]) -> None:
    """Print the gcloud commands that publish these files to the bucket."""
    target = f"{BUCKET_GS}{project.bucket}/"
    log("Upload with:")
    for folder in folders:
        log(f"  gcloud storage rsync --recursive {assets / folder} {target}{folder}")
    if extra:
        log(f"  gcloud storage cp {' '.join(str(assets / f) for f in extra)} {target}")
