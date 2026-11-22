# Texture licences

These textures come from [Poly Haven](https://polyhaven.com) and
[ambientCG](https://ambientcg.com). `tools/textures/fetch.sh` downloads each
set at 2048 × 2048 pixels, `tools/textures/convert.sh` converts them to KTX2, a
file format for compressed GPU textures, and `tools/textures/images.sh` writes
JPEG copies for exported models. The 1k and 512 versions are reduced copies.
The source licences below apply to these converted files.

KTX2 files are named `<material>/<id>-<size>-<map>-<format>.ktx2`, and JPEG
files `images/<material>/<id>-<size>-<map>.jpg`, where the material is the
start of the id (`snow`, `moss` or `leaves`). The tables below list each set
by its id.

All sets are released under
[CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/) (public
domain). Attribution is not required but is given as a courtesy.

## Snow

| Id         | Set      | Author(s)  | Source                            |
| ---------- | -------- | ---------- | --------------------------------- |
| `snow_02`  | Snow 02  | Rob Tuytel | https://polyhaven.com/a/snow_02   |
| `snow_005` | Snow 005 | ambientCG  | https://ambientcg.com/a/Snow005   |
| `snow_014` | Snow 014 | ambientCG  | https://ambientcg.com/a/Snow014   |

## Moss

| Id         | Set      | Author(s) | Source                          |
| ---------- | -------- | --------- | ------------------------------- |
| `moss_002` | Moss 002 | ambientCG | https://ambientcg.com/a/Moss002 |
| `moss_003` | Moss 003 | ambientCG | https://ambientcg.com/a/Moss003 |
| `moss_004` | Moss 004 | ambientCG | https://ambientcg.com/a/Moss004 |

## Leaf litter

| Id                     | Set                  | Author(s)                      | Source                                       |
| ---------------------- | -------------------- | ------------------------------ | -------------------------------------------- |
| `leaves_dry_decay`     | Dry Decay Leaves     | Amal Kumar                     | https://polyhaven.com/a/dry_decay_leaves     |
| `leaves_forest_ground` | Leaves Forest Ground | Dario Barresi, Dimitrios Savva | https://polyhaven.com/a/leaves_forest_ground |
| `leaves_forest_03`     | Forest Leaves 03     | Dimitrios Savva, Rob Tuytel    | https://polyhaven.com/a/forest_leaves_03     |

## Stalks

Stalk cards are transparent images of twigs and similar shapes, written as
`stalks/<id>-<map>-<format>.ktx2` and `images/stalks/<id>-<map>.png`.

| Id             | Set                                   | Author(s)         | Source                         |
| -------------- | ------------------------------------- | ----------------- | ------------------------------ |
| `twig_bare`    | Free 3D branches textures 04, #36     | Nobiax (yughues)  | https://nobiax.deviantart.com  |
| `twig_budding` | Free 3D branches textures 04, #37     | Nobiax (yughues)  | https://nobiax.deviantart.com  |

The Nobiax twigs are CC0. The sporophyte cards below are not from a CC0
library: they were cut out of AI-generated images made for this project, using
`tools/textures/key_card.py`. Their licence is set by the project owner and
has not been chosen yet.

| Id                  | Set                 | Author(s) | Source                         |
| ------------------- | ------------------- | --------- | ------------------------------ |
| `sporophyte_sparse` | Sporophytes, sparse | procgena_clump  | AI-generated, keyed by script  |
| `sporophyte_dense`  | Sporophytes, dense  | procgena_clump  | AI-generated, keyed by script  |
