//! A stable content hash for options types.
//!
//! A type implements [`ContentHash`] by writing each of its fields into a
//! [`ContentHasher`] in a fixed order. The result is the same in every run,
//! on every platform and with every Rust version, so it can name cached or
//! saved results. It does not depend on field names, serialisation formats or
//! `std::hash::Hash`.

use glam::{IVec2, IVec3, Quat, UVec2, UVec3, Vec2, Vec3, Vec4};

use crate::hash::mix64;

/// The bits every `f32` NaN is written as: a positive quiet NaN with no
/// payload. Fixed here, since `f32::NAN`'s bits are not guaranteed.
const CANONICAL_NAN_F32: u32 = 0x7fc0_0000;
/// The bits every `f64` NaN is written as: a positive quiet NaN with no
/// payload. Fixed here, since `f64::NAN`'s bits are not guaranteed.
const CANONICAL_NAN_F64: u64 = 0x7ff8_0000_0000_0000;

/// Builds a 64-bit content hash from a sequence of values.
///
/// Each write adds one or more 64-bit words, in order:
/// - Integers and `bool` are widened to one word. Signed integers are
///   sign-extended. A `u32` and a `u64` of equal value write the same word.
/// - Floats are written by their bits, except that `-0.0` writes as `0.0`
///   and every NaN writes as one canonical NaN, so values that compare equal
///   hash equal.
/// - Byte strings and `str` write their length first, so that `("ab", "c")`
///   and `("a", "bc")` differ.
///
/// The hasher has no type tags: the reader of a hash must know which fields
/// were written, in which order.
#[derive(Clone, Debug)]
pub struct ContentHasher {
    state: u64,
}

impl Default for ContentHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentHasher {
    /// Starts an empty hash.
    pub fn new() -> Self {
        Self {
            state: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// The hash of everything written so far. Writing more afterwards is
    /// allowed and continues the same hash.
    pub fn finish(&self) -> u64 {
        self.state
    }

    /// Writes any value that implements [`ContentHash`].
    pub fn write<T: ContentHash + ?Sized>(&mut self, value: &T) {
        value.content_hash(self);
    }

    /// Writes one 64-bit word.
    pub fn write_u64(&mut self, value: u64) {
        self.state = mix64(self.state, value);
    }

    /// Writes a `u32` as one word.
    pub fn write_u32(&mut self, value: u32) {
        self.write_u64(u64::from(value));
    }

    /// Writes a `usize` as one 64-bit word, the same on every platform.
    pub fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }

    /// Writes an `i64` as one word.
    pub fn write_i64(&mut self, value: i64) {
        self.write_u64(value as u64);
    }

    /// Writes an `i32` as one word, sign-extended.
    pub fn write_i32(&mut self, value: i32) {
        self.write_i64(i64::from(value));
    }

    /// Writes `false` as 0 and `true` as 1.
    pub fn write_bool(&mut self, value: bool) {
        self.write_u64(u64::from(value));
    }

    /// Writes an `f32` by its bits, with `-0.0` as `0.0` and every NaN as
    /// one canonical NaN.
    pub fn write_f32(&mut self, value: f32) {
        let bits = if value == 0.0 {
            0
        } else if value.is_nan() {
            CANONICAL_NAN_F32
        } else {
            value.to_bits()
        };
        self.write_u32(bits);
    }

    /// Writes an `f64` by its bits, with `-0.0` as `0.0` and every NaN as
    /// one canonical NaN.
    pub fn write_f64(&mut self, value: f64) {
        let bits = if value == 0.0 {
            0
        } else if value.is_nan() {
            CANONICAL_NAN_F64
        } else {
            value.to_bits()
        };
        self.write_u64(bits);
    }

    /// Writes the length of `bytes`, then the bytes, eight to a word in
    /// little-endian order, with the last word padded with zeros.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_usize(bytes.len());
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }

    /// Writes a string's UTF-8 bytes, as [`write_bytes`](Self::write_bytes)
    /// does.
    pub fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }
}

/// A value with a stable content hash: one that is the same in every run, on
/// every platform and with every Rust version.
///
/// Implement it for an options type by writing every field that affects the
/// result, in a fixed order. Destructure `self` with `let Self { .. } = self`
/// so that adding a field fails to compile until the field is hashed. For an
/// enum, write a variant number you choose, then the variant's fields. Never
/// use `std::mem::discriminant` or a derived `Hash`, whose encodings may
/// change.
///
/// An options type's hash names a pool of specimens, so it must leave out
/// the seed and the detail level. Changing the order of writes, or a variant
/// number, changes every hash.
///
/// ```
/// use procgena_core::{ContentHash, ContentHasher, OptionsHash};
///
/// struct StoneOptions {
///     radius: f32,
///     facets: u32,
///     seed: u32,
/// }
///
/// impl ContentHash for StoneOptions {
///     fn content_hash(&self, hasher: &mut ContentHasher) {
///         // The seed is not part of the options hash.
///         let Self { radius, facets, seed: _ } = self;
///         hasher.write_f32(*radius);
///         hasher.write_u32(*facets);
///     }
/// }
///
/// let a = StoneOptions { radius: 1.0, facets: 12, seed: 1 };
/// let b = StoneOptions { radius: 1.0, facets: 12, seed: 2 };
/// assert_eq!(OptionsHash::of(&a), OptionsHash::of(&b));
/// ```
pub trait ContentHash {
    /// Writes this value into `hasher`.
    fn content_hash(&self, hasher: &mut ContentHasher);
}

/// The content hash of `value` alone, from a new [`ContentHasher`].
pub fn content_hash<T: ContentHash + ?Sized>(value: &T) -> u64 {
    let mut hasher = ContentHasher::new();
    value.content_hash(&mut hasher);
    hasher.finish()
}

/// The content hash of one asset's options: no seed and no detail level.
///
/// Equal options give equal hashes. Different options give different hashes
/// except by rare chance, so an equal hash is not proof of equal options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OptionsHash(pub u64);

impl OptionsHash {
    /// The options hash of `options`.
    pub fn of<T: ContentHash + ?Sized>(options: &T) -> Self {
        Self(content_hash(options))
    }
}

impl ContentHash for OptionsHash {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u64(self.0);
    }
}

// Primitives.

impl ContentHash for u8 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u64(u64::from(*self));
    }
}

impl ContentHash for u16 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u64(u64::from(*self));
    }
}

impl ContentHash for u32 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u32(*self);
    }
}

impl ContentHash for u64 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u64(*self);
    }
}

impl ContentHash for usize {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_usize(*self);
    }
}

impl ContentHash for i8 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_i64(i64::from(*self));
    }
}

impl ContentHash for i16 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_i64(i64::from(*self));
    }
}

impl ContentHash for i32 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_i32(*self);
    }
}

impl ContentHash for i64 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_i64(*self);
    }
}

impl ContentHash for bool {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_bool(*self);
    }
}

impl ContentHash for f32 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_f32(*self);
    }
}

impl ContentHash for f64 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_f64(*self);
    }
}

impl ContentHash for str {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_str(self);
    }
}

impl ContentHash for String {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_str(self);
    }
}

// Containers.

impl<T: ContentHash + ?Sized> ContentHash for &T {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        (**self).content_hash(hasher);
    }
}

impl<T: ContentHash + ?Sized> ContentHash for Box<T> {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        (**self).content_hash(hasher);
    }
}

/// Writes the length, then each element.
impl<T: ContentHash> ContentHash for [T] {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_usize(self.len());
        for item in self {
            item.content_hash(hasher);
        }
    }
}

/// Writes the length, then each element, as a slice does.
impl<T: ContentHash, const N: usize> ContentHash for [T; N] {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        self.as_slice().content_hash(hasher);
    }
}

/// Writes the length, then each element, as a slice does.
impl<T: ContentHash> ContentHash for Vec<T> {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        self.as_slice().content_hash(hasher);
    }
}

/// Writes 0 for `None`, or 1 and then the value for `Some`.
impl<T: ContentHash> ContentHash for Option<T> {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        match self {
            None => hasher.write_u64(0),
            Some(value) => {
                hasher.write_u64(1);
                value.content_hash(hasher);
            }
        }
    }
}

/// Writes the start, then the end.
impl<T: ContentHash> ContentHash for std::ops::Range<T> {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        self.start.content_hash(hasher);
        self.end.content_hash(hasher);
    }
}

impl<A: ContentHash, B: ContentHash> ContentHash for (A, B) {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        self.0.content_hash(hasher);
        self.1.content_hash(hasher);
    }
}

impl<A: ContentHash, B: ContentHash, C: ContentHash> ContentHash for (A, B, C) {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        self.0.content_hash(hasher);
        self.1.content_hash(hasher);
        self.2.content_hash(hasher);
    }
}

// glam types: each writes its components in order, with no length.

/// Writes `x`, then `y`.
impl ContentHash for Vec2 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_f32(self.x);
        hasher.write_f32(self.y);
    }
}

/// Writes `x`, `y`, then `z`.
impl ContentHash for Vec3 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_f32(self.x);
        hasher.write_f32(self.y);
        hasher.write_f32(self.z);
    }
}

/// Writes `x`, `y`, `z`, then `w`.
impl ContentHash for Vec4 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_f32(self.x);
        hasher.write_f32(self.y);
        hasher.write_f32(self.z);
        hasher.write_f32(self.w);
    }
}

/// Writes `x`, `y`, `z`, then `w`, as stored. A quaternion and its negation
/// turn things the same way but hash differently.
impl ContentHash for Quat {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_f32(self.x);
        hasher.write_f32(self.y);
        hasher.write_f32(self.z);
        hasher.write_f32(self.w);
    }
}

/// Writes `x`, then `y`.
impl ContentHash for IVec2 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_i32(self.x);
        hasher.write_i32(self.y);
    }
}

/// Writes `x`, `y`, then `z`.
impl ContentHash for IVec3 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_i32(self.x);
        hasher.write_i32(self.y);
        hasher.write_i32(self.z);
    }
}

/// Writes `x`, then `y`.
impl ContentHash for UVec2 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u32(self.x);
        hasher.write_u32(self.y);
    }
}

/// Writes `x`, `y`, then `z`.
impl ContentHash for UVec3 {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u32(self.x);
        hasher.write_u32(self.y);
        hasher.write_u32(self.z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in options type, hashed the way the docs ask.
    struct Sample {
        count: u32,
        radius: f32,
        name: String,
        tint: Option<Vec3>,
        weights: Vec<f32>,
        smooth: bool,
    }

    impl ContentHash for Sample {
        fn content_hash(&self, hasher: &mut ContentHasher) {
            let Self {
                count,
                radius,
                name,
                tint,
                weights,
                smooth,
            } = self;
            hasher.write_u32(*count);
            hasher.write_f32(*radius);
            hasher.write_str(name);
            hasher.write(tint);
            hasher.write(weights);
            hasher.write_bool(*smooth);
        }
    }

    fn sample() -> Sample {
        Sample {
            count: 7,
            radius: 1.5,
            name: "pine".to_owned(),
            tint: None,
            weights: vec![0.25, -2.0],
            smooth: true,
        }
    }

    /// Reference values computed outside this crate, in Python, from the
    /// encoding described on `ContentHasher`. A failure means hashes changed:
    /// every saved or cached hash would then be stale.
    #[test]
    fn hashes_match_reference_values() {
        assert_eq!(ContentHasher::new().finish(), 0xcbf2_9ce4_8422_2325);
        assert_eq!(content_hash(&0u64), 0x3202_04e2_84a7_5848);
        assert_eq!(content_hash(&1.5f32), 0xc0b8_0bc6_d5bd_d47a);
        assert_eq!(content_hash("pine"), 0xe53a_ac0c_8618_d091);
        assert_eq!(content_hash(&sample()), 0x2756_18ee_aca3_ac6f);
        assert_eq!(
            content_hash(&f32::from_bits(0xffc0_0001)),
            0x1c5e_8d98_b1c5_a002
        );
        assert_eq!(
            content_hash(&f64::from_bits(0xfff8_0000_0000_0001)),
            0xf32a_3ac0_fbd9_7b98
        );
        assert_eq!(content_hash(&-2.5f64), 0xe6c3_f8c2_64af_5c9a);
        assert_eq!(content_hash("pine_forest_floor"), 0x04ae_dbd5_ba99_4bd3);
        assert_eq!(content_hash(&-3i32), 0x06db_9e5e_1001_2ea9);
        assert_eq!(content_hash(&Some(9u32)), 0x4e2a_deb5_253a_358d);
    }

    #[test]
    fn each_field_changes_the_hash() {
        let base = content_hash(&sample());
        let changes: [fn(&mut Sample); 7] = [
            |s| s.count += 1,
            |s| s.radius = 1.5000001,
            |s| s.name.push('s'),
            |s| s.tint = Some(Vec3::ZERO),
            |s| s.weights[1] = 2.0,
            |s| s.weights.push(0.0),
            |s| s.smooth = false,
        ];
        let mut seen = vec![base];
        for change in changes {
            let mut s = sample();
            change(&mut s);
            let hash = content_hash(&s);
            assert!(!seen.contains(&hash), "a change left the hash unchanged");
            seen.push(hash);
        }
    }

    #[test]
    fn signed_zero_and_nan_are_normalised() {
        assert_eq!(content_hash(&-0.0f32), content_hash(&0.0f32));
        assert_eq!(content_hash(&-0.0f64), content_hash(&0.0f64));
        let other_nan = f32::from_bits(0xffc0_0001);
        assert!(other_nan.is_nan());
        assert_eq!(content_hash(&other_nan), content_hash(&f32::NAN));
        let other_nan = f64::from_bits(0xfff8_0000_0000_0001);
        assert!(other_nan.is_nan());
        assert_eq!(content_hash(&other_nan), content_hash(&f64::NAN));
        assert_ne!(content_hash(&f32::NAN), content_hash(&f32::INFINITY));
    }

    #[test]
    fn lengths_keep_neighbouring_strings_apart() {
        assert_ne!(content_hash(&("ab", "c")), content_hash(&("a", "bc")));
        assert_ne!(content_hash(""), content_hash("\0"));
        assert_ne!(content_hash("abcdefgh"), content_hash("abcdefgh\0"));
        assert_ne!(
            content_hash(&(vec![1u32, 2], vec![3u32])),
            content_hash(&(vec![1u32], vec![2u32, 3]))
        );
    }

    #[test]
    fn option_none_differs_from_some_zero() {
        assert_ne!(content_hash(&None::<u32>), content_hash(&Some(0u32)));
    }

    #[test]
    fn order_matters() {
        assert_ne!(content_hash(&(1u32, 2u32)), content_hash(&(2u32, 1u32)));
        assert_ne!(
            content_hash(&Vec2::new(1.0, 2.0)),
            content_hash(&Vec2::new(2.0, 1.0))
        );
    }

    #[test]
    fn containers_match_their_slices() {
        let array = [1.0f32, 2.0];
        assert_eq!(content_hash(&array), content_hash(array.as_slice()));
        assert_eq!(
            content_hash(&array.to_vec()),
            content_hash(array.as_slice())
        );
        assert_eq!(content_hash(&Box::new(3u32)), content_hash(&3u32));
    }

    #[test]
    fn widths_and_signs_are_fixed() {
        // Integers widen to one word, so equal values agree across widths.
        assert_eq!(content_hash(&5u8), content_hash(&5u64));
        assert_eq!(content_hash(&5usize), content_hash(&5u64));
        // Signed integers sign-extend.
        assert_eq!(content_hash(&-1i32), content_hash(&u64::MAX));
        assert_ne!(content_hash(&-1i32), content_hash(&u64::from(u32::MAX)));
    }
}
