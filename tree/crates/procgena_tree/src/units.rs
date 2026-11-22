//! Fixed reference lengths used by tree generation.
//!
//! Most lengths come from [`TreeOptions`](crate::TreeOptions). These few are
//! built into formulas instead, so they are defined here from one base length.

/// One ez-tree unit, in metres. ez-tree's original formulas and presets were
/// written in ez-tree units.
pub(crate) const TREE_UNIT: f32 = 0.1;

/// Branch radius at which random bending uses the configured gnarliness
/// unchanged. Thinner branches bend more, in proportion to
/// `1 / sqrt(radius / GNARL_REFERENCE_RADIUS)`.
pub(crate) const GNARL_REFERENCE_RADIUS: f32 = TREE_UNIT;

/// Radius of a branch tip at the deepest branch level.
pub(crate) const TIP_RADIUS: f32 = 0.001 * TREE_UNIT;

/// Branches closer than this to a trellis wire feel no pull from it.
pub(crate) const WIRE_MIN_DISTANCE: f32 = 0.001 * TREE_UNIT;

/// Leaf offset that tilts a rounded leaf normal by 45°. A rounded normal is
/// the leaf's face direction plus the corner's offset from the leaf origin
/// divided by this length, so larger leaves look rounder.
pub(crate) const LEAF_NORMAL_REFERENCE: f32 = TREE_UNIT;

#[cfg(test)]
impl crate::TreeOptions {
    /// Multiplies every length setting by `factor`, and divides every
    /// per-length setting (texture repeats and cluster density) by it.
    ///
    /// The result is not an exact scaled copy of the tree, because the
    /// reference lengths in this module stay fixed.
    pub(crate) fn scale_lengths(&mut self, factor: f32) {
        // Bark texture repeats per unit of branch radius.
        self.bark.texture_scale.x /= factor;

        // Branch lengths, trunk radius and the growth force. Radii of higher
        // levels are fractions of the parent radius, so they stay as they are.
        let branch = &mut self.branch;
        branch.length.0.iter_mut().for_each(|l| *l *= factor);
        branch.radius.0[0] *= factor;
        branch.force.strength *= factor;

        self.leaves.size *= factor;

        // Trellis layout and pull.
        let trellis = &mut self.trellis;
        trellis.position *= factor;
        trellis.width *= factor;
        trellis.height *= factor;
        trellis.spacing *= factor;
        trellis.cylinder_radius *= factor;
        trellis.force.strength *= factor;
        trellis.force.max_distance *= factor;

        // Fruit and flower clusters. Item shapes are relative to item size.
        for (placement, cluster) in [
            (&mut self.fruit.placement, &mut self.fruit.cluster),
            (&mut self.flowers.placement, &mut self.flowers.cluster),
        ] {
            placement.density /= factor;
            placement.size *= factor;
            cluster.axis_length *= factor;
            cluster.stalk_length *= factor;
            cluster.stalk_radius *= factor;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::TreeOptions;

    /// Every JSON key that `TreeOptions::scale_lengths` changes. Keep this in
    /// step with the options: a new length setting must be added here and
    /// to `scale_lengths`.
    const SCALED_KEYS: &[&str] = &[
        "bark.textureScale.x",
        "branch.force.strength",
        "branch.length.0",
        "branch.length.1",
        "branch.length.2",
        "branch.length.3",
        "branch.radius.0",
        "flowers.cluster.axisLength",
        "flowers.cluster.stalkLength",
        "flowers.cluster.stalkRadius",
        "flowers.placement.density",
        "flowers.placement.size",
        "fruit.cluster.axisLength",
        "fruit.cluster.stalkLength",
        "fruit.cluster.stalkRadius",
        "fruit.placement.density",
        "fruit.placement.size",
        "leaves.size",
        "trellis.cylinderRadius",
        "trellis.force.maxDistance",
        "trellis.force.strength",
        "trellis.height",
        "trellis.position.x",
        "trellis.position.y",
        "trellis.position.z",
        "trellis.spacing",
        "trellis.width",
    ];

    /// Collects the dotted path of every number in `value` that differs from `other`.
    fn changed_numbers(
        path: &str,
        value: &serde_json::Value,
        other: &serde_json::Value,
        out: &mut Vec<String>,
    ) {
        match (value, other) {
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                for (key, v) in a {
                    let child = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    changed_numbers(&child, v, &b[key], out);
                }
            }
            (a, b) if a != b => out.push(path.to_string()),
            _ => {}
        }
    }

    #[test]
    fn scale_lengths_changes_exactly_the_length_settings() {
        // Give every number a nonzero value so no scaled setting can hide at zero.
        let mut options = TreeOptions::default();
        let mut json = serde_json::to_value(&options).unwrap();
        fn fill(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(map) => map.values_mut().for_each(fill),
                serde_json::Value::Number(n) if n.is_f64() => *value = serde_json::json!(0.375),
                _ => {}
            }
        }
        fill(&mut json);
        options = serde_json::from_value(json).unwrap();

        let before = serde_json::to_value(&options).unwrap();
        options.scale_lengths(2.0);
        let after = serde_json::to_value(&options).unwrap();
        let mut changed = Vec::new();
        changed_numbers("", &after, &before, &mut changed);
        changed.sort();
        assert_eq!(changed, SCALED_KEYS);
    }

    #[test]
    fn scaling_back_restores_the_options() {
        let original = crate::TreePreset::ALL[0].options();
        let mut options = original.clone();
        options.scale_lengths(4.0);
        options.scale_lengths(0.25);
        assert_eq!(options, original);
    }
}
