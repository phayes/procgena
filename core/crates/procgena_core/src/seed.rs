//! Seeds for children from their parent's seed, and the keys that name them.

use std::borrow::Cow;
use std::fmt;

use crate::hash::{derive_seed, salt, seed_for};

/// The most bytes a [`ChildKey`] may have.
pub const CHILD_KEY_MAX_LEN: usize = 32;

/// The name of one child within its parent: a Scatter in an Ecotope, an
/// Ecotope in a Biome, or an asset in a Scatter's mix.
///
/// A key is 1 to 32 bytes: a lower-case ASCII letter, then lower-case
/// letters, digits or `_` (for example `pines` or `pine_floor`). It has no
/// dots, so it can never be mistaken for a code seed key.
///
/// The key is the child's identity: its seed and the ids of its copies come
/// from it. Keys must be unique within their parent, and are frozen once
/// worlds are built from them. Renaming a key gives the child a new seed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChildKey(Cow<'static, str>);

impl ChildKey {
    /// Makes a key from a string known at compile time.
    ///
    /// # Panics
    ///
    /// Panics if `key` is not a well-formed key (see [`ChildKey`]). Used in
    /// a `const` item, a bad key fails the build.
    pub const fn from_static(key: &'static str) -> Self {
        if let Err(error) = check(key) {
            error.panic();
        }
        Self(Cow::Borrowed(key))
    }

    /// Makes a key from a string read at run time, such as from a preset
    /// file or a hook.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError`] if `key` is not a well-formed key (see
    /// [`ChildKey`]).
    pub fn new(key: impl Into<String>) -> Result<Self, KeyError> {
        let key = key.into();
        check(&key)?;
        Ok(Self(Cow::Owned(key)))
    }

    /// The key as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The key's salt: the same [`salt`] that code seed keys use.
    ///
    /// Two different keys can share a salt. A parent that holds several
    /// keys must reject such a pair, since the salt is what reaches seeds
    /// and copy ids.
    pub fn salt(&self) -> u32 {
        salt(&self.0)
    }
}

impl fmt::Display for ChildKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a string is not a well-formed [`ChildKey`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// The string is empty.
    Empty,
    /// The string is longer than [`CHILD_KEY_MAX_LEN`] bytes.
    TooLong,
    /// The first character is not a lower-case ASCII letter.
    BadStart,
    /// The byte at this index is not a lower-case ASCII letter, a digit or
    /// `_`.
    BadByte(usize),
}

impl KeyError {
    /// Panics with this error's message, in a form usable in `const fn`.
    const fn panic(self) -> ! {
        match self {
            Self::Empty => panic!("a child key must not be empty"),
            Self::TooLong => panic!("a child key must be at most 32 bytes"),
            Self::BadStart => panic!("a child key must start with a lower-case letter"),
            Self::BadByte(_) => {
                panic!("a child key may hold only lower-case letters, digits and `_`")
            }
        }
    }
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("a child key must not be empty"),
            Self::TooLong => write!(f, "a child key must be at most {CHILD_KEY_MAX_LEN} bytes"),
            Self::BadStart => f.write_str("a child key must start with a lower-case letter"),
            Self::BadByte(i) => write!(
                f,
                "a child key may hold only lower-case letters, digits and `_` (byte {i})"
            ),
        }
    }
}

impl std::error::Error for KeyError {}

/// Checks the form of a [`ChildKey`].
const fn check(key: &str) -> Result<(), KeyError> {
    let bytes = key.as_bytes();
    if bytes.is_empty() {
        return Err(KeyError::Empty);
    }
    if bytes.len() > CHILD_KEY_MAX_LEN {
        return Err(KeyError::TooLong);
    }
    if !bytes[0].is_ascii_lowercase() {
        return Err(KeyError::BadStart);
    }
    let mut i = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
            return Err(KeyError::BadByte(i));
        }
        i += 1;
    }
    Ok(())
}

/// The seed of the child named `key`, of kind `kind`, under the seed
/// `parent`.
///
/// `kind` is a code seed key naming what sort of child this is, such as
/// `"ecotope.child.scatter"`. It keeps a child keyed `hills` apart from a
/// code role such as `"terrain.relief.hills"` under the same parent, and
/// keeps children of different kinds with equal keys apart.
pub fn child_seed(parent: u32, kind: &str, key: &ChildKey) -> u32 {
    derive_seed(seed_for(parent, kind), key.salt())
}

// Seed keys owned by this crate, named `<crate>.<area>.<role>`. They are
// frozen: renaming one changes every world built from it. Add a new key for
// a new role, and list it in `KEYS`.

/// The root of every specimen's seed (see [`crate::SpecimenId::seed`]).
pub(crate) const SPECIMEN_SEED: &str = "core.specimen.seed";

/// Every seed key in this crate, for the test that they are all different.
#[cfg(test)]
pub(crate) const KEYS: &[&str] = &[SPECIMEN_SEED];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_keys_and_their_salts_are_all_different() {
        for (i, a) in KEYS.iter().enumerate() {
            assert!(a.starts_with("core."), "{a}");
            assert_eq!(a.split('.').count(), 3, "{a} isn't <crate>.<area>.<role>");
            for b in &KEYS[i + 1..] {
                assert_ne!(a, b);
                assert_ne!(salt(a), salt(b), "{a} and {b} share a salt");
            }
        }
    }

    #[test]
    fn well_formed_keys_are_accepted() {
        for key in [
            "a",
            "pines",
            "pine_floor",
            "z9_",
            "abcdefghijklmnopqrstuvwxyz012345",
        ] {
            let parsed = ChildKey::new(key).unwrap();
            assert_eq!(parsed.as_str(), key);
            assert_eq!(parsed, ChildKey::from_static(key));
        }
    }

    #[test]
    fn malformed_keys_are_rejected() {
        assert_eq!(ChildKey::new(""), Err(KeyError::Empty));
        assert_eq!(
            ChildKey::new("abcdefghijklmnopqrstuvwxyz0123456"),
            Err(KeyError::TooLong)
        );
        assert_eq!(ChildKey::new("Pines"), Err(KeyError::BadStart));
        assert_eq!(ChildKey::new("9pines"), Err(KeyError::BadStart));
        assert_eq!(ChildKey::new("_pines"), Err(KeyError::BadStart));
        assert_eq!(ChildKey::new("pine.floor"), Err(KeyError::BadByte(4)));
        assert_eq!(ChildKey::new("pineS"), Err(KeyError::BadByte(4)));
        assert_eq!(ChildKey::new("pine-floor"), Err(KeyError::BadByte(4)));
        assert_eq!(ChildKey::new("pinë"), Err(KeyError::BadByte(3)));
    }

    #[test]
    #[should_panic(expected = "lower-case letter")]
    fn from_static_panics_on_a_bad_key() {
        let _ = ChildKey::from_static("Pines");
    }

    #[test]
    fn from_static_works_in_const_items() {
        const PINES: ChildKey = ChildKey::from_static("pines");
        assert_eq!(PINES.as_str(), "pines");
    }

    #[test]
    fn a_key_salts_like_a_code_seed_key() {
        assert_eq!(ChildKey::from_static("hills").salt(), salt("hills"));
    }

    /// Reference values computed outside this crate, in Python, from the
    /// published FNV-1a and PCG3D definitions.
    #[test]
    fn child_seed_matches_reference_values() {
        let pines = ChildKey::from_static("pines");
        assert_eq!(child_seed(0, "ecotope.child.scatter", &pines), 0x2434_f59d);
        assert_eq!(child_seed(42, "ecotope.child.scatter", &pines), 0x0144_aff5);
        assert_eq!(child_seed(42, "biome.child.ecotope", &pines), 0x274f_965e);
    }

    #[test]
    fn child_seeds_separate_parents_kinds_and_keys() {
        let pines = ChildKey::from_static("pines");
        let firs = ChildKey::from_static("firs");
        let scatter = "ecotope.child.scatter";
        let ecotope = "biome.child.ecotope";

        // The same inputs give the same seed.
        assert_eq!(
            child_seed(7, scatter, &pines),
            child_seed(7, scatter, &pines)
        );
        // Each input changes the seed.
        assert_ne!(
            child_seed(7, scatter, &pines),
            child_seed(8, scatter, &pines)
        );
        assert_ne!(
            child_seed(7, scatter, &pines),
            child_seed(7, ecotope, &pines)
        );
        assert_ne!(
            child_seed(7, scatter, &pines),
            child_seed(7, scatter, &firs)
        );
        // A child is not seeded like its parent's own role of the same name.
        assert_ne!(child_seed(7, scatter, &pines), seed_for(7, scatter));
        assert_ne!(child_seed(7, scatter, &pines), seed_for(7, "pines"));
    }

    #[test]
    fn a_data_key_never_meets_a_code_key() {
        let hills = ChildKey::from_static("hills");
        let child = child_seed(7, "ecotope.child.scatter", &hills);
        assert_ne!(child, seed_for(7, "terrain.relief.hills"));
        assert_ne!(child, seed_for(7, "hills"));
    }
}
