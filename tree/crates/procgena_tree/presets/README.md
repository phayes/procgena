# Presets

`apple.json`, `big_leaf_maple_medium.json`, `big_leaf_maple_large.json`, `flowering_dogwood.json`, `spring_pussy_willow.json`, `winter_thicket.json` and `cedar.json` are original to procgena_tree.
`oak_small.json`, `oak_medium.json` and `oak_large.json` started from ez-tree's oak presets and have since been reworked for procgena_tree.
The other tree presets come from
[ez-tree](https://github.com/dgreenheck/ez-tree), commit
`dcf309bd86bd521083d9c70f01f2de45fdc7c457` (`src/lib/presets/`).

All lengths are in metres. ez-tree's presets use a unit of about 10 cm, so
every length setting here is ez-tree's value × 0.1 (bark `textureScale.x` and
cluster `density`, which are per length, are × 10). Apart from that, the
ez-tree presets are unchanged.

ez-tree is Copyright (c) 2024 Daniel Greenheck and is released under the MIT licence.
The full licence text is in [`LICENSE-ez-tree`](LICENSE-ez-tree).
