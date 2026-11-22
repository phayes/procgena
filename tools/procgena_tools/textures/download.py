"""The `download` command: fetch finished textures listed in a project's manifest."""

from __future__ import annotations

import json
from pathlib import Path

from .util import ToolError, fetch_file, file_hash, log


def download(manifest_path: Path, dest: Path, sizes: list[str], formats: list[str] | None) -> None:
    """Download every manifest file whose size and format are chosen into `dest`.

    Files without a size (cards) or without a format (export images) always
    match; `formats` of None ignores formats. Files whose SHA-256 already
    matches are kept; each download is checked before it replaces anything.
    """
    if not manifest_path.is_file():
        raise ToolError(f"missing {manifest_path}")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    base_url = manifest["baseUrl"]

    # Pick the files to fetch.
    chosen = [
        f
        for asset in manifest["assets"]
        for f in asset["files"]
        if ("size" not in f or f["size"] in sizes)
        and (formats is None or "format" not in f or f["format"] in formats)
    ]
    if not chosen:
        raise ToolError("no files match the chosen sizes and formats")

    # Download each missing or changed file to a temporary name, check it, then
    # move it into place.
    kept = downloaded = 0
    for entry in chosen:
        file = dest / entry["path"]
        if file.is_file() and file_hash(file, "sha256") == entry["sha256"]:
            kept += 1
            continue
        part = file.with_name(file.name + ".part")
        url = base_url + entry["path"]
        fetch_file(url, part)
        if file_hash(part, "sha256") != entry["sha256"]:
            part.unlink()
            raise ToolError(f"checksum mismatch for {url}")
        part.replace(file)
        downloaded += 1
        log(f"downloaded {entry['path']}")
    log(f"Downloaded {downloaded} files and kept {kept} up-to-date files in {dest}")
