//! Loading documents from `.star` scripts or packed document files.

use cadpole_engine::cadpole_proto::file_types::DOCUMENT_EXT;
use std::path::Path;

use anyhow::Context as _;
use cadpole_engine::cadpole_assets;
use cadpole_engine::cadpole_assets::AssetStore as _;
use cadpole_engine::cadpole_proto::prost::Message;
use cadpole_engine::cadpole_proto::v1 as pb;

/// Load a document. For a `.star` file the file becomes the entry script
/// (`main.star`); if it uses `load()`, every other `.star` in the same
/// directory is bundled so they can be loaded -- except a real `main.star`
/// beside a differently named entry, which [`entry_owns_main_star`] handles.
pub fn load(path: &Path) -> anyhow::Result<pb::Document> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        e if e == DOCUMENT_EXT => {
            let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            pb::Document::decode(bytes.as_slice()).with_context(|| format!("decoding {}", path.display()))
        }
        "star" => {
            let entry = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("model").to_owned();
            let uses_load = entry.contains("load(");
            let mut scripts = vec![("main.star".to_owned(), entry)];
            let dir_of = path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from("."));
            // Sibling .star files are only bundled when the entry script can
            // load them; otherwise unrelated examples would clutter the document.
            if let (Some(dir), true) = (path.parent(), uses_load) {
                let mut sibs: Vec<_> = std::fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p != path && p.extension().and_then(|e| e.to_str()) == Some("star"))
                    .collect();
                sibs.sort();
                for p in sibs {
                    if let (Some(name), Ok(src)) = (p.file_name().and_then(|n| n.to_str()), std::fs::read_to_string(&p)) {
                        scripts.push((name.to_owned(), src));
                    }
                }
            }
            if uses_load {
                bundle_loads(&dir_of, &mut scripts);
            }
            entry_owns_main_star(path, &dir_of, &mut scripts)?;
            let mut doc = cadpole_engine::document_from_scripts(&stem, scripts);
            doc.assets = loose_assets(&dir_of);
            Ok(doc)
        }
        _ => anyhow::bail!("{}: expected a .star script or a .{DOCUMENT_EXT} document", path.display()),
    }
}

/// A document's entry script is the one keyed `main.star` (document.proto),
/// and [`load`] keys the file it was given there whatever its name. A real
/// `main.star` beside a differently named entry would be a second script
/// under the same key, and a `load("main.star")` would resolve to the entry
/// itself -- a "load cycle: main.star -> main.star" that is not in the
/// user's files. So the sibling is left out when nothing loads it, and when
/// something does the command is refused, naming both files and the way out.
fn entry_owns_main_star(path: &Path, dir: &Path, scripts: &mut Vec<(String, String)>) -> anyhow::Result<()> {
    const ENTRY: &str = "main.star";
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(ENTRY);
    if name == ENTRY || !dir.join(ENTRY).is_file() {
        return Ok(());
    }
    // The first script is the entry; the sibling may have been bundled
    // after it under the same key.
    if let Some(i) = scripts.iter().skip(1).position(|(k, _)| k == ENTRY) {
        scripts.remove(i + 1);
    }
    // Only what the entry reaches matters: an unrelated sibling that loads
    // main.star is bundled but never run.
    let mut reached = vec![0usize];
    let mut loaders = Vec::new();
    let mut i = 0;
    while i < reached.len() {
        let (key, src) = &scripts[reached[i]];
        // Real load statements only: this decides an error, and a comment
        // that mentions load("main.star") loads nothing.
        for t in load_targets(&without_comments(src)).iter().filter(|t| !t.starts_with('@')) {
            let k = cadpole_engine::cadpole_script::resolve_load(key, t);
            if k == ENTRY {
                let who = if reached[i] == 0 { name.to_owned() } else { key.clone() };
                if !loaders.contains(&who) {
                    loaders.push(who);
                }
            } else if let Some(j) = scripts.iter().position(|(p, _)| *p == k) {
                if !reached.contains(&j) {
                    reached.push(j);
                }
            }
        }
        i += 1;
    }
    if !loaders.is_empty() {
        anyhow::bail!(
            "{}: {} calls load(\"main.star\"), but {name} is the entry script here, and a document's entry script is the \
             one named main.star, so the main.star beside it cannot be loaded as well; rename that file (to lib.star, \
             say) and load it by its new name, or run main.star itself",
            path.display(),
            loaders.join(", ")
        );
    }
    Ok(())
}

/// Every file a bundled script `load()`s from outside the entry's directory,
/// transitively, under the key the loader resolves it to:
/// `load("../lib/sheets.star")` from `main.star` is bundled as
/// `../lib/sheets.star`, and a `load("helpers.star")` inside that file as
/// `../lib/helpers.star`, beside it. Siblings of the entry are already in.
/// A path that is not there is left for the loader to report, with the
/// line it was written on.
fn bundle_loads(dir: &Path, scripts: &mut Vec<(String, String)>) {
    let mut i = 0;
    while i < scripts.len() {
        let (key, src) = scripts[i].clone();
        for target in load_targets(&src) {
            if target.starts_with('@') {
                continue;
            }
            let k = cadpole_engine::cadpole_script::resolve_load(&key, &target);
            if k.is_empty() || scripts.iter().any(|(p, _)| *p == k) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(dir.join(&k)) {
                scripts.push((k, text));
            }
        }
        i += 1;
    }
}

/// The first argument of every `load("...")` in `src`: a string literal
/// after `load(`, however it is quoted. Comments that mention a load are
/// harmless -- a file that is not there is simply not bundled.
/// `src` with every `#` comment cut off its line (a `#` inside a string
/// literal is kept).
fn without_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.split_inclusive('\n') {
        let mut quote: Option<char> = None;
        let mut escaped = false;
        let mut cut = line.len();
        for (i, c) in line.char_indices() {
            match quote {
                Some(_) if escaped => escaped = false,
                Some(_) if c == '\\' => escaped = true,
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == '"' || c == '\'' => quote = Some(c),
                None if c == '#' => {
                    cut = i;
                    break;
                }
                None => {}
            }
        }
        out.push_str(&line[..cut]);
        if cut < line.len() && line.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

pub(crate) fn load_targets(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find("load(") {
        let before = rest[..i].chars().last();
        rest = &rest[i + "load(".len()..];
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
            continue;
        }
        let t = rest.trim_start();
        let Some(q) = t.chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
        if let Some(end) = t[1..].find(q) {
            out.push(t[1..1 + end].to_owned());
        }
    }
    out
}

/// What a loose `./assets` directory may hold in this process: set
/// once by `main` from `--max-asset-mb` / `CADPOLE_MAX_ASSET_MB`, the native
/// caps otherwise.
static ASSET_LIMITS: std::sync::OnceLock<cadpole_assets::Limits> = std::sync::OnceLock::new();

/// Set the caps [`load`] reads `./assets` under. The first call wins.
pub fn set_asset_limits(limits: cadpole_assets::Limits) {
    let _ = ASSET_LIMITS.set(limits);
}

/// The caps [`load`] reads `./assets` under.
pub fn asset_limits() -> cadpole_assets::Limits {
    ASSET_LIMITS.get().copied().unwrap_or(cadpole_assets::Limits::NATIVE)
}

/// Loose mode: the files a `.star` script
/// can name with `asset("...")` live in `./assets` next to it, and the
/// document is *packed* with them so everything downstream — the engine, a
/// render host, a saved packed document — sees one self-contained document.
///
/// A missing `assets` directory is not an error: most scripts have none.
/// Reproducibility comes from the file system (git), exactly as it does for
/// the script itself; the hash the CLI records is what `asset(name,
/// sha256=...)` asserts against. A file over its cap ([`asset_limits`]) is
/// left out with a warning that says how to raise it.
fn loose_assets(dir: &Path) -> Vec<pb::Asset> {
    let (store, warnings) = cadpole_assets::load_dir_with(&dir.join("assets"), &asset_limits());
    for w in warnings {
        eprintln!("warning: {w}");
    }
    store
        .names()
        .into_iter()
        .filter_map(|name| {
            let (a, bytes) = store.get(&name)?;
            Some(cadpole_engine::asset_to_proto(&a, bytes.to_vec()))
        })
        .collect()
}

pub fn save(path: &Path, doc: &pb::Document) -> anyhow::Result<()> {
    std::fs::write(path, doc.encode_to_vec()).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document's entry script is the one at `main.star`, so `cadpole eval
    /// part.star` runs part.star under that key. A real `main.star` beside
    /// it cannot also be in the document: when nothing loads it, it is left
    /// out (one script per key); when something does, the command says why it
    /// cannot run rather than reporting a load cycle that is not there.
    #[test]
    fn a_sibling_named_main_star_is_never_a_second_entry() {
        let root = std::env::temp_dir().join(format!("cadpole-cli-main-sibling-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.star"), "W = 42\n").unwrap();
        std::fs::write(root.join("lib.star"), "W = 7\n").unwrap();
        std::fs::write(root.join("part.star"), "load(\"main.star\", \"W\")\nbox(W, 1, 1)\n").unwrap();
        std::fs::write(root.join("bar.star"), "load(\"lib.star\", \"W\")\nbox(W, 2, 2)\n").unwrap();

        let e = format!("{:#}", load(&root.join("part.star")).unwrap_err());
        assert!(e.contains("part.star") && e.contains("main.star") && e.contains("rename"), "{e}");

        // The same from the directory itself, as `cadpole eval part.star` is
        // usually typed: the entry's directory is then the empty path.
        if let Some(bin) = crate::tests::cadpole_binary() {
            let out = std::process::Command::new(bin).args(["eval", "part.star"]).current_dir(&root).output().unwrap();
            let err = String::from_utf8_lossy(&out.stderr);
            assert_eq!(out.status.code(), Some(2), "{err}");
            assert!(err.contains("rename") && !err.contains("load cycle"), "{err}");
        }

        let doc = load(&root.join("bar.star")).unwrap();
        let mains: Vec<&str> = doc.scripts.iter().filter(|s| s.path == "main.star").map(|s| s.source.as_str()).collect();
        assert_eq!(mains, vec!["load(\"lib.star\", \"W\")\nbox(W, 2, 2)\n"], "one entry, and it is the file asked for");

        // A load of main.star in a comment, in the entry or in what it loads,
        // loads nothing and is no reason to refuse.
        std::fs::write(root.join("foo.star"), "# was: load(\"main.star\", \"W\")\nload(\"lib.star\", \"W\")\nbox(W, 2, 2)\n").unwrap();
        let doc = load(&root.join("foo.star")).unwrap_or_else(|e| panic!("{e:#}"));
        assert_eq!(doc.scripts.iter().filter(|s| s.path == "main.star").count(), 1);
        std::fs::write(root.join("lib.star"), "# see load(\"main.star\")\nW = 7\n").unwrap();
        let doc = load(&root.join("bar.star")).unwrap_or_else(|e| panic!("{e:#}"));
        assert_eq!(doc.scripts.iter().filter(|s| s.path == "main.star").count(), 1);
        // A '#' inside a string is not a comment.
        assert_eq!(without_comments("x = \"a#b\" # c\n"), "x = \"a#b\" \n");

        // main.star itself, loading a sibling, is the ordinary case.
        std::fs::write(root.join("main.star"), "load(\"lib.star\", \"W\")\nbox(W, 3, 3)\n").unwrap();
        let doc = load(&root.join("main.star")).unwrap();
        assert_eq!(doc.scripts.iter().filter(|s| s.path == "main.star").count(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_targets_are_the_string_after_load() {
        let src = "load(\"@cadpole/site.star\", \"ft\")\nload( '../lib/sheets.star', \"a\")\nx = reload(\"no.star\")\n# load(\"c.star\")\n";
        assert_eq!(load_targets(src), vec!["@cadpole/site.star", "../lib/sheets.star", "c.star"]);
    }

    /// A sheet script in `sheets/` loads its model from `../model/` and the
    /// model loads its own sibling: both are bundled, under the keys the
    /// loader resolves them to, and the document evaluates.
    #[test]
    fn a_load_outside_the_entry_directory_is_bundled_transitively() {
        let root = std::env::temp_dir().join(format!("cadpole-cli-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sheets")).unwrap();
        std::fs::create_dir_all(root.join("model")).unwrap();
        std::fs::write(root.join("model/site.star"), "load(\"dims.star\", \"W\")\nhouse = box(W, 10, 10, name=\"house\")\n").unwrap();
        std::fs::write(root.join("model/dims.star"), "W = 42\n").unwrap();
        std::fs::write(root.join("sheets/a.star"), "load(\"../model/site.star\", \"house\")\nassert_eq(house.bbox()[1][0], 42.0)\n").unwrap();
        let doc = load(&root.join("sheets/a.star")).unwrap();
        let paths: Vec<&str> = doc.scripts.iter().map(|s| s.path.as_str()).collect();
        assert!(paths.contains(&"../model/site.star") && paths.contains(&"../model/dims.star"), "{paths:?}");
        let (ctx, _, _) = cadpole_engine::evaluate_document_limited(
            &doc,
            &std::collections::HashMap::new(),
            0.0,
            0.0,
            &std::collections::HashMap::new(),
            cadpole_engine::cadpole_script::Limits::structural(),
        );
        let errors: Vec<&str> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_engine::cadpole_core::context::Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
