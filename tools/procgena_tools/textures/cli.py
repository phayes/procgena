"""Command line for the texture tool: `textures <command> <project> [options]`."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from . import config
from .util import ToolError

DESCRIPTION = """\
Download, prepare and publish the textures of a procgena project. Each project
lists its textures in <project>/textures.yml. Most people only need `download`.
"""


def main(argv: list[str] | None = None) -> None:
    """Run the command named on the command line; exits non-zero on failure."""
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        args.run(args)
    except (ToolError, config.ConfigError) as error:
        sys.exit(f"error: {error}")


def build_parser() -> argparse.ArgumentParser:
    """The argument parser with every subcommand."""
    parser = argparse.ArgumentParser(prog="textures", description=DESCRIPTION)
    commands = parser.add_subparsers(required=True, metavar="COMMAND")

    # download: finished textures from the public bucket.
    p = project_command(
        commands, "download",
        "download ready-made KTX2 textures (or, with --images, export images) listed in the project's manifest",
    )
    p.add_argument("--images", action="store_true", help="download the JPEG/PNG export images instead")
    p.add_argument("--sizes", default="512,1k", help="comma-separated surface sizes: 128, 256, 512, 1k, 2k, as the project lists them (default 512,1k)")
    p.add_argument("--formats", default="bc7,uastc", help="comma-separated formats: bc7, uastc (default both)")
    p.add_argument("--dest", type=Path, help="folder to save into (default <project>/assets/textures)")
    p.set_defaults(run=run_download)

    # fetch: 2k source images from the texture sites.
    p = project_command(commands, "fetch", "download each surface set's 2k source images from its website")
    p.add_argument("ids", nargs="*", help="set ids to fetch (default all)")
    add_source_options(p)
    p.set_defaults(run=run_fetch)

    # convert: KTX2 textures from the source images.
    p = project_command(commands, "convert", "convert source images to KTX2 textures (BC7 and UASTC)")
    p.add_argument("ids", nargs="*", help="set ids or card kinds to convert (default all)")
    p.add_argument("--only", choices=("surfaces", "cards", "clumps"), help="convert only this part")
    add_source_options(p)
    add_assets_option(p)
    p.set_defaults(run=run_convert)

    # manifest: list the KTX2 files for download.
    p = project_command(commands, "manifest", "write assets_manifest.json from the converted KTX2 files")
    add_source_options(p)
    add_assets_option(p)
    p.set_defaults(run=run_manifest)

    # images: export images and their manifest.
    p = project_command(commands, "images", "write JPEG/PNG images for exported models, and their manifest")
    add_source_options(p)
    add_assets_option(p)
    p.set_defaults(run=run_images)

    # average-colors: Rust table rows.
    p = project_command(commands, "average-colors", "print each surface set's average linear colour as Rust table rows")
    add_source_options(p)
    p.set_defaults(run=run_average_colors)

    # key-card: cut a stalk card out of a photo.
    p = commands.add_parser("key-card", help="cut a reddish stalk card out of a photo on a light background")
    p.add_argument("input", help="photo to key, any format Pillow reads")
    p.add_argument("prefix", help="output path without .png; writes <prefix>.png and <prefix>-normal.png")
    p.add_argument("--lo", type=float, default=0.16, help="red minus green (0-1) at or below which pixels are clear")
    p.add_argument("--hi", type=float, default=0.31, help="red minus green (0-1) at or above which pixels are solid")
    p.add_argument("--crop-bottom", type=int, default=0, help="rows to remove from the bottom of the input before keying")
    p.add_argument("--min-area", type=int, default=400, help="drop solid areas smaller than this many input pixels")
    p.add_argument("--margin", type=float, default=0.02, help="space above and beside the stalks, as a fraction of the card")
    p.add_argument("--size", type=int, default=1024, help="output width and height")
    p.set_defaults(run=run_key_card)
    return parser


def project_command(commands, name: str, help_text: str) -> argparse.ArgumentParser:
    """Add a subcommand that takes a project name."""
    p = commands.add_parser(name, help=help_text, description=help_text)
    p.add_argument("project", help=f"project folder: {', '.join(config.project_names())}")
    return p


def add_source_options(p: argparse.ArgumentParser) -> None:
    """Options that move the git-ignored source folders."""
    p.add_argument(
        "--prior-art", type=Path,
        help="folder with hand-supplied card sources (default <project>/prior_art)",
    )
    p.add_argument(
        "--sources", type=Path,
        help="folder with each set's 2k images and info.json (default <prior-art>/textures)",
    )


def add_assets_option(p: argparse.ArgumentParser) -> None:
    """Option that moves the output folder."""
    p.add_argument("--assets", type=Path, help="output folder (default <project>/assets/textures)")


def load_project(args: argparse.Namespace) -> tuple[config.Project, Path]:
    """The project named in `args` with any --prior-art override, and its sources folder."""
    project = config.load(args.project, prior_art=args.prior_art)
    return project, args.sources or project.sources


def run_download(args: argparse.Namespace) -> None:
    from .download import download

    project = config.load(args.project)
    dest = args.dest or project.assets
    if args.images:
        download(project.assets / "images" / "images_manifest.json", dest / "images", split(args.sizes), None)
    else:
        download(project.assets / "assets_manifest.json", dest, split(args.sizes), split(args.formats))


def run_fetch(args: argparse.Namespace) -> None:
    from .fetch import fetch

    project, sources = load_project(args)
    fetch(project, sources, args.ids)


def run_convert(args: argparse.Namespace) -> None:
    from .convert import convert

    project, sources = load_project(args)
    convert(project, sources, args.assets or project.assets, args.only, args.ids)


def run_manifest(args: argparse.Namespace) -> None:
    from .manifest import write_manifest

    project, sources = load_project(args)
    write_manifest(project, sources, args.assets or project.assets)


def run_images(args: argparse.Namespace) -> None:
    from .manifest import write_images

    project, sources = load_project(args)
    write_images(project, sources, args.assets or project.assets)


def run_average_colors(args: argparse.Namespace) -> None:
    from .colors import average_colors

    project, sources = load_project(args)
    average_colors(project, sources)


def run_key_card(args: argparse.Namespace) -> None:
    from .key_card import key_card

    key_card(args)


def split(value: str) -> list[str]:
    """Split a comma-separated option into its non-empty parts."""
    return [part for part in value.split(",") if part]
