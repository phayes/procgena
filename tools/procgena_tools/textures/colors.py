"""The `average-colors` command: print each surface set's average colour as Rust."""

from __future__ import annotations

from pathlib import Path

from .config import Project
from .util import ToolError, magick, require


def average_colors(project: Project, sources: Path) -> None:
    """Print one `("<id>", [r, g, b]),` line per surface set, in config order.

    Channels are linear RGB from 0 to 1, averaged in linear light. Paste the
    lines into the project's average-colour table, such as
    BARK_AVERAGE_COLORS in procgena_tree; this command edits no files.
    """
    require("magick")
    for surface in project.sets:
        color = sources / surface.id / "color.jpg"
        if not color.is_file():
            raise ToolError(f"missing {color}; run `textures fetch {project.name}` first")
        r, g, b = (
            float(v)
            for v in magick(
                color, "-colorspace", "RGB", "-scale", "1x1!", "-format", "%[fx:r] %[fx:g] %[fx:b]", "info:"
            ).split()
        )
        print(f'    ("{surface.id}", [{r:.4f}, {g:.4f}, {b:.4f}]),')
