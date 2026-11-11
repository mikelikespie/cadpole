//! Targeted script edits and the diff that reports them.
//!
//! Rewriting a whole file to change one number is the most common way an
//! assistant destroys work it never read: it re-emits the whole file from memory
//! and quietly drops a comment added in between. `EditScript`
//! exists so the model sends the *change* instead, and the server refuses an
//! edit whose anchor is not exactly where the caller thinks it is.
//!
//! Three forms, checked in this order:
//!
//! | form | anchor | when to use it |
//! | --- | --- | --- |
//! | `old_str` -> `new_str` | an exact, unique snippet | almost always |
//! | `start_line` / `end_line` | 1-based inclusive lines | rewriting a block whose text is awkward to quote |
//! | `feature` | a feature's call, from its span | changing arguments when you have just read `ExplainRef` |
//!
//! Nothing is applied unless every edit applies: the caller gets one error
//! naming the edit that failed, and the file is untouched.

use std::collections::HashMap;

use cadpole_proto::v1 as pb;

/// Largest script a session will accept, in bytes (docs/security.md item 12).
pub const MAX_SCRIPT_BYTES: usize = 1024 * 1024;

/// Byte offsets of a source span, or `None` when the span does not land in
/// this text (a span from a different file, or a file that has since changed).
pub fn span_offsets(source: &str, span: &pb::SourceSpan) -> Option<(usize, usize)> {
    let start = offset_of(source, span.start_line, span.start_col)?;
    let end = offset_of(source, span.end_line, span.end_col)?;
    if end < start { None } else { Some((start, end)) }
}

/// Byte offset of a 1-based (line, column) position.
fn offset_of(source: &str, line: u32, col: u32) -> Option<usize> {
    if line == 0 {
        return None;
    }
    let mut at = 0usize;
    for (i, l) in source.split_inclusive('\n').enumerate() {
        if i as u32 + 1 == line {
            // Columns count characters, not bytes; a comment with an em dash
            // in it should not shift every span after it.
            let mut rest = l.char_indices();
            let want = col.saturating_sub(1) as usize;
            return Some(at + rest.nth(want).map(|(b, _)| b).unwrap_or(l.len()));
        }
        at += l.len();
    }
    // A position one past the end of the last line is legal (an empty span at
    // EOF); anything further is not.
    if line as usize == source.split_inclusive('\n').count() + 1 { Some(source.len()) } else { None }
}

/// The argument list of the call at `span`: the byte range strictly inside the
/// call's outermost parentheses.
///
/// Parentheses inside string literals do not count, which is the whole reason
/// this is not a `find('(')`: `text("(")` is a perfectly ordinary Cadpole call.
fn argument_range(source: &str, span: &pb::SourceSpan) -> anyhow::Result<(usize, usize)> {
    let (start, end) = span_offsets(source, span).ok_or_else(|| anyhow::anyhow!("the recorded span is not in this file"))?;
    let region = &source[start..end];
    let bytes = region.as_bytes();
    let mut depth = 0i32;
    let mut open = None;
    let mut quote: Option<u8> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) => {
                if c == b'\\' {
                    i += 2;
                    continue;
                }
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                b'"' | b'\'' => quote = Some(c),
                b'#' => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    continue;
                }
                b'(' => {
                    if depth == 0 {
                        open = Some(i + 1);
                    }
                    depth += 1;
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        let o = open.ok_or_else(|| anyhow::anyhow!("unbalanced parentheses in the call"))?;
                        return Ok((start + o, start + i));
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    anyhow::bail!("could not find the call's argument list in the recorded span")
}

/// Apply `edits` in order. `spans` maps a feature id to the span of its call,
/// for the `feature` form; it may be empty when no edit uses that form.
///
/// Returns the new source. Either every edit applied or none did.
pub fn apply_edits(source: &str, edits: &[pb::ScriptEdit], spans: &HashMap<String, pb::SourceSpan>) -> anyhow::Result<String> {
    if edits.is_empty() {
        anyhow::bail!("no edits given: pass at least one {{old_str, new_str}}");
    }
    let mut out = source.to_owned();
    for (i, e) in edits.iter().enumerate() {
        let n = i + 1;
        if !e.old_str.is_empty() {
            let count = out.matches(&e.old_str).count();
            if count == 0 {
                anyhow::bail!(
                    "edit {n}: old_str does not appear in the file. Copy it exactly from the current \
                     source (whitespace included), or use the line-range form."
                );
            }
            if count > 1 && !e.replace_all {
                anyhow::bail!(
                    "edit {n}: old_str appears {count} times; it must be unique. Widen the snippet \
                     until only one place matches, or set replace_all."
                );
            }
            out = if e.replace_all { out.replace(&e.old_str, &e.new_str) } else { out.replacen(&e.old_str, &e.new_str, 1) };
            continue;
        }
        if e.start_line > 0 {
            let lines: Vec<&str> = out.split_inclusive('\n').collect();
            let first = e.start_line as usize;
            let last = if e.end_line == 0 { first } else { e.end_line as usize };
            if last < first {
                anyhow::bail!("edit {n}: end_line {last} is before start_line {first}");
            }
            if first > lines.len() {
                anyhow::bail!("edit {n}: start_line {first} is past the end of the file ({} lines)", lines.len());
            }
            let last = last.min(lines.len());
            let start: usize = lines[..first - 1].iter().map(|l| l.len()).sum();
            let end: usize = lines[..last].iter().map(|l| l.len()).sum();
            let mut replacement = e.new_str.clone();
            // Keep the file line-oriented: a replacement for whole lines ends
            // in a newline unless it is the (newline-less) last line.
            if !replacement.is_empty() && !replacement.ends_with('\n') && end < out.len() {
                replacement.push('\n');
            }
            out.replace_range(start..end, &replacement);
            continue;
        }
        if !e.feature.is_empty() {
            let span = spans
                .get(&e.feature)
                .ok_or_else(|| anyhow::anyhow!("edit {n}: no feature named {:?} in the current model", e.feature))?;
            let (a, b) = argument_range(&out, span).map_err(|err| anyhow::anyhow!("edit {n}: {err}"))?;
            out.replace_range(a..b, &e.new_str);
            continue;
        }
        anyhow::bail!("edit {n}: needs one of old_str, start_line or feature");
    }
    if out.len() > MAX_SCRIPT_BYTES {
        anyhow::bail!("the edited script would be {} bytes, over the {MAX_SCRIPT_BYTES} byte limit", out.len());
    }
    Ok(out)
}

/// A unified diff of two texts, truncated to `max_lines` body lines.
///
/// Hand-rolled rather than a crate: it is 60 lines, the input is a script
/// (hundreds of lines, not a kernel tree), and a diff library would be a
/// dependency and a licence review for something this small.
pub fn unified_diff(old: &str, new: &str, path: &str, max_lines: usize) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    if a == b {
        return String::new();
    }
    // O(n*m) LCS. Guarded, because a pathological pair of big files would be
    // a denial of service all by itself.
    const MAX_CELLS: usize = 4_000_000;
    if a.len() * b.len() > MAX_CELLS {
        return format!("--- a/{path}\n+++ b/{path}\n@@ files too large to diff ({} -> {} lines) @@\n", a.len(), b.len());
    }
    let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut body = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            body.push(format!(" {}", a[i]));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            body.push(format!("-{}", a[i]));
            i += 1;
        } else {
            body.push(format!("+{}", b[j]));
            j += 1;
        }
    }
    body.extend(a[i..].iter().map(|l| format!("-{l}")));
    body.extend(b[j..].iter().map(|l| format!("+{l}")));

    let truncated = body.len() > max_lines;
    let shown = body.len().min(max_lines);
    let mut out = format!("--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@\n", a.len(), b.len());
    for l in &body[..shown] {
        out.push_str(l);
        out.push('\n');
    }
    if truncated {
        out.push_str(&format!("... {} more diff lines\n", body.len() - shown));
    }
    out
}

/// Feature id -> the span of the call that created it, for the `feature` edit
/// form.
pub fn feature_spans(ctx: &cadpole_core::Context) -> HashMap<String, pb::SourceSpan> {
    ctx.features
        .iter()
        .filter_map(|f| f.span.as_ref().map(|s| (f.id.0.clone(), cadpole_core::context::Span::to_proto(s))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(old: &str, new: &str) -> pb::ScriptEdit {
        pb::ScriptEdit { old_str: old.into(), new_str: new.into(), ..Default::default() }
    }

    const SRC: &str = "\
# a bracket
w = param(\"w\", 40)
h = param(\"h\", 10)
base = extrude(sketch(XY).rect(w, 20), h, name=\"base\")
";

    #[test]
    fn a_unique_snippet_is_replaced() {
        let out = apply_edits(SRC, &[edit("param(\"h\", 10)", "param(\"h\", 25)")], &HashMap::new()).unwrap();
        assert!(out.contains("param(\"h\", 25)"));
        assert!(out.contains("# a bracket"), "the rest of the file is untouched");
    }

    #[test]
    fn edits_apply_in_order_and_all_or_nothing() {
        let out = apply_edits(SRC, &[edit("40", "50"), edit("50", "60")], &HashMap::new()).unwrap();
        assert!(out.contains("param(\"w\", 60)"), "{out}");

        let e = apply_edits(SRC, &[edit("40", "50"), edit("nowhere", "x")], &HashMap::new()).unwrap_err().to_string();
        assert!(e.contains("edit 2"), "the error names the edit: {e}");
        assert!(e.contains("does not appear"), "{e}");
    }

    #[test]
    fn an_ambiguous_snippet_is_refused_unless_replace_all() {
        let src = "a = 1\nb = 1\n";
        let e = apply_edits(src, &[edit("= 1", "= 2")], &HashMap::new()).unwrap_err().to_string();
        assert!(e.contains("appears 2 times"), "{e}");
        let out = apply_edits(src, &[pb::ScriptEdit { replace_all: true, ..edit("= 1", "= 2") }], &HashMap::new()).unwrap();
        assert_eq!(out, "a = 2\nb = 2\n");
    }

    #[test]
    fn a_line_range_is_replaced_whole() {
        let out = apply_edits(
            SRC,
            &[pb::ScriptEdit { start_line: 2, end_line: 3, new_str: "w = param(\"w\", 1)".into(), ..Default::default() }],
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(out, "# a bracket\nw = param(\"w\", 1)\nbase = extrude(sketch(XY).rect(w, 20), h, name=\"base\")\n");

        let e = apply_edits(SRC, &[pb::ScriptEdit { start_line: 99, ..Default::default() }], &HashMap::new())
            .unwrap_err()
            .to_string();
        assert!(e.contains("past the end"), "{e}");
    }

    #[test]
    fn a_features_arguments_are_replaced_through_its_span() {
        let doc = crate::document_from_scripts("t", vec![("main.star".into(), SRC.to_owned())]);
        let (ctx, _) = crate::evaluate_document(&doc, &Default::default(), 0.0);
        let spans = feature_spans(&ctx);
        assert!(spans.contains_key("base"), "{:?}", spans.keys().collect::<Vec<_>>());
        let out = apply_edits(
            SRC,
            &[pb::ScriptEdit { feature: "base".into(), new_str: "sketch(XY).rect(w, 30), h * 2, name=\"base\"".into(), ..Default::default() }],
            &spans,
        )
        .unwrap();
        assert!(out.contains("extrude(sketch(XY).rect(w, 30), h * 2, name=\"base\")"), "{out}");
        assert!(out.starts_with("# a bracket"), "{out}");
    }

    #[test]
    fn parentheses_inside_strings_do_not_end_the_argument_list() {
        let src = "t = text(\"a)b\", size=4, name=\"t\")\n";
        let span = pb::SourceSpan { path: "main.star".into(), start_line: 1, start_col: 5, end_line: 1, end_col: 34 };
        let (a, b) = argument_range(src, &span).unwrap();
        assert_eq!(&src[a..b], "\"a)b\", size=4, name=\"t\"");
    }

    #[test]
    fn a_diff_shows_the_change_and_truncates() {
        let d = unified_diff(SRC, &SRC.replace("40", "50"), "main.star", 200);
        assert!(d.contains("-w = param(\"w\", 40)"), "{d}");
        assert!(d.contains("+w = param(\"w\", 50)"), "{d}");
        assert!(d.starts_with("--- a/main.star"), "{d}");
        assert_eq!(unified_diff(SRC, SRC, "main.star", 200), "", "no change, no diff");

        let big: String = (0..500).map(|i| format!("x{i} = {i}\n")).collect();
        let d = unified_diff("", &big, "main.star", 10);
        assert!(d.contains("more diff lines"), "{d}");
        assert!(d.lines().count() < 20, "truncated: {}", d.lines().count());
    }

    #[test]
    fn an_edit_that_would_blow_the_size_cap_is_refused() {
        let huge = "x".repeat(MAX_SCRIPT_BYTES + 1);
        let e = apply_edits("a = 1\n", &[edit("a", &huge)], &HashMap::new()).unwrap_err().to_string();
        assert!(e.contains("byte limit"), "{e}");
    }

    #[test]
    fn an_empty_edit_says_what_it_needs() {
        let e = apply_edits(SRC, &[pb::ScriptEdit::default()], &HashMap::new()).unwrap_err().to_string();
        assert!(e.contains("old_str, start_line or feature"), "{e}");
        let e = apply_edits(SRC, &[], &HashMap::new()).unwrap_err().to_string();
        assert!(e.contains("no edits given"), "{e}");
    }
}
