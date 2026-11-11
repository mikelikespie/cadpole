use std::fmt;

/// Identifier of a feature in the tree. Stable across evaluations as long as
/// the script's call order (or explicit `name=`) does not change.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FeatureId(pub String);

impl fmt::Display for FeatureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for FeatureId {
    fn from(s: &str) -> Self {
        FeatureId(s.to_owned())
    }
}

/// Identifier of a body. Bodies are produced by features; the id is
/// `<feature id>` for the feature's primary body, `<feature id>#<n>` for
/// additional ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BodyId(pub u32);

impl fmt::Display for BodyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "body#{}", self.0)
    }
}
