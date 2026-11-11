//! Reading the two lockfiles Cadpole ships from, and the version algebra the
//! advisory database is written in.
//!
//! Three things use this: the licence test
//! (`//build_support/licenses:license_test`), the advisory test
//! (`//build_support/advisories:advisory_test`) and the attribution generator
//! (`//build_support/licenses:gen_attribution`). It has no dependencies, on
//! purpose -- a checker that enforces the dependency policy should not be able
//! to violate it.
//!
//! The parsers are deliberately line-oriented rather than full TOML/YAML: the
//! only thing that must be read out of either lockfile is the (name, version)
//! of every package, and the shapes are fixed and machine-written.

// ---------------------------------------------------------------- packages

/// One resolved package: what a lockfile row names, and the version it pins.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Package {
    pub name: String,
    pub version: String,
    /// `false` for a workspace member -- Cargo.lock gives those no `source`,
    /// and pnpm keeps them in `importers:` rather than `packages:`. A crate
    /// with no source is one of Cadpole's own.
    pub third_party: bool,
}

impl Package {
    pub fn id(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

/// Every `[[package]]` in a `Cargo.lock`, in file order.
pub fn cargo_lock_packages(lock: &str) -> Vec<Package> {
    let mut out = Vec::new();
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut source = false;
    let mut flush = |name: &mut Option<String>, version: &mut Option<String>, source: &mut bool| {
        if let (Some(n), Some(v)) = (name.take(), version.take()) {
            out.push(Package {
                name: n,
                version: v,
                third_party: *source,
            });
        }
        *source = false;
    };
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            flush(&mut name, &mut version, &mut source);
        } else if let Some(v) = line.strip_prefix("name = ") {
            name = Some(v.trim_matches('"').to_owned());
        } else if let Some(v) = line.strip_prefix("version = ") {
            version = Some(v.trim_matches('"').to_owned());
        } else if line.starts_with("source = ") {
            source = true;
        }
    }
    flush(&mut name, &mut version, &mut source);
    out
}

/// Every package in a pnpm lockfile's top-level `packages:` map.
///
/// A key is `<name>@<version>`, quoted when the name is scoped, and may carry
/// a `(peer@1.2.3)` suffix that identifies the peer resolution rather than the
/// package: `'@types/three@0.186.0':`, `three@0.186.0:`,
/// `'@vue/compiler@3.0.0(vue@3.0.0)':`. Workspace packages live in
/// `importers:` instead and never appear here, which is why every package this
/// returns is third party.
pub fn pnpm_lock_packages(lock: &str) -> Vec<Package> {
    let mut out: Vec<Package> = Vec::new();
    let mut in_packages = false;
    for line in lock.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            in_packages = line.starts_with("packages:");
            continue;
        }
        if !in_packages {
            continue;
        }
        // Exactly one level in, and a map key: `  <spec>:`.
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        if rest.starts_with(' ') || !rest.ends_with(':') {
            continue;
        }
        let spec = rest[..rest.len() - 1]
            .trim()
            .trim_matches('\'')
            .trim_matches('"');
        let spec = match spec.find('(') {
            Some(i) => &spec[..i],
            None => spec,
        };
        // Scoped names start with `@`, so split on the *last* `@`.
        let Some(at) = spec.rfind('@') else { continue };
        if at == 0 {
            continue;
        }
        let pkg = Package {
            name: spec[..at].to_owned(),
            version: spec[at + 1..].to_owned(),
            third_party: true,
        };
        if !out.contains(&pkg) {
            out.push(pkg);
        }
    }
    out
}

// --------------------------------------------------------------------- tsv

/// Data rows of one of the `.tsv` inventories: comments, the header line and
/// blank lines dropped, every row split on tabs.
pub fn tsv_rows(text: &str) -> Vec<Vec<&str>> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .skip_while(|l| l.starts_with("name\t") || l.starts_with("id\t"))
        .map(|l| l.split('\t').collect())
        .collect()
}

/// The value of a `# <key>: ...` header line, if the file has one.
pub fn tsv_header(text: &str, key: &str) -> Option<String> {
    let want = format!("# {key}:");
    text.lines()
        .take_while(|l| l.starts_with('#'))
        .find(|l| l.starts_with(&want))
        .map(|l| l[want.len()..].trim().to_owned())
}

// ------------------------------------------------------------------ semver

/// A parsed version. Build metadata (`+1.3.281`) is dropped -- it never
/// participates in ordering -- and a pre-release tag sorts *before* the
/// release with the same numbers, the way Cargo and SemVer say.
#[derive(Clone, Debug)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: String,
    /// How many numbers the text actually carried: `1.2` is 2, `1` is 1. A
    /// comparator's precision is what makes `^0.6` and `~1.2` mean what they
    /// mean.
    pub precision: usize,
}

impl Version {
    pub fn parse(text: &str) -> Version {
        let text = text.trim().trim_start_matches('v');
        let text = text.split('+').next().unwrap_or("");
        let (numbers, pre) = match text.find('-') {
            Some(i) => (&text[..i], text[i + 1..].to_owned()),
            None => (text, String::new()),
        };
        let mut v = Version {
            major: 0,
            minor: 0,
            patch: 0,
            pre,
            precision: 0,
        };
        for (i, part) in numbers.split('.').enumerate() {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() && i == 0 {
                break;
            }
            let n = digits.parse().unwrap_or(0);
            match i {
                0 => v.major = n,
                1 => v.minor = n,
                2 => v.patch = n,
                _ => {}
            }
            if i < 3 {
                v.precision = i + 1;
            }
        }
        v
    }

    fn numbers(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

/// Equality is the SemVer order, so `1` and `1.0.0` are the same version even
/// though only one of them wrote the zeros.
impl PartialEq for Version {
    fn eq(&self, other: &Version) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match self.numbers().cmp(&other.numbers()) {
            Equal => match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Equal,
                (true, false) => Greater,
                (false, true) => Less,
                (false, false) => self.pre.cmp(&other.pre),
            },
            other => other,
        }
    }
}

/// The first version *excluded* by `^v`, exactly as Cargo defines it:
/// `^1.2.3` and `^1` -> 2.0.0, `^0.6.1` and `^0.6` -> 0.7.0, `^0.0.3` -> 0.0.4,
/// `^0.0` -> 0.1.0, `^0` -> 1.0.0.
fn caret_ceiling(v: &Version) -> Version {
    let (major, minor, patch) = if v.major > 0 {
        (v.major + 1, 0, 0)
    } else if v.minor > 0 {
        (0, v.minor + 1, 0)
    } else if v.precision >= 3 {
        (0, 0, v.patch + 1)
    } else if v.precision == 2 {
        (0, 1, 0)
    } else {
        (1, 0, 0)
    };
    Version {
        major,
        minor,
        patch,
        pre: String::new(),
        precision: 3,
    }
}

/// The first version excluded by `~v`: `~1.2.3` and `~1.2` -> 1.3.0,
/// `~1` -> 2.0.0.
fn tilde_ceiling(v: &Version) -> Version {
    if v.precision >= 2 {
        Version {
            major: v.major,
            minor: v.minor + 1,
            patch: 0,
            pre: String::new(),
            precision: 3,
        }
    } else {
        Version {
            major: v.major + 1,
            minor: 0,
            patch: 0,
            pre: String::new(),
            precision: 3,
        }
    }
}

/// One comparator: `>= 1.2.0`, `< 2.0.0`, `^0.6`, `~1.2`, `=0.2.4`, `*`, or a
/// bare version, which Cargo reads as `^`.
fn satisfies_comparator(version: &Version, comparator: &str) -> bool {
    use std::cmp::Ordering::*;
    let c = comparator.trim();
    if c.is_empty() || c == "*" {
        return true;
    }
    let (op, rest) = if let Some(r) = c.strip_prefix(">=") {
        (">=", r)
    } else if let Some(r) = c.strip_prefix("<=") {
        ("<=", r)
    } else if let Some(r) = c.strip_prefix("==") {
        ("=", r)
    } else if let Some(r) = c.strip_prefix('>') {
        (">", r)
    } else if let Some(r) = c.strip_prefix('<') {
        ("<", r)
    } else if let Some(r) = c.strip_prefix('=') {
        ("=", r)
    } else if let Some(r) = c.strip_prefix('^') {
        ("^", r)
    } else if let Some(r) = c.strip_prefix('~') {
        ("~", r)
    } else {
        ("^", c)
    };
    let bound = Version::parse(rest);
    match op {
        ">=" => *version >= bound,
        ">" => *version > bound,
        "<=" => *version <= bound,
        "<" => *version < bound,
        // A partial `=` is a prefix match, which is how RustSec writes
        // `unaffected = ["=0.2.4"]`.
        "=" => match bound.precision {
            0 => true,
            1 => version.major == bound.major,
            2 => version.major == bound.major && version.minor == bound.minor,
            _ => version.cmp(&bound) == Equal,
        },
        "^" => *version >= bound && *version < caret_ceiling(&bound),
        "~" => *version >= bound && *version < tilde_ceiling(&bound),
        _ => false,
    }
}

/// One range: a comma-separated conjunction, `">= 0.9.3, < 0.10.0"`.
pub fn satisfies(version: &Version, range: &str) -> bool {
    range
        .split(',')
        .filter(|c| !c.trim().is_empty())
        .all(|c| satisfies_comparator(version, c))
}

/// `;`-separated ranges, as the advisory inventories store them. Any range
/// matching is enough. An empty field matches nothing.
pub fn satisfies_any(version: &Version, ranges: &str) -> bool {
    ranges
        .split(';')
        .filter(|r| !r.trim().is_empty())
        .any(|r| satisfies(version, r))
}
