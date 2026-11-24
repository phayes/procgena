# Presets

Each `<key>.tree.yml` file is one built-in `TreePreset`, embedded in the crate.
It gives only the settings that differ from `TreeOptions::default()`.

`apple`, `big_leaf_maple_medium`, `big_leaf_maple_large`, `flowering_dogwood`, `spring_pussy_willow`, `winter_thicket` and `cedar` are original to procgena_tree.
`oak_small`, `oak_medium` and `oak_large` started from ez-tree's oak presets and have since been reworked for procgena_tree.
The other tree presets come from
[ez-tree](https://github.com/dgreenheck/ez-tree), commit
`dcf309bd86bd521083d9c70f01f2de45fdc7c457` (`src/lib/presets/`).

All lengths are in metres. ez-tree's presets use a unit of about 10 cm, so
every length setting here is ez-tree's value × 0.1 (bark `texture_scale.x` and
cluster `density`, which are per length, are × 10). Apart from that, and
writing the settings in snake case, the ez-tree presets are unchanged.

ez-tree is Copyright (c) 2024 Daniel Greenheck and is released under the MIT licence.
The full licence text is in [`LICENSE-ez-tree`](LICENSE-ez-tree).
