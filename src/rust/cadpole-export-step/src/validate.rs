//! A structural validator for the part 21 text this crate writes.
//!
//! It is not an EXPRESS checker — that would mean implementing the AP214
//! schema — but it catches the class of bug an exporter actually produces:
//! an entity id used before it is written, an id written twice, a shell whose
//! edges do not pair up, a solid whose voids are mis-oriented, an entity left
//! behind by a code path that changed its mind. `truck-stepio`'s own writer
//! fails two of these checks, which is the best
//! argument there is for running them.
//!
//! What is checked:
//!
//! 1. the file frame — `ISO-10303-21;`, a `HEADER;` with `FILE_DESCRIPTION`,
//!    `FILE_NAME` and `FILE_SCHEMA`, a `DATA;` section, `END-ISO-10303-21;`;
//! 2. every entity id is defined exactly once;
//! 3. every `#n` referenced is defined;
//! 4. nothing refers to itself, and the reference graph has no cycles;
//! 5. no orphans: the only entities nothing refers to are the three the
//!    AP214 product structure leaves at the top;
//! 6. every `CLOSED_SHELL` is closed — each of its edges used exactly twice,
//!    once in each direction;
//! 7. every `ADVANCED_FACE` has at most one `FACE_OUTER_BOUND` (AP214) and at
//!    least one bound at all;
//! 8. every void of a `BREP_WITH_VOIDS` is oriented `.F.` (ISO 10303-42 WR1).
//!
//! Parsing is deliberately independent of the writer: it re-reads the text
//! rather than inspecting the [`crate::text::Emitter`] that produced it, so a
//! bug in formatting shows up here instead of hiding behind shared state.

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// One entity instance as read back out of the file.
#[derive(Clone, Debug)]
pub struct Entity {
    pub id: usize,
    /// Every entity name in the instance. A simple instance has one; a
    /// complex instance (`( BOUNDED_CURVE() B_SPLINE_CURVE(...) ... )`) has
    /// one per sub-supertype.
    pub types: Vec<String>,
    /// The instance text after `#n = `, without the trailing `;`.
    pub body: String,
    /// Every `#n` the instance refers to, in the order they appear.
    pub refs: Vec<usize>,
}

impl Entity {
    pub fn is(&self, name: &str) -> bool {
        self.types.iter().any(|t| t == name)
    }
}

/// The entities of a file's data section, by id.
pub type Entities = BTreeMap<usize, Entity>;

/// Check `text`. An empty result means the file passed every check above.
pub fn validate(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    check_frame(text, &mut problems);
    let entities = match parse_data(text, &mut problems) {
        Some(e) => e,
        None => return problems,
    };
    check_references(&entities, &mut problems);
    check_acyclic(&entities, &mut problems);
    check_orphans(&entities, &mut problems);
    check_faces(&entities, &mut problems);
    check_shells(&entities, &mut problems);
    check_voids(&entities, &mut problems);
    problems
}

/// [`validate`] as a `Result`, for a test or a `?`.
pub fn check(text: &str) -> Result<(), String> {
    let p = validate(text);
    if p.is_empty() { Ok(()) } else { Err(p.join("\n")) }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

fn check_frame(text: &str, out: &mut Vec<String>) {
    for (what, needle) in [
        ("the part 21 prologue", "ISO-10303-21;"),
        ("a header section", "\nHEADER;\n"),
        ("a file description", "FILE_DESCRIPTION("),
        ("a file name record", "FILE_NAME("),
        ("a schema declaration", "FILE_SCHEMA("),
        ("a data section", "\nDATA;\n"),
        ("the part 21 epilogue", "END-ISO-10303-21;"),
    ] {
        if !text.contains(needle) {
            out.push(format!("the file has no {what} ({needle:?} is missing)"));
        }
    }
    if text.matches("ENDSEC;").count() < 2 {
        out.push("the file does not close both of its sections with ENDSEC;".into());
    }
}

fn data_section(text: &str) -> Option<&str> {
    let start = text.find("\nDATA;\n")? + "\nDATA;\n".len();
    let rest = &text[start..];
    let end = rest.find("\nENDSEC;")?;
    Some(&rest[..end])
}

/// Split a data section into instances, respecting string literals so a `;`
/// or a `#` inside a name does not confuse the scan.
fn parse_data(text: &str, out: &mut Vec<String>) -> Option<Entities> {
    let data = match data_section(text) {
        Some(d) => d,
        None => {
            out.push("the data section is not delimited by DATA; ... ENDSEC;".into());
            return None;
        }
    };
    let mut entities: Entities = BTreeMap::new();
    for instance in split_instances(data) {
        let Some((head, body)) = instance.split_once('=') else {
            out.push(format!("instance {instance:?} is not `#n = ...`"));
            continue;
        };
        let head = head.trim();
        let Some(id) = head.strip_prefix('#').and_then(|d| d.trim().parse::<usize>().ok()) else {
            out.push(format!("instance {head:?} does not start with an entity id"));
            continue;
        };
        let body = body.trim().to_owned();
        let e = Entity { id, types: type_names(&body), body: body.clone(), refs: refs_in(&body) };
        if let Some(prev) = entities.insert(id, e) {
            out.push(format!("entity #{id} is defined twice (first as {})", prev.types.join("+")));
        }
    }
    if entities.is_empty() {
        out.push("the data section has no entity instances".into());
        return None;
    }
    Some(entities)
}

fn split_instances(data: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_string = false;
    let mut chars = data.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            cur.push(c);
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    cur.push(chars.next().unwrap());
                } else {
                    in_string = false;
                }
            }
            continue;
        }
        match c {
            '\'' => {
                in_string = true;
                cur.push(c);
            }
            ';' => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_owned());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_owned());
    }
    out
}

/// The entity names in an instance body, outside string literals.
fn type_names(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut ident = String::new();
    let mut in_string = false;
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    in_string = false;
                }
            }
            continue;
        }
        if c == '\'' {
            in_string = true;
            ident.clear();
            continue;
        }
        if c.is_ascii_alphanumeric() || c == '_' {
            ident.push(c);
            continue;
        }
        if c == '(' && !ident.is_empty() && ident.chars().next().is_some_and(|f| f.is_ascii_alphabetic()) {
            out.push(ident.clone());
        }
        ident.clear();
    }
    out
}

/// Every `#n` in an instance body, outside string literals.
fn refs_in(body: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut in_string = false;
    let bytes: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c == '\'' {
                if bytes.get(i + 1) == Some(&'\'') {
                    i += 1;
                } else {
                    in_string = false;
                }
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            in_string = true;
            i += 1;
            continue;
        }
        if c == '#' {
            let mut j = i + 1;
            let mut n = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                n.push(bytes[j]);
                j += 1;
            }
            if let Ok(v) = n.parse::<usize>() {
                out.push(v);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// The trailing `.T.` / `.F.` of an instance, which is what an `ORIENTED_EDGE`
/// and an `ORIENTED_CLOSED_SHELL` carry last.
fn trailing_bool(body: &str) -> Option<bool> {
    let t = body.trim_end().trim_end_matches(')').trim_end();
    if t.ends_with(".T.") {
        Some(true)
    } else if t.ends_with(".F.") {
        Some(false)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

fn check_references(entities: &Entities, out: &mut Vec<String>) {
    for e in entities.values() {
        for r in &e.refs {
            if !entities.contains_key(r) {
                out.push(format!("#{} ({}) refers to #{r}, which is not defined", e.id, e.types.join("+")));
            }
            if *r == e.id {
                out.push(format!("#{} ({}) refers to itself", e.id, e.types.join("+")));
            }
        }
    }
}

fn check_acyclic(entities: &Entities, out: &mut Vec<String>) {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unseen,
        Open,
        Done,
    }
    let mut mark: HashMap<usize, Mark> = entities.keys().map(|k| (*k, Mark::Unseen)).collect();
    // Iterative depth-first search: a 100 k-entity file must not blow the
    // stack proving it has no cycles.
    for root in entities.keys() {
        if mark[root] != Mark::Unseen {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(*root, 0)];
        mark.insert(*root, Mark::Open);
        while let Some((node, at)) = stack.pop() {
            let Some(e) = entities.get(&node) else { continue };
            if at < e.refs.len() {
                stack.push((node, at + 1));
                let next = e.refs[at];
                match mark.get(&next).copied() {
                    Some(Mark::Unseen) => {
                        mark.insert(next, Mark::Open);
                        stack.push((next, 0));
                    }
                    Some(Mark::Open) => {
                        out.push(format!("#{node} and #{next} are part of a reference cycle"));
                        mark.insert(next, Mark::Done);
                    }
                    _ => {}
                }
            } else {
                mark.insert(node, Mark::Done);
            }
        }
    }
}

/// The three entity types AP214 leaves with nothing pointing at them.
const EXPECTED_ROOTS: [&str; 3] =
    ["APPLICATION_PROTOCOL_DEFINITION", "SHAPE_DEFINITION_REPRESENTATION", "PRODUCT_RELATED_PRODUCT_CATEGORY"];

fn check_orphans(entities: &Entities, out: &mut Vec<String>) {
    let referenced: BTreeSet<usize> = entities.values().flat_map(|e| e.refs.iter().copied()).collect();
    for e in entities.values() {
        if referenced.contains(&e.id) {
            continue;
        }
        if EXPECTED_ROOTS.iter().any(|r| e.is(r)) {
            continue;
        }
        out.push(format!("#{} ({}) is an orphan: nothing refers to it", e.id, e.types.join("+")));
    }
}

fn check_faces(entities: &Entities, out: &mut Vec<String>) {
    for f in entities.values().filter(|e| e.is("ADVANCED_FACE")) {
        if f.refs.len() < 2 {
            out.push(format!("#{} is an ADVANCED_FACE with no boundary loop", f.id));
            continue;
        }
        let bounds = &f.refs[..f.refs.len() - 1];
        let outer = bounds
            .iter()
            .filter(|b| entities.get(b).is_some_and(|e| e.is("FACE_OUTER_BOUND")))
            .count();
        if outer > 1 {
            out.push(format!("#{} is an ADVANCED_FACE with {outer} FACE_OUTER_BOUNDs; AP214 allows one", f.id));
        }
        if trailing_bool(&f.body).is_none() {
            out.push(format!("#{} is an ADVANCED_FACE with no same_sense flag", f.id));
        }
    }
}

/// Every edge of a closed shell must be walked exactly twice, once each way.
fn check_shells(entities: &Entities, out: &mut Vec<String>) {
    for shell in entities.values().filter(|e| e.is("CLOSED_SHELL") || e.is("OPEN_SHELL")) {
        let mut uses: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        for face_id in &shell.refs {
            let Some(face) = entities.get(face_id) else { continue };
            if face.refs.is_empty() {
                continue;
            }
            for bound_id in &face.refs[..face.refs.len() - 1] {
                let Some(bound) = entities.get(bound_id) else { continue };
                let Some(loop_id) = bound.refs.first() else { continue };
                let Some(edge_loop) = entities.get(loop_id) else { continue };
                for oe_id in &edge_loop.refs {
                    let Some(oe) = entities.get(oe_id) else { continue };
                    let Some(edge) = oe.refs.first() else { continue };
                    let slot = uses.entry(*edge).or_insert((0, 0));
                    match trailing_bool(&oe.body) {
                        Some(true) => slot.0 += 1,
                        Some(false) => slot.1 += 1,
                        None => out.push(format!("#{} is an ORIENTED_EDGE with no orientation flag", oe.id)),
                    }
                }
            }
        }
        if !shell.is("CLOSED_SHELL") {
            continue;
        }
        for (edge, (fwd, rev)) in uses {
            if (fwd, rev) != (1, 1) {
                out.push(format!(
                    "shell #{} uses edge #{edge} {fwd} time(s) forward and {rev} backward; a closed shell uses \
                     every edge exactly once each way",
                    shell.id
                ));
            }
        }
    }
}

fn check_voids(entities: &Entities, out: &mut Vec<String>) {
    for b in entities.values().filter(|e| e.is("BREP_WITH_VOIDS")) {
        if b.refs.len() < 2 {
            out.push(format!("#{} is a BREP_WITH_VOIDS with no void", b.id));
            continue;
        }
        for v in &b.refs[1..] {
            let Some(oriented) = entities.get(v) else { continue };
            if !oriented.is("ORIENTED_CLOSED_SHELL") {
                out.push(format!("#{} names #{v} as a void, but it is a {}", b.id, oriented.types.join("+")));
                continue;
            }
            if trailing_bool(&oriented.body) != Some(false) {
                out.push(format!(
                    "#{v} is a void of #{} oriented .T.; ISO 10303-42 brep_with_voids WR1 requires .F.",
                    b.id
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('x'), '2;1');\nFILE_NAME('a','b',(''),(''),'c','d','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n#1 = APPLICATION_PROTOCOL_DEFINITION('a', 'b', 1994, #2);\n#2 = APPLICATION_CONTEXT('c');\nENDSEC;\nEND-ISO-10303-21;\n";

    #[test]
    fn a_minimal_well_formed_file_passes() {
        assert!(validate(GOOD).is_empty(), "{:?}", validate(GOOD));
    }

    #[test]
    fn a_dangling_reference_is_reported_with_both_ids() {
        let bad = GOOD.replace("#2);", "#9);");
        let p = validate(&bad);
        assert!(p.iter().any(|m| m.contains("#9") && m.contains("not defined")), "{p:?}");
    }

    #[test]
    fn a_duplicated_id_is_reported() {
        let bad = GOOD.replace("#2 = APPLICATION_CONTEXT('c');", "#1 = APPLICATION_CONTEXT('c');\n#2 = APPLICATION_CONTEXT('d');");
        let p = validate(&bad);
        assert!(p.iter().any(|m| m.contains("defined twice")), "{p:?}");
    }

    #[test]
    fn a_cycle_is_reported() {
        let bad = GOOD.replace("#2 = APPLICATION_CONTEXT('c');", "#2 = APPLICATION_CONTEXT_X('c', #1);");
        let p = validate(&bad);
        assert!(p.iter().any(|m| m.contains("cycle")), "{p:?}");
    }

    #[test]
    fn an_orphan_is_reported() {
        let bad = GOOD.replace("ENDSEC;\nEND-ISO", "#3 = CARTESIAN_POINT('', (0.0, 0.0, 0.0));\nENDSEC;\nEND-ISO");
        let p = validate(&bad);
        assert!(p.iter().any(|m| m.contains("orphan")), "{p:?}");
    }

    #[test]
    fn a_semicolon_inside_a_string_does_not_split_an_instance() {
        let tricky = GOOD.replace("APPLICATION_CONTEXT('c')", "APPLICATION_CONTEXT('a; b ''quoted'' #7')");
        let p = validate(&tricky);
        assert!(p.is_empty(), "{p:?}");
        let ents = parse_data(&tricky, &mut Vec::new()).unwrap();
        assert!(ents[&2].refs.is_empty(), "a #7 inside a string is not a reference");
    }

    #[test]
    fn complex_instances_report_every_sub_supertype() {
        let names = type_names("( BOUNDED_CURVE() B_SPLINE_CURVE(2, (#1)) REPRESENTATION_ITEM('') )");
        assert_eq!(names, vec!["BOUNDED_CURVE", "B_SPLINE_CURVE", "REPRESENTATION_ITEM"]);
    }

    #[test]
    fn trailing_booleans_are_read_off_the_end() {
        assert_eq!(trailing_bool("ORIENTED_EDGE('', *, *, #4, .T.)"), Some(true));
        assert_eq!(trailing_bool("ORIENTED_EDGE('', *, *, #4, .F.)"), Some(false));
        assert_eq!(trailing_bool("CARTESIAN_POINT('', (0.0))"), None);
    }
}
