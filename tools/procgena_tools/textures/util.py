"""Shared helpers: HTTP downloads, checksums, and running image tools."""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path

USER_AGENT = (
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/140.0 Safari/537.36"
)


class ToolError(Exception):
    """A failed step, reported to the user without a traceback."""


def log(message: str) -> None:
    """Print a progress line."""
    print(message, flush=True)


def fetch_bytes(url: str) -> bytes:
    """Download `url` and return its body. Raises ToolError on HTTP errors."""
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            return response.read()
    except OSError as error:
        raise ToolError(f"download failed for {url}: {error}") from error


def fetch_json(url: str):
    """Download `url` and parse it as JSON."""
    return json.loads(fetch_bytes(url))


def fetch_file(url: str, dest: Path) -> None:
    """Download `url` to `dest`, creating its folder. Raises ToolError on errors."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=600) as response, dest.open("wb") as out:
            shutil.copyfileobj(response, out)
    except OSError as error:
        dest.unlink(missing_ok=True)
        raise ToolError(f"download failed for {url}: {error}") from error


def file_hash(path: Path, algorithm: str) -> str:
    """Hex digest of a file, for example with "sha256" or "md5"."""
    digest = hashlib.new(algorithm)
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def require(*programs: str) -> None:
    """Stop with an install hint if any external program is not on PATH."""
    hints = {
        "magick": "ImageMagick 7",
        "ktx": "KTX-Software 4.4+",
        "ctt": "ctt-cli 0.6 (cargo install ctt-cli --version 0.6.1 --locked)",
    }
    missing = [p for p in programs if shutil.which(p) is None]
    if missing:
        names = ", ".join(f"{p} ({hints.get(p, p)})" for p in missing)
        raise ToolError(f"install {names} first")


def run(*args: str | Path) -> str:
    """Run a command and return its standard output. Raises ToolError if it fails."""
    command = [str(a) for a in args]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise ToolError(f"{command[0]} failed: {' '.join(command)}")
    return result.stdout


def magick(*args: str | Path) -> str:
    """Run ImageMagick 7 with the given arguments."""
    return run("magick", *args)


def image_size(path: Path) -> tuple[int, int]:
    """Width and height of an image in pixels."""
    width, height = magick("identify", "-format", "%w %h\n", path).splitlines()[0].split()
    return int(width), int(height)


def write_json(path: Path, value) -> None:
    """Write `value` as indented JSON with a trailing newline."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
