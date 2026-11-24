//! The options hash: the same in every run, changed by every setting that
//! shapes the piece, and unchanged by the seed and the mesh detail.

use procgena_core::{OptionsHash, content_hash};
use procgena_deadwood::{DeadwoodDetail, DeadwoodOptions, DeadwoodPreset, EndKind, PieceKind};

/// Hashes of the default options and of every preset, in
/// [`DeadwoodPreset::ALL`] order. They must never change by accident:
/// a changed hash regrows every specimen built from those options.
const GOLDEN: &[(&str, u64)] = &[
    ("default", 0x95598bc00475c557),
    ("Fresh cut log", 0x44be1db389bf6b69),
    ("Windthrow log", 0xdaa8d01e5593ba15),
    ("Mossy log", 0x4bb8adcd88e8a071),
    ("Hollow log", 0x58a34025cc749718),
    ("Rotten log", 0x5610a5452f6e4dca),
    ("Birch log", 0x1a01595627f97c6b),
    ("Fallen limb", 0xb4066ea4e668bf72),
    ("Dead branch", 0x9e2c4a17cd4b9a10),
    ("Stick", 0x67a48b3503eaafe6),
    ("Twig", 0x3fa83863f6a1e359),
    ("Felled stump", 0xfaa9265e38fd86e3),
    ("Notched stump", 0xbb094a73d75b6d42),
    ("Snapped stump", 0x5634ac90cf328533),
    ("Rotten stump", 0x84756754578576e8),
    ("Sprouting stump", 0x8126a9558b8702e6),
    ("Seedling log", 0x79b3715563ea8500),
];

#[test]
fn hashes_match_reference_values() {
    let mut actual = vec![("default", content_hash(&DeadwoodOptions::default()))];
    actual.extend(
        DeadwoodPreset::ALL
            .iter()
            .map(|p| (p.name(), content_hash(&p.options()))),
    );
    assert_eq!(actual, GOLDEN);
}

#[test]
fn seed_and_detail_leave_the_hash_alone() {
    let o = DeadwoodOptions::default();
    let mut changed = o.clone();
    changed.seed = o.seed.wrapping_add(1);
    changed.detail = DeadwoodDetail::STRIP;
    assert_eq!(OptionsHash::of(&o), OptionsHash::of(&changed));
}

#[test]
fn every_part_of_the_options_changes_the_hash() {
    let base = DeadwoodOptions::default();
    type Edit = fn(&mut DeadwoodOptions);
    let edits: Vec<(&str, Edit)> = vec![
        ("kind", |o| o.kind = PieceKind::Stump),
        ("decay", |o| o.decay += 0.5),
        ("length", |o| o.length += 0.1),
        ("diameter", |o| o.diameter += 0.01),
        ("taper", |o| o.taper *= 0.9),
        ("shape", |o| o.shape.ovality = Some(0.2)),
        ("ends", |o| o.ends.tip = EndKind::Taper),
        ("stubs", |o| o.stubs.keep = Some(0.5)),
        ("branching", |o| o.branching.twigs += 1),
        ("bark", |o| o.bark.cracks += 0.1),
        ("hollow", |o| o.hollow.enabled = !o.hollow.enabled),
        ("roots", |o| o.roots.count += 1),
        ("leaves", |o| o.leaves.tint.push('0')),
        ("material", |o| o.material.end_texture = Some("x".into())),
        ("settle", |o| o.settle.settle = !o.settle.settle),
    ];
    let mut seen = vec![content_hash(&base)];
    for (name, edit) in edits {
        let mut o = base.clone();
        edit(&mut o);
        let hash = content_hash(&o);
        assert!(!seen.contains(&hash), "{name} did not change the hash");
        seen.push(hash);
    }
}

#[test]
fn neighbouring_end_kinds_hash_apart() {
    let kinds = [
        EndKind::Saw { tilt: 0.0 },
        EndKind::Chainsaw {
            tilt: 0.0,
            step: 0.0,
        },
        EndKind::Axe { angle: 0.0 },
        EndKind::Snapped {
            jaggedness: 0.0,
            splinters: 0,
            one_sided: 0.0,
        },
        EndKind::Taper,
    ];
    let hashes: Vec<u64> = kinds.iter().map(content_hash).collect();
    for (i, a) in hashes.iter().enumerate() {
        assert!(!hashes[i + 1..].contains(a), "{:?} collides", kinds[i]);
    }
}

#[test]
fn detail_levels_hash_apart() {
    let hashes: Vec<u64> = DeadwoodDetail::PRESETS
        .iter()
        .map(|(_, d)| content_hash(d))
        .collect();
    for (i, a) in hashes.iter().enumerate() {
        assert!(!hashes[i + 1..].contains(a));
    }
}
