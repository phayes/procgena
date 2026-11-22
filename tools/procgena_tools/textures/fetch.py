"""The `fetch` command: download each surface set's 2k source images."""

from __future__ import annotations

import re
import shutil
import stat
import tempfile
import zipfile
from pathlib import Path

from .config import Project, SurfaceSet
from .util import ToolError, fetch_bytes, fetch_file, fetch_json, file_hash, log, magick, require, write_json

# Map names on each site, by our file name.
POLYHAVEN_MAPS = {
    "color": "Diffuse",
    "normal": "nor_gl",
    "roughness": "Rough",
    "occlusion": "AO",
    "height": "Displacement",
}
AMBIENTCG_MAPS = {
    "color": "Color",
    "normal": "NormalGL",
    "roughness": "Roughness",
    "occlusion": "AmbientOcclusion",
    "height": "Displacement",
    "metalness": "Metalness",
}
SHARETEXTURES_MAPS = {
    "color": "diffuse",
    "normal": "normal",
    "roughness": "roughness",
    "occlusion": "ao",
    "height": "displacement",
}
TEXTURECAN_MAPS = {
    "color": "color",
    "normal": "normal_opengl",
    "roughness": "roughness",
    "occlusion": "ao",
    "height": "height",
}


def fetch(project: Project, dest: Path, ids: list[str]) -> None:
    """Download the chosen sets (all when `ids` is empty) into `dest/<id>/`."""
    require("magick")
    sets = select_sets(project, ids)
    with tempfile.TemporaryDirectory() as stage_dir:
        stage = Path(stage_dir)
        for surface in sets:
            out = dest / surface.id
            out.mkdir(parents=True, exist_ok=True)
            fetcher = {
                "polyhaven": fetch_polyhaven,
                "ambientcg": fetch_ambientcg,
                "sharetextures": fetch_sharetextures,
                "texturecan": fetch_texturecan,
                "manual": prepare_manual,
            }[surface.source]
            fetcher(project, surface, out, stage)
    log(f"Images are in {dest}")


def select_sets(project: Project, ids: list[str]) -> list[SurfaceSet]:
    """The sets named in `ids`, in config order, or every set when `ids` is empty."""
    if not ids:
        return project.sets
    known = {s.id for s in project.sets}
    unknown = [i for i in ids if i not in known]
    if unknown:
        raise ToolError(f"unknown set ids for {project.name}: {', '.join(unknown)}")
    return [s for s in project.sets if s.id in ids]


def has_images(project: Project, out: Path) -> bool:
    """Whether `out` already holds every source image the project needs."""
    return all((out / f"{name}.jpg").is_file() for name in project.surface_maps)


def fetch_polyhaven(project: Project, surface: SurfaceSet, out: Path, stage: Path) -> None:
    """Download a Poly Haven set, keeping files that match Poly Haven's checksums."""
    sid = surface.source_id

    # Save the title and authors for the manifests.
    info = fetch_json(f"https://api.polyhaven.com/info/{sid}")
    write_json(
        out / "info.json",
        {
            "title": info["name"],
            "authors": sorted(info["authors"]),
            "source": "polyhaven",
            "sourceUrl": f"https://polyhaven.com/a/{sid}",
            "license": "CC0-1.0",
        },
    )

    # Download each needed 2k JPEG unless a matching copy is already there.
    listing = fetch_json(f"https://api.polyhaven.com/files/{sid}")
    for name in project.surface_maps:
        try:
            entry = listing[POLYHAVEN_MAPS[name]]["2k"]["jpg"]
        except KeyError as error:
            raise ToolError(f"Poly Haven {sid} has no 2k {POLYHAVEN_MAPS[name]} JPEG") from error
        file = out / f"{name}.jpg"
        if file.is_file() and file_hash(file, "md5") == entry["md5"]:
            continue
        fetch_file(entry["url"], file)
        if file_hash(file, "md5") != entry["md5"]:
            file.unlink()
            raise ToolError(f"checksum mismatch for {entry['url']}")
        log(f"downloaded {surface.id}/{name}.jpg")


def fetch_ambientcg(project: Project, surface: SurfaceSet, out: Path, stage: Path) -> None:
    """Download an ambientCG set's 2k JPEG archive when any needed image is missing."""
    sid = surface.source_id
    found = fetch_json(
        f"https://ambientcg.com/api/v2/full_json?id={sid}&include=downloadData,displayData"
    )["foundAssets"]
    if not found:
        raise ToolError(f"ambientCG has no asset {sid}")
    asset = found[0]

    # Save the title for the manifests. ambientCG names no individual authors,
    # so the site is credited.
    write_json(
        out / "info.json",
        {
            "title": asset["displayName"],
            "authors": ["ambientCG"],
            "source": "ambientcg",
            "sourceUrl": f"https://ambientcg.com/a/{sid}",
            "license": "CC0-1.0",
        },
    )
    if has_images(project, out):
        return

    # Download the 2k JPEG archive and check its size and contents.
    downloads = asset["downloadFolders"]["default"]["downloadFiletypeCategories"]["zip"]["downloads"]
    entry = next((d for d in downloads if d["attribute"] == "2K-JPG"), None)
    if entry is None:
        raise ToolError(f"ambientCG {sid} has no 2K-JPG download")
    archive = stage / f"{sid}.zip"
    fetch_file(entry["downloadLink"], archive)
    if archive.stat().st_size != entry["size"]:
        raise ToolError(f"damaged download for {entry['downloadLink']}: wrong size")
    unpacked = unpack(archive, stage / sid)

    # Copy the maps the layout uses; occlusion and metalness are optional, and
    # metalness is kept only for the orh layout's exported orm images.
    wanted = list(project.surface_maps) + (["metalness"] if project.layout == "orh" else [])
    for name in wanted:
        site_name = AMBIENTCG_MAPS[name]
        file = unpacked / f"{sid}_2K-JPG_{site_name}.jpg"
        if file.is_file():
            shutil.copyfile(file, out / f"{name}.jpg")
            log(f"downloaded {surface.id}/{name}.jpg")
        elif name in project.surface_maps and name != "occlusion":
            raise ToolError(f"ambientCG {sid} has no {site_name} map")
    if "occlusion" in project.surface_maps and not (unpacked / f"{sid}_2K-JPG_AmbientOcclusion.jpg").is_file():
        write_white_occlusion(out, surface)


def write_white_occlusion(out: Path, surface: SurfaceSet) -> None:
    """Write a plain white occlusion.jpg, which adds no shading."""
    magick(
        out / "color.jpg", "-fill", "white", "-colorize", "100", "-colorspace", "Gray",
        "-quality", "95", out / "occlusion.jpg",
    )
    log(f"wrote white {surface.id}/occlusion.jpg")


def fetch_sharetextures(project: Project, surface: SurfaceSet, out: Path, stage: Path) -> None:
    """Download a ShareTextures set's 2k archive when any needed image is missing."""
    write_json(out / "info.json", surface.attribution)
    if has_images(project, out):
        return

    # Download and check the archive, then copy maps that end in -<map>.jpg.
    sid = surface.source_id
    archive = stage / f"{sid}.zip"
    fetch_file(f"https://files.sharetextures.com/file/Share-Textures/{sid}-2K.zip", archive)
    unpacked = unpack(archive, stage / sid)
    for name, site_name in SHARETEXTURES_MAPS.items():
        file = find_one(unpacked, f"*-{site_name}.jpg")
        if file:
            shutil.copyfile(file, out / f"{name}.jpg")
            log(f"downloaded {surface.id}/{name}.jpg")
        elif name != "roughness":
            raise ToolError(f"ShareTextures {sid} has no {site_name} map")

    # Some sets have a specular map instead of roughness: shiny areas are polished.
    if not (out / "roughness.jpg").is_file():
        specular = find_one(unpacked, "*-specular.jpg")
        if specular is None:
            raise ToolError(f"ShareTextures {sid} has no roughness or specular map")
        magick(specular, "-colorspace", "Gray", "-negate", "-quality", "95", out / "roughness.jpg")
        log(f"wrote {surface.id}/roughness.jpg from the specular map")


def fetch_texturecan(project: Project, surface: SurfaceSet, out: Path, stage: Path) -> None:
    """Download a TextureCan set's 2k archive when any needed image is missing."""
    write_json(out / "info.json", surface.attribution)
    if has_images(project, out):
        return

    # Find the 2k archive's address on the texture's page, then download it.
    sid = surface.source_id
    page = fetch_bytes(f"https://www.texturecan.com/details/{sid}/").decode("utf-8", "replace")
    match = re.search(r'/downloads/[^"]*_2k_[^"]*\.zip', page)
    if match is None:
        raise ToolError(f"no 2k download on TextureCan page {sid}")
    archive = stage / f"texturecan_{sid}.zip"
    fetch_file(f"https://www.texturecan.com{match.group(0)}", archive)
    unpacked = unpack(archive, stage / f"texturecan_{sid}", skip_prefix="__MACOSX/")

    # Save each map as a JPEG; images end in _<map>_2k.jpg or .png.
    for name, site_name in TEXTURECAN_MAPS.items():
        file = find_one(unpacked, f"*_{site_name}_2k.jpg") or find_one(unpacked, f"*_{site_name}_2k.png")
        if file is None:
            raise ToolError(f"TextureCan {sid} has no {site_name} map")
        if name in ("color", "normal"):
            magick(file, "-alpha", "off", "-depth", "8", "-quality", "97", "-sampling-factor", "4:4:4", out / f"{name}.jpg")
        else:
            magick(file, "-alpha", "off", "-colorspace", "Gray", "-depth", "8", "-quality", "97", out / f"{name}.jpg")
        log(f"downloaded {surface.id}/{name}.jpg")


def prepare_manual(project: Project, surface: SurfaceSet, out: Path, stage: Path) -> None:
    """Convert hand-downloaded files in `out/original/` to the standard names."""
    write_json(out / "info.json", surface.attribution)
    originals = out / "original"

    # Use a specular map in place of roughness when the set has no roughness map.
    rough_key = "roughness" if "roughness" in surface.originals else "specular"
    keys = ["color", "normal", rough_key, "occlusion", "height"]
    for key in keys:
        name = surface.originals.get(key)
        if name is None:
            raise ToolError(f"{surface.id}: textures.yml lists no '{key}' original")
        if not (originals / name).is_file():
            raise ToolError(
                f"missing {originals / name}; download {surface.id} from "
                f"{surface.attribution['sourceUrl']} and save its files in {originals}"
            )

    # Save each map as a high-quality JPEG. Flip the green channel of DirectX
    # (-Y) normal maps to OpenGL (+Y), and invert a specular map so shiny areas
    # become polished (low roughness).
    flip = ["-channel", "G", "-negate", "+channel"] if surface.normal_convention == "directx" else []
    for key in keys:
        source = originals / surface.originals[key]
        if key == "specular":
            magick(source, "-alpha", "off", "-colorspace", "Gray", "-negate", "-quality", "97", out / "roughness.jpg")
            name = "roughness"
        elif key == "color":
            magick(source, "-alpha", "off", "-quality", "97", "-sampling-factor", "4:4:4", out / "color.jpg")
            name = key
        elif key == "normal":
            magick(source, "-alpha", "off", *flip, "-quality", "97", "-sampling-factor", "4:4:4", out / "normal.jpg")
            name = key
        else:
            magick(source, "-alpha", "off", "-colorspace", "Gray", "-quality", "97", out / f"{key}.jpg")
            name = key
        log(f"prepared {surface.id}/{name}.jpg")


def unpack(archive: Path, folder: Path, skip_prefix: str | None = None) -> Path:
    """Check a zip archive for damage and extract it into `folder`."""
    try:
        with zipfile.ZipFile(archive) as z:
            if z.testzip() is not None:
                raise ToolError(f"damaged download {archive.name}")
            members = [m for m in z.namelist() if not (skip_prefix and m.startswith(skip_prefix))]
            z.extractall(folder, members)
    except zipfile.BadZipFile as error:
        raise ToolError(f"damaged download {archive.name}") from error

    # Some archives unpack as read-only folders, which cleanup cannot remove.
    for path in [folder, *folder.rglob("*")]:
        path.chmod(path.stat().st_mode | stat.S_IWUSR)
    return folder


def find_one(folder: Path, pattern: str) -> Path | None:
    """The first file under `folder` matching a glob pattern, in sorted order."""
    matches = sorted(p for p in folder.rglob(pattern) if p.is_file())
    return matches[0] if matches else None
