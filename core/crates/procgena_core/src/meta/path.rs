//! Paths to settings in a settings tree, and patterns over them.

use std::fmt;

use serde_json::Value;

/// A dotted path to one setting in a settings tree, such as
/// `branch.length.0` or `scatters.pines.density_scale`.
///
/// Each segment is a map key or, inside a list, the item's index. The empty
/// path names the whole tree. Paths are the same ones that preset files,
/// [`preset::diff`](crate::preset::diff) and the metadata files use.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SettingPath(Vec<String>);

impl SettingPath {
    /// Reads a dotted path. An empty string is the path to the whole tree.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        if text.is_empty() {
            return Self::default();
        }
        Self(text.split('.').map(str::to_owned).collect())
    }

    /// The path's segments, outermost first.
    #[must_use]
    pub fn segments(&self) -> &[String] {
        &self.0
    }

    /// This path with `segment` added at the end.
    #[must_use]
    pub fn child(&self, segment: impl Into<String>) -> Self {
        let mut segments = self.0.clone();
        segments.push(segment.into());
        Self(segments)
    }

    /// Whether `prefix` is this path or one of its parents.
    #[must_use]
    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.0.starts_with(&prefix.0)
    }

    /// The setting at this path in `value`, or `None` if there is none.
    #[must_use]
    pub fn get<'a>(&self, value: &'a Value) -> Option<&'a Value> {
        self.0
            .iter()
            .try_fold(value, |value, segment| step(value, segment))
    }

    /// The setting at this path in `value`, to change it, or `None` if
    /// there is none.
    pub fn get_mut<'a>(&self, value: &'a mut Value) -> Option<&'a mut Value> {
        self.0
            .iter()
            .try_fold(value, |value, segment| step_mut(value, segment))
    }

    /// Puts `new` at this path in `value` and returns the value it replaced.
    ///
    /// A missing last key of a map is added. Returns `None`, changing
    /// nothing, when the path's parent is missing, is not a map or list, or
    /// the last segment is not an index of the list.
    pub fn set(&self, value: &mut Value, new: Value) -> Option<Value> {
        let Some((last, parents)) = self.0.split_last() else {
            return Some(std::mem::replace(value, new));
        };
        let parent = Self(parents.to_vec()).get_mut(value)?;
        match parent {
            Value::Object(map) => Some(map.insert(last.clone(), new).unwrap_or(Value::Null)),
            Value::Array(items) => {
                let slot = items.get_mut(last.parse::<usize>().ok()?)?;
                Some(std::mem::replace(slot, new))
            }
            _ => None,
        }
    }
}

impl fmt::Display for SettingPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("."))
    }
}

/// One step into a map by key or a list by index.
fn step<'a>(value: &'a Value, segment: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map.get(segment),
        Value::Array(items) => items.get(segment.parse::<usize>().ok()?),
        _ => None,
    }
}

/// One step into a map by key or a list by index, to change it.
fn step_mut<'a>(value: &'a mut Value, segment: &str) -> Option<&'a mut Value> {
    match value {
        Value::Object(map) => map.get_mut(segment),
        Value::Array(items) => items.get_mut(segment.parse::<usize>().ok()?),
        _ => None,
    }
}

/// The path of every leaf in `value`: each number, text, `true` or `false`,
/// `null`, empty list and empty map, in the tree's order.
#[must_use]
pub fn leaves(value: &Value) -> Vec<SettingPath> {
    let mut found = Vec::new();
    collect_leaves(value, &SettingPath::default(), &mut found);
    found
}

fn collect_leaves(value: &Value, at: &SettingPath, found: &mut Vec<SettingPath>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                collect_leaves(child, &at.child(key.clone()), found);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, child) in items.iter().enumerate() {
                collect_leaves(child, &at.child(index.to_string()), found);
            }
        }
        _ => found.push(at.clone()),
    }
}

/// The path of every setting in `value`, containers included, in the tree's
/// order, starting with the whole tree's empty path.
#[must_use]
pub fn all_paths(value: &Value) -> Vec<SettingPath> {
    let mut found = Vec::new();
    collect_all(value, &SettingPath::default(), &mut found);
    found
}

fn collect_all(value: &Value, at: &SettingPath, found: &mut Vec<SettingPath>) {
    found.push(at.clone());
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                collect_all(child, &at.child(key.clone()), found);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_all(child, &at.child(index.to_string()), found);
            }
        }
        _ => {}
    }
}

/// A setting path in which `*` stands for any one segment, such as
/// `branch.length.*` for every branch level's length.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pattern(Vec<Option<String>>);

impl Pattern {
    /// Reads a dotted pattern; a segment of `*` matches any segment.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        Self(
            SettingPath::parse(text)
                .0
                .into_iter()
                .map(|segment| (segment != "*").then_some(segment))
                .collect(),
        )
    }

    /// Whether the pattern has a `*`.
    #[must_use]
    pub fn is_wild(&self) -> bool {
        self.0.iter().any(Option::is_none)
    }

    /// The one path the pattern matches, when it has no `*`.
    #[must_use]
    pub fn path(&self) -> Option<SettingPath> {
        self.0
            .iter()
            .cloned()
            .collect::<Option<Vec<_>>>()
            .map(SettingPath)
    }

    /// Whether `path` matches the pattern, and if so the index the last `*`
    /// matched (`None` when there is no `*`, or the segment is not a number).
    #[must_use]
    pub fn matches(&self, path: &SettingPath) -> Option<Match> {
        if self.0.len() != path.0.len() {
            return None;
        }
        let mut index = None;
        for (wanted, segment) in self.0.iter().zip(&path.0) {
            match wanted {
                Some(wanted) if wanted != segment => return None,
                Some(_) => {}
                None => index = segment.parse::<usize>().ok(),
            }
        }
        Some(Match { index })
    }

    /// Every path in `value` the pattern matches, in the tree's order.
    #[must_use]
    pub fn expand(&self, value: &Value) -> Vec<SettingPath> {
        all_paths(value)
            .into_iter()
            .filter(|path| self.matches(path).is_some())
            .collect()
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let segments: Vec<&str> = self.0.iter().map(|s| s.as_deref().unwrap_or("*")).collect();
        f.write_str(&segments.join("."))
    }
}

/// How a path matched a [`Pattern`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    /// The number the pattern's last `*` matched, such as the branch level
    /// in `branch.length.*`. It fills `{index}` in the setting's conditions.
    pub index: Option<usize>,
}

/// A path or name with `{index}` and `{parent}` (the index less one) in it,
/// such as `branch.length.{parent}` or `grows_{index}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Template(Vec<Piece>);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Text(String),
    /// The index, less the given amount.
    Index(usize),
}

impl Template {
    /// Reads a template; any `{…}` other than `{index}` and `{parent}` is
    /// an error, returned as its message.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut pieces = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            if open > 0 {
                pieces.push(Piece::Text(rest[..open].to_owned()));
            }
            let close = rest[open..]
                .find('}')
                .ok_or_else(|| format!("unclosed {{ in {text:?}"))?
                + open;
            pieces.push(match &rest[open + 1..close] {
                "index" => Piece::Index(0),
                "parent" => Piece::Index(1),
                other => return Err(format!("unknown {{{other}}} in {text:?}")),
            });
            rest = &rest[close + 1..];
        }
        if !rest.is_empty() {
            pieces.push(Piece::Text(rest.to_owned()));
        }
        Ok(Self(pieces))
    }

    /// Whether the template uses `{index}` or `{parent}`.
    pub(crate) fn uses_index(&self) -> bool {
        self.0.iter().any(|piece| matches!(piece, Piece::Index(_)))
    }

    /// The text with the index filled in, or `None` when the template needs
    /// an index and there is none (or `{parent}` of index 0).
    pub(crate) fn fill(&self, index: Option<usize>) -> Option<String> {
        let mut text = String::new();
        for piece in &self.0 {
            match piece {
                Piece::Text(part) => text.push_str(part),
                Piece::Index(less) => text.push_str(&index?.checked_sub(*less)?.to_string()),
            }
        }
        Some(text)
    }

    /// The path with the index filled in; see [`Template::fill`].
    pub(crate) fn path(&self, index: Option<usize>) -> Option<SettingPath> {
        self.fill(index).map(|text| SettingPath::parse(&text))
    }
}
