"""Reads a project's textures.yml and the folders its textures live in."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

import yaml

# The workspace root: tools/procgena_tools/textures/config.py is four levels down.
WORKSPACE = Path(__file__).resolve().parents[3]

# Public bucket that serves the finished textures, one folder per project.
BUCKET_URL = "https://storage.googleapis.com/ze_textures/"
BUCKET_GS = "gs://ze_textures/"

# Surface texture sizes a textures.yml may list: name and pixels per side.
SIZE_PIXELS = {"128": 128, "256": 256, "512": 512, "1k": 1024, "2k": 2048}
# Sizes written for projects whose textures.yml lists none.
DEFAULT_SIZES = ("512", "1k", "2k")
FORMATS = ("bc7", "uastc")
LAYOUTS = ("orh", "roughness")
SET_SOURCES = ("polyhaven", "ambientcg", "sharetextures", "texturecan", "manual")
CARD_SOURCES = ("ez-tree", "nobiax", "generated")

# Attribution for card sources that are whole libraries.
LIBRARY_ATTRIBUTION = {
    "ez-tree": {
        "authors": ["Daniel Greenheck"],
        "source": "ez-tree",
        "sourceUrl": "https://github.com/dgreenheck/ez-tree",
        "license": "MIT",
    },
    "nobiax": {
        "authors": ["Nobiax (yughues)"],
        "source": "nobiax",
        "sourceUrl": "https://nobiax.deviantart.com",
        "license": "CC0-1.0",
    },
}


class ConfigError(Exception):
    """A textures.yml that is missing, unreadable or inconsistent."""


@dataclass(frozen=True)
class Recolor:
    """Settings that regrade a card's colour image to a green ramp."""

    dark: str
    mid: str
    light: str
    twig_level: str
    twig_modulate: str


@dataclass(frozen=True)
class SurfaceSet:
    """One tiling surface texture, such as a rock or bark set."""

    id: str
    category: str
    source: str
    source_id: str
    # Attribution written to info.json, for sources whose site has no API.
    attribution: dict | None = None
    # File names delivered by hand, by map, for `manual` sets.
    originals: dict[str, str] = field(default_factory=dict)
    # `directx` when the delivered normal map points green down (-Y).
    normal_convention: str = "opengl"


@dataclass(frozen=True)
class Card:
    """One transparent card image, such as a leaf spray or stalk."""

    kind: str
    source: str
    path: str
    title: str
    attribution: dict
    recolor: Recolor | None = None


@dataclass(frozen=True)
class Project:
    """A project's texture settings and folders."""

    name: str
    root: Path
    # Git-ignored folder holding downloaded and hand-supplied sources.
    prior_art: Path
    bucket: str
    layout: str
    # Surface texture sizes to write, smallest first: name and pixels per side.
    sizes: tuple[tuple[str, int], ...]
    sets: list[SurfaceSet]
    card_category: str | None
    clump_images: bool
    cards: list[Card]

    @property
    def sources(self) -> Path:
        """Folder where `fetch` saves each surface set's 2k images."""
        return self.prior_art / "textures"

    @property
    def assets(self) -> Path:
        """Folder holding the finished KTX2 textures and their manifest."""
        return self.root / "assets" / "textures"

    @property
    def license_file(self) -> Path:
        """The texture licence and attribution page, published with the textures."""
        return self.assets / "LICENSE.md"

    @property
    def surface_maps(self) -> tuple[str, ...]:
        """Source images every surface set needs, by name."""
        if self.layout == "orh":
            return ("color", "normal", "roughness", "occlusion", "height")
        return ("color", "normal", "roughness")

    def card_maps(self, card: Card) -> tuple[str, ...]:
        """Maps a card has: every card has colour; some have a normal map."""
        if card.source == "nobiax":
            return ("color", "normal")
        if card.source == "generated":
            normal = self.prior_art / f"{card.path}-normal.png"
            return ("color", "normal") if normal.is_file() else ("color",)
        return ("color",)


def project_names() -> list[str]:
    """Names of the workspace folders that have a textures.yml."""
    return sorted(p.parent.name for p in WORKSPACE.glob("*/textures.yml"))


def load(name: str, prior_art: Path | None = None) -> Project:
    """Load `<workspace>/<name>/textures.yml`. Raises ConfigError on problems.

    `prior_art` replaces the default `<workspace>/<name>/prior_art` source folder.
    """
    root = WORKSPACE / name
    path = root / "textures.yml"
    if not path.is_file():
        known = ", ".join(project_names())
        raise ConfigError(f"no textures.yml for project '{name}'; known projects: {known}")
    with path.open(encoding="utf-8") as f:
        data = yaml.safe_load(f) or {}

    # Read the project-wide settings.
    layout = data.get("layout", "orh")
    if layout not in LAYOUTS:
        raise ConfigError(f"{path}: layout must be one of {', '.join(LAYOUTS)}")
    bucket = _required(data, "bucket", path)
    default_category = data.get("category")
    size_names = [str(n) for n in data.get("sizes") or DEFAULT_SIZES]
    unknown_sizes = [n for n in size_names if n not in SIZE_PIXELS]
    if unknown_sizes:
        known = ", ".join(SIZE_PIXELS)
        raise ConfigError(f"{path}: unknown sizes {', '.join(unknown_sizes)}; known sizes: {known}")
    sizes = tuple(sorted({(n, SIZE_PIXELS[n]) for n in size_names}, key=lambda s: s[1]))

    # Read the surface sets.
    sets = []
    for entry in data.get("sets") or []:
        set_id = _required(entry, "id", path)
        source = entry.get("source", "polyhaven")
        if source not in SET_SOURCES:
            raise ConfigError(f"{path}: unknown source '{source}' for set {set_id}")
        category = entry.get("category", default_category)
        if not category:
            raise ConfigError(f"{path}: set {set_id} has no category")
        attribution = None
        if source in ("sharetextures", "texturecan", "manual"):
            attribution = _attribution(entry, source, path, set_id)
        if source == "manual" and not entry.get("originals"):
            raise ConfigError(f"{path}: manual set {set_id} lists no originals")
        sets.append(
            SurfaceSet(
                id=set_id,
                category=category,
                source=source,
                source_id=str(entry.get("source_id", set_id)),
                attribution=attribution,
                originals=entry.get("originals") or {},
                normal_convention=entry.get("normal_convention", "opengl"),
            )
        )

    # Read the cards.
    cards_data = data.get("cards") or {}
    cards = []
    for entry in cards_data.get("kinds") or []:
        kind = _required(entry, "kind", path)
        source = _required(entry, "source", path)
        if source not in CARD_SOURCES:
            raise ConfigError(f"{path}: unknown source '{source}' for card {kind}")
        title = _required(entry, "title", path)
        if source == "generated":
            attribution = _attribution(entry, source, path, kind)
        else:
            attribution = {"title": title, **LIBRARY_ATTRIBUTION[source]}
        recolor = Recolor(**entry["recolor"]) if entry.get("recolor") else None
        cards.append(
            Card(
                kind=kind,
                source=source,
                path=_required(entry, "path", path),
                title=title,
                attribution=attribution,
                recolor=recolor,
            )
        )
    if cards and not cards_data.get("category"):
        raise ConfigError(f"{path}: cards need a category")

    # Ids name files, so they must be unique within the project.
    ids = [s.id for s in sets] + [c.kind for c in cards]
    duplicates = sorted({i for i in ids if ids.count(i) > 1})
    if duplicates:
        raise ConfigError(f"{path}: duplicate ids {', '.join(duplicates)}")

    return Project(
        name=name,
        root=root,
        prior_art=prior_art or root / "prior_art",
        bucket=bucket,
        layout=layout,
        sizes=sizes,
        sets=sets,
        card_category=cards_data.get("category"),
        clump_images=bool(cards_data.get("clump_images", False)),
        cards=cards,
    )


def _required(entry: dict, key: str, path: Path):
    """Return entry[key], or raise ConfigError naming the file."""
    if key not in entry or entry[key] in (None, ""):
        raise ConfigError(f"{path}: missing '{key}' in {entry}")
    return entry[key]


def _attribution(entry: dict, source: str, path: Path, name: str) -> dict:
    """Attribution given in textures.yml, in the field names info.json uses."""
    for key in ("title", "authors", "source_url", "license"):
        if key not in entry:
            raise ConfigError(f"{path}: {name} needs '{key}' (source {source})")
    return {
        "title": entry["title"],
        "authors": list(entry["authors"]),
        "source": entry.get("site", source),
        "sourceUrl": entry["source_url"],
        "license": entry["license"],
    }
