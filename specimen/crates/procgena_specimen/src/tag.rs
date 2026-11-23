//! Labels that say what each part of a generated specimen is.

use std::borrow::Cow;
use std::fmt;

/// One label on a [`Part`](crate::Part), such as `bark` or `trunk`.
///
/// Tags are free text and compare exactly, including case. Use the constants
/// in [`tags`] for the labels procgena's generators use, and your own names
/// for anything else.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(Cow<'static, str>);

impl Tag {
    /// Creates a tag from a string literal. Usable in constants.
    pub const fn new(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// Creates a tag from a name built at runtime, such as one read from a file.
    pub fn owned(name: impl Into<String>) -> Self {
        Self(Cow::Owned(name.into()))
    }

    /// Returns the tag's name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The tags procgena's generators put on their parts.
///
/// A part usually has a tag for what it is made of ([`BARK`](tags::BARK)) and one for
/// what it is ([`TRUNK`](tags::TRUNK)).
pub mod tags {
    use super::Tag;

    /// Stone: every part of a procgena_stone rock.
    pub const ROCK: Tag = Tag::new("rock");
    /// Open ground, such as a flat ground patch.
    pub const GROUND: Tag = Tag::new("ground");
    /// Tree bark, on the trunk, limbs and branches.
    pub const BARK: Tag = Tag::new("bark");
    /// A tree's trunk.
    pub const TRUNK: Tag = Tag::new("trunk");
    /// A large branch growing from a tree's trunk.
    pub const LIMB: Tag = Tag::new("limb");
    /// A smaller branch growing from a limb or another branch.
    pub const BRANCH: Tag = Tag::new("branch");
    /// Leaves.
    pub const LEAF: Tag = Tag::new("leaf");
    /// Fruit, with their stalks.
    pub const FRUIT: Tag = Tag::new("fruit");
    /// Flowers, with their stalks.
    pub const FLOWER: Tag = Tag::new("flower");
    /// Flat cards with see-through cut-outs, such as leaves. They have no
    /// thickness, so nothing should grow on them as if they were solid.
    pub const CARD: Tag = Tag::new("card");
    /// Anything grown by procgena_clump on top of another surface.
    pub const CLUMP: Tag = Tag::new("clump");
    /// Snow.
    pub const SNOW: Tag = Tag::new("snow");
    /// Moss.
    pub const MOSS: Tag = Tag::new("moss");
    /// Mud.
    pub const MUD: Tag = Tag::new("mud");
    /// Fallen leaves lying in a pile or layer.
    pub const LEAVES: Tag = Tag::new("leaves");
    /// A woody swelling on a trunk, limb or rock.
    pub const BURL: Tag = Tag::new("burl");
    /// Dead wood lying on or standing in the ground, such as a fallen log,
    /// a fallen branch, a stick or a stump.
    pub const DEADWOOD: Tag = Tag::new("deadwood");
    /// Bare wood with no bark covering it, such as a sawn end, a broken end
    /// or a patch where the bark has peeled away. A part that shows both
    /// bare wood and bark carries this tag and [`BARK`](self::BARK)
    /// together.
    pub const WOOD: Tag = Tag::new("wood");
}

/// The tags of one part, kept sorted and without repeats.
///
/// Two sets with the same tags are equal whatever order the tags were added in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TagSet(Vec<Tag>);

impl TagSet {
    /// Creates an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `tag`, unless the set already has it.
    pub fn insert(&mut self, tag: Tag) {
        if let Err(at) = self.0.binary_search(&tag) {
            self.0.insert(at, tag);
        }
    }

    /// Returns true if the set has `tag`.
    pub fn contains(&self, tag: &Tag) -> bool {
        self.0.binary_search(tag).is_ok()
    }

    /// Returns the tags in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &Tag> {
        self.0.iter()
    }

    /// Returns the number of tags.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the set has no tags.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<Tag> for TagSet {
    fn from_iter<I: IntoIterator<Item = Tag>>(iter: I) -> Self {
        let mut tags: Vec<Tag> = iter.into_iter().collect();
        tags.sort_unstable();
        tags.dedup();
        Self(tags)
    }
}

impl Extend<Tag> for TagSet {
    fn extend<I: IntoIterator<Item = Tag>>(&mut self, iter: I) {
        for tag in iter {
            self.insert(tag);
        }
    }
}

impl<const N: usize> From<[Tag; N]> for TagSet {
    fn from(tags: [Tag; N]) -> Self {
        tags.into_iter().collect()
    }
}

/// Picks parts by their tags.
///
/// A part matches when it has at least one of the [`any_of`](Self::any_of)
/// tags and none of the [`none_of`](Self::none_of) tags. An empty `any_of`
/// matches every part not ruled out by `none_of`, so the default filter
/// matches everything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagFilter {
    /// A part must have at least one of these tags. Empty means no requirement.
    pub any_of: Vec<Tag>,
    /// A part must have none of these tags.
    pub none_of: Vec<Tag>,
}

impl TagFilter {
    /// Creates a filter that matches parts with at least one of `tags`.
    pub fn any_of(tags: impl IntoIterator<Item = Tag>) -> Self {
        Self {
            any_of: tags.into_iter().collect(),
            none_of: Vec::new(),
        }
    }

    /// Returns this filter, also ruling out parts with any of `tags`.
    pub fn excluding(mut self, tags: impl IntoIterator<Item = Tag>) -> Self {
        self.none_of.extend(tags);
        self
    }

    /// Returns true if a part with `tags` passes the filter.
    pub fn matches(&self, tags: &TagSet) -> bool {
        let wanted = self.any_of.is_empty() || self.any_of.iter().any(|t| tags.contains(t));
        wanted && !self.none_of.iter().any(|t| tags.contains(t))
    }
}

#[cfg(test)]
mod tests {
    use super::tags::*;
    use super::*;

    #[test]
    fn sets_are_sorted_and_ignore_repeats() {
        let a = TagSet::from([TRUNK, BARK, TRUNK]);
        let b: TagSet = [BARK, TRUNK].into_iter().collect();
        assert_eq!(a, b);
        assert_eq!(a.len(), 2);
        assert_eq!(
            a.iter().map(Tag::as_str).collect::<Vec<_>>(),
            ["bark", "trunk"]
        );

        let mut c = TagSet::new();
        c.insert(TRUNK);
        c.extend([BARK, BARK]);
        assert_eq!(a, c);
    }

    #[test]
    fn owned_and_constant_tags_are_equal() {
        assert_eq!(Tag::owned("bark"), BARK);
        assert!(TagSet::from([Tag::owned("rock")]).contains(&ROCK));
        assert_ne!(Tag::owned("Bark"), BARK);
    }

    #[test]
    fn filters_need_one_wanted_tag_and_no_unwanted_tag() {
        let trunk = TagSet::from([BARK, TRUNK]);
        let leaf = TagSet::from([LEAF, CARD]);
        let untagged = TagSet::new();

        // The default filter matches everything.
        let all = TagFilter::default();
        assert!(all.matches(&trunk) && all.matches(&leaf) && all.matches(&untagged));

        // Any one wanted tag is enough.
        let wood = TagFilter::any_of([ROCK, BARK]);
        assert!(wood.matches(&trunk));
        assert!(!wood.matches(&leaf));
        assert!(!wood.matches(&untagged));

        // An unwanted tag rules a part out, even with a wanted one.
        let solid = TagFilter::default().excluding([CARD]);
        assert!(solid.matches(&trunk));
        assert!(!solid.matches(&leaf));
        let no_trunk = TagFilter::any_of([BARK]).excluding([TRUNK]);
        assert!(!no_trunk.matches(&trunk));
    }
}
