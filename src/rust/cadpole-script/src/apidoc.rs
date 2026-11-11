//! Machine-readable API documentation, derived from the Starlark globals'
//! doc comments so it can never drift from the implementation.

use cadpole_proto::v1 as pb;
use starlark::docs::{DocItem, DocMember, DocModule, DocParam, DocType};

fn doc_string(d: &Option<starlark::docs::DocString>) -> String {
    match d {
        Some(s) => match &s.details {
            Some(det) => format!("{}\n\n{}", s.summary, det),
            None => s.summary.clone(),
        },
        None => String::new(),
    }
}

fn param(p: &DocParam, required: bool) -> pb::ApiParam {
    pb::ApiParam {
        name: p.name.clone(),
        r#type: p.typ.to_string(),
        doc: doc_string(&p.docs),
        required: required && p.default_value.is_none(),
        default_value: p.default_value.clone().unwrap_or_default(),
    }
}

/// A bare separator in a signature, Python's notation: `/` ends the
/// positional-only parameters, `*` starts the keyword-only ones. It is
/// `required` with no type and no default so that every renderer (`cadpole
/// doc`, the web reference, an assistant reading `--json`) prints it as it
/// stands, which is what tells `cylinder(r=..., h=..., /, *, d=...)` (call it
/// `cylinder(5, 10)`) apart from a function whose `r` really is a keyword.
fn separator(mark: &str) -> pb::ApiParam {
    let doc = match mark {
        "/" => "the parameters before this mark are positional-only: pass them without their names",
        _ => "the parameters after this mark are keyword-only: pass them as name=value",
    };
    pb::ApiParam { name: mark.to_owned(), required: true, doc: doc.to_owned(), ..Default::default() }
}

fn function(name: &str, f: &starlark::docs::DocFunction, on_type: &str) -> pb::ApiFunction {
    let mut params = Vec::new();
    for p in &f.params.pos_only {
        params.push(param(p, true));
    }
    if !f.params.pos_only.is_empty() {
        params.push(separator("/"));
    }
    for p in &f.params.pos_or_named {
        params.push(param(p, true));
    }
    if let Some(p) = &f.params.args {
        let mut a = param(p, false);
        a.name = format!("*{}", a.name);
        params.push(a);
    } else if !f.params.named_only.is_empty() {
        params.push(separator("*"));
    }
    for p in &f.params.named_only {
        params.push(param(p, true));
    }
    if let Some(p) = &f.params.kwargs {
        let mut a = param(p, false);
        a.name = format!("**{}", a.name);
        params.push(a);
    }
    pb::ApiFunction {
        name: name.to_owned(),
        doc: doc_string(&f.docs),
        params,
        returns: f.ret.typ.to_string(),
        on_type: on_type.to_owned(),
        constant: false,
    }
}

fn type_doc(name: &str, t: &DocType) -> pb::ApiType {
    let mut methods = Vec::new();
    let mut attrs = Vec::new();
    for (mname, member) in &t.members {
        match member {
            DocMember::Function(f) => methods.push(function(mname, f, name)),
            DocMember::Property(p) => attrs.push(pb::ApiParam {
                name: mname.clone(),
                r#type: p.typ.to_string(),
                doc: doc_string(&p.docs),
                required: false,
                default_value: String::new(),
            }),
        }
    }
    pb::ApiType { name: name.to_owned(), doc: doc_string(&t.docs), methods, attrs }
}

/// `prefix` is the namespace path a member sits under, so `field.sphere`
/// documents as `field.sphere` rather than colliding with a global of the
/// same short name (`field.box` versus `box`).
fn walk(module: &DocModule, prefix: &str, out: &mut pb::ApiDocResponse) {
    for (name, item) in &module.members {
        let full = if prefix.is_empty() { name.clone() } else { format!("{prefix}.{name}") };
        match item {
            DocItem::Member(DocMember::Function(f)) => out.functions.push(function(&full, f, "")),
            DocItem::Member(DocMember::Property(p)) => out.functions.push(pb::ApiFunction {
                name: full,
                doc: doc_string(&p.docs),
                params: vec![],
                returns: p.typ.to_string(),
                on_type: String::new(),
                // `math.pi`, `XY`: a value to read, not a function to call.
                constant: true,
            }),
            DocItem::Type(t) => {
                out.types.push(type_doc(&full, t));
                // A callable vocabulary is a type *and* a function:
                // `cadpole doc emboss` lists the parameters too.
                if let Some(f) = &t.constructor {
                    out.functions.push(function(&full, f, ""));
                }
            }
            DocItem::Module(m) => walk(m, &full, out),
        }
    }
}

/// The Cadpole API (our globals only, not the Starlark standard library) plus
/// the methods of the value types.
pub fn api_doc() -> pb::ApiDocResponse {
    use starlark::environment::GlobalsBuilder;
    // Exactly what `globals()` installs, minus the Starlark standard library:
    // a namespace missing from here is a namespace assistants are told does
    // not exist. `every_global_is_documented` below is what
    // keeps the two lists together.
    let ours = GlobalsBuilder::new()
        .with(crate::globals::cadpole_globals)
        .with(crate::animation::animation_globals)
        .with(crate::assembly::assembly_globals)
        .with(crate::chamfer::chamfer_globals)
        .with(crate::emboss::emboss_globals)
        .with(crate::fillet::fillet_globals)
        .with(crate::field::field_globals)
        .with(crate::import2d::import2d_globals)
        .with(crate::import3d::import_globals)
        .with(crate::layers::layer_globals)
        .with(crate::vocab::vocab_globals)
        .with(crate::primitives::primitive_globals)
        .with(crate::sketch2d::sketch2d_globals)
        .with(crate::pattern::pattern_globals)
        .with(crate::sun::sun_globals)
        .with(crate::sightline::sightline_globals)
        .with(crate::sweep::sweep_globals)
        .with(crate::terrain::terrain_globals)
        .with(crate::sheets::sheet_globals)
        .with(crate::fmtspec::format_globals)
        .with(crate::units::units_globals)
        .with_namespace("field", crate::field::field_namespace)
        .with_namespace("math", crate::mathns::math_namespace)
        .with_namespace("opt", crate::opt::opt_namespace)
        .build();
    let mut out = pb::ApiDocResponse::default();
    walk(&ours.documentation(), "", &mut out);
    out.functions.sort_by(|a, b| a.name.cmp(&b.name));

    // Value types: ask each type for its methods.
    let types: Vec<(&str, Option<&'static starlark::environment::Methods>)> = vec![
        ("Plane", <crate::values::KPlane as starlark::values::StarlarkValue>::get_methods()),
        ("Sketch", <crate::values::KSketch as starlark::values::StarlarkValue>::get_methods()),
        (
            "ConstrainedSketch",
            <crate::constrained::KConstrained as starlark::values::StarlarkValue>::get_methods(),
        ),
        (
            "SketchEntity",
            <crate::constrained::KEntity as starlark::values::StarlarkValue>::get_methods(),
        ),
        ("Body", <crate::values::KBody as starlark::values::StarlarkValue>::get_methods()),
        (
            "Selection",
            <crate::selection::KSelection as starlark::values::StarlarkValue>::get_methods(),
        ),
        ("Face", <crate::values::KFace as starlark::values::StarlarkValue>::get_methods()),
        ("Edge", <crate::values::KEdge as starlark::values::StarlarkValue>::get_methods()),
        ("Field", <crate::field::KField as starlark::values::StarlarkValue>::get_methods()),
        ("Ref", <crate::values::KRef as starlark::values::StarlarkValue>::get_methods()),
        ("Drawing", <crate::drawing::KDrawing as starlark::values::StarlarkValue>::get_methods()),
        // Types a script can hold and call but the reference did not mention:
        // `path(...)` returns a Path and the docs describe its 9 methods,
        // `asset(...)` an Asset, `printer(...)` a Printer, and
        // `print_check(...)` a PrintReport.
        ("Path", <crate::values::KPath as starlark::values::StarlarkValue>::get_methods()),
        ("Curve", <crate::values::KCurve as starlark::values::StarlarkValue>::get_methods()),
        // `terrain(...)` returns a Terrain.
        ("Terrain", <crate::terrain::KTerrain as starlark::values::StarlarkValue>::get_methods()),
        // `wrap(...)` returns a Wrapped.
        ("Wrapped", <crate::emboss::KWrapped as starlark::values::StarlarkValue>::get_methods()),
        ("Asset", <crate::values::KAsset as starlark::values::StarlarkValue>::get_methods()),
        ("Printer", <crate::values::KPrinter as starlark::values::StarlarkValue>::get_methods()),
        ("PrintReport", <crate::values::KPrintReport as starlark::values::StarlarkValue>::get_methods()),
        // `import_report(...)`.
        ("ImportReport", <crate::values::KImportReport as starlark::values::StarlarkValue>::get_methods()),
        // `import_dxf(...)` returns a Drawing2D.
        ("Drawing2D", <crate::import2d::KDrawing2D as starlark::values::StarlarkValue>::get_methods()),
        // Display assemblies.
        ("Part", <crate::assembly::KPart as starlark::values::StarlarkValue>::get_methods()),
        ("Instance", <crate::assembly::KInstance as starlark::values::StarlarkValue>::get_methods()),
        ("Assembly", <crate::assembly::KAssembly as starlark::values::StarlarkValue>::get_methods()),
        ("Frame", <crate::assembly::KFrame as starlark::values::StarlarkValue>::get_methods()),
        // Animations: `keyframe(...)` and `view(...)` return these.
        ("Keyframe", <crate::animation::KKeyframe as starlark::values::StarlarkValue>::get_methods()),
        ("View", <crate::animation::KView as starlark::values::StarlarkValue>::get_methods()),
        // The sun: `sun(...)` returns one.
        ("Sun", <crate::sun::KSun as starlark::values::StarlarkValue>::get_methods()),
        // Shadow studies and sightlines.
        ("Shadow", <crate::sightline::KShadow as starlark::values::StarlarkValue>::get_methods()),
        ("ShadowStudy", <crate::sightline::KShadowStudy as starlark::values::StarlarkValue>::get_methods()),
        ("Sightline", <crate::sightline::KSightline as starlark::values::StarlarkValue>::get_methods()),
        ("SightlineFan", <crate::sightline::KFan as starlark::values::StarlarkValue>::get_methods()),
        // Display layers: `layer(...)` returns one.
        ("Layer", <crate::layers::KLayer as starlark::values::StarlarkValue>::get_methods()),
        // Sheet sets: `sheet_set(...)`, its sheets and their views.
        ("SheetSet", <crate::sheets::KSheetSet as starlark::values::StarlarkValue>::get_methods()),
        ("Sheet", <crate::sheets::KSheet as starlark::values::StarlarkValue>::get_methods()),
        ("SheetView", <crate::sheets::KSheetView as starlark::values::StarlarkValue>::get_methods()),
        ("TitleBlock", <crate::sheets::KTitleBlock as starlark::values::StarlarkValue>::get_methods()),
        // Linear and mixed-integer programs: `opt.problem(...)`, its
        // variables, the expressions they make and what `solve()` returns.
        ("OptProblem", <crate::opt::KProblem as starlark::values::StarlarkValue>::get_methods()),
        ("OptVar", <crate::opt::KVar as starlark::values::StarlarkValue>::get_methods()),
        ("OptExpr", <crate::opt::KExpr as starlark::values::StarlarkValue>::get_methods()),
        ("OptSolution", <crate::opt::KSolution as starlark::values::StarlarkValue>::get_methods()),
    ];
    for (name, methods) in types {
        let mut t = pb::ApiType { name: name.to_owned(), ..Default::default() };
        if let Some(m) = methods {
            let d = m.documentation(starlark::typing::Ty::any());
            let full = type_doc(name, &d);
            t.methods = full.methods;
            t.attrs = full.attrs;
        }
        out.types.push(t);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn api_doc_lists_globals_and_methods() {
        let d = super::api_doc();
        let names: Vec<_> = d.functions.iter().map(|f| f.name.as_str()).collect();
        for expected in [
            "param", "sketch", "extrude", "revolve", "union", "subtract", "box", "export",
            "selection", "mesh", "fillet", "chamfer", "field.sphere", "field.box",
            "field.from_body", "imprint", "emboss", "wrap",
        ] {
            assert!(names.contains(&expected), "missing {expected} in {names:?}");
        }
        // The callable vocabularies document their parameters as functions
        // and their members as types.
        let emboss = d.functions.iter().find(|f| f.name == "emboss").unwrap();
        assert_eq!(
            emboss.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["body", "face", "art", "/", "height", "*", "kind", "draft", "wrap", "origin", "up", "width", "color", "name", "layer"]
        );
        let wrap = d.functions.iter().find(|f| f.name == "wrap").unwrap();
        assert!(wrap.doc.contains("developed"), "{}", wrap.doc);
        let extrude = d.functions.iter().find(|f| f.name == "extrude").unwrap();
        assert!(extrude.doc.contains("Extrude"));
        // `layer=` is on every body-producing builtin that takes `name=`.
        assert_eq!(extrude.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["sketch", "distance", "/", "*", "name", "layer"]);
        let sketch = d.types.iter().find(|t| t.name == "Sketch").unwrap();
        let m: Vec<_> = sketch.methods.iter().map(|m| m.name.as_str()).collect();
        assert!(m.contains(&"rect") && m.contains(&"circle"), "{m:?}");
        assert!(names.contains(&"constrained_sketch"), "{names:?}");
        let cs = d.types.iter().find(|t| t.name == "ConstrainedSketch").unwrap();
        let m: Vec<_> = cs.methods.iter().map(|m| m.name.as_str()).collect();
        for expected in ["point", "line", "arc", "circle", "constrain", "close", "dof", "at"] {
            assert!(m.contains(&expected), "missing {expected} in {m:?}");
        }
        let constrain = cs.methods.iter().find(|m| m.name == "constrain").unwrap();
        assert_eq!(
            constrain.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["kind", "a", "b", "c", "/", "*", "name"]
        );
        // The `field` namespace documents under its own path, so `field.box`
        // and the global `box` are two distinct entries.
        let field = d.types.iter().find(|t| t.name == "Field").unwrap();
        let fm: Vec<_> = field.methods.iter().map(|m| m.name.as_str()).collect();
        assert!(fm.contains(&"smooth_union") && fm.contains(&"shell"), "{fm:?}");
        let fillet = d.functions.iter().find(|f| f.name == "fillet").unwrap();
        assert!(fillet.doc.contains("mesh body"), "fillet says what it returns: {}", fillet.doc);
    }

    /// The reference is what CLAUDE.md, docs/starlark-api.md and the MCP tool
    /// description send an assistant to read before writing a script, so a
    /// name it omits is a name that does not exist as far as the reader is
    /// concerned. The hand-picked list above cannot catch drift — the whole
    /// `math` namespace was missing from `cadpole doc` while evaluating fine —
    /// so this compares the document against the globals themselves.
    #[test]
    fn every_global_is_documented() {
        let d = super::api_doc();
        let documented: std::collections::HashSet<&str> = d.functions.iter().map(|f| f.name.as_str()).collect();
        // The Starlark standard library documents itself elsewhere; what has
        // to be here is Cadpole's own surface.
        let standard: std::collections::HashSet<String> = starlark::environment::Globals::standard()
            .documentation()
            .members
            .keys()
            .map(|k| k.to_string())
            .collect();
        let mut missing: Vec<String> = Vec::new();
        for (name, item) in &crate::globals().documentation().members {
            if standard.contains(name.as_str()) || name == "print" {
                continue;
            }
            match item {
                // A namespace: every member documents under `ns.member`.
                starlark::docs::DocItem::Module(m) => {
                    for member in m.members.keys() {
                        let full = format!("{name}.{member}");
                        if !documented.contains(full.as_str()) {
                            missing.push(full);
                        }
                    }
                }
                // A global that is a type — a vocabulary — documents as one
                // entry under Types, its members as the type's attributes.
                starlark::docs::DocItem::Type(t) => match d.types.iter().find(|ty| ty.name == *name) {
                    None => missing.push(name.to_string()),
                    Some(ty) => {
                        for member in t.members.keys() {
                            let listed = ty.attrs.iter().any(|a| a.name == *member)
                                || ty.methods.iter().any(|m| m.name == *member);
                            if !listed {
                                missing.push(format!("{name}.{member}"));
                            }
                        }
                    }
                },
                _ => {
                    if !documented.contains(name.as_str()) {
                        missing.push(name.to_string());
                    }
                }
            }
        }
        assert!(missing.is_empty(), "these globals are not in `cadpole doc`: {missing:?}");
        // The namespaces really are in there, under their own paths.
        for expected in ["math.sqrt", "math.pi", "math.clamp", "field.sphere"] {
            assert!(documented.contains(expected), "missing {expected}");
        }
    }

    /// The vocabularies are how an assistant learns the closed
    /// spellings without guessing, so each is a type in `cadpole doc` whose
    /// attributes are exactly its members and whose summary names the
    /// keyword it feeds and lists the members again.
    #[test]
    fn every_vocabulary_is_documented_with_its_members() {
        let d = super::api_doc();
        for v in crate::vocab::all() {
            let t = d.types.iter().find(|t| t.name == v.name).unwrap_or_else(|| panic!("no type {} in cadpole doc", v.name));
            let attrs: Vec<&str> = t.attrs.iter().map(|a| a.name.as_str()).collect();
            let mut members = v.members.clone();
            members.sort();
            let mut sorted = attrs.clone();
            sorted.sort();
            assert_eq!(sorted, members, "{}'s attrs are its members", v.name);
            assert!(t.methods.is_empty(), "{} has no methods", v.name);
            assert!(t.doc.contains(&format!("`{}=`", v.keyword)), "{}: {}", v.name, t.doc);
            assert!(t.doc.contains(&v.members.join(", ")), "{}: {}", v.name, t.doc);
            for a in &t.attrs {
                assert_eq!(a.r#type, "str", "{}.{} is a str", v.name, a.name);
                assert!(a.doc.contains(&format!("{:?}", a.name)), "{}.{}: {}", v.name, a.name, a.doc);
            }
        }
    }

    /// Every value type a script can get hold of has an entry, so `cadpole doc`
    /// can be read as the whole API rather than most of it.
    #[test]
    fn every_value_type_is_documented() {
        let d = super::api_doc();
        let names: Vec<&str> = d.types.iter().map(|t| t.name.as_str()).collect();
        for expected in [
            "Plane", "Sketch", "ConstrainedSketch", "SketchEntity", "Body", "Selection", "Face", "Edge",
            "Field", "Ref", "Drawing", "Path", "Curve", "Asset", "Printer", "PrintReport", "Wrapped", "ImportReport", "Layer",
            "SheetSet", "Sheet", "SheetView", "TitleBlock", "Drawing2D",
        ] {
            assert!(names.contains(&expected), "missing type {expected} in {names:?}");
        }
        // A sheet's views and its annotation are all listed.
        let sheet = d.types.iter().find(|t| t.name == "Sheet").unwrap();
        let m: Vec<&str> = sheet.methods.iter().map(|m| m.name.as_str()).collect();
        for expected in [
            "plan_view", "section_view", "elevation_view", "axon_view", "image_view", "text", "note", "table", "dimension",
            "north_arrow", "scale_bar", "legend", "hatch", "fill", "polyline", "label", "view_title",
        ] {
            assert!(m.contains(&expected), "Sheet.{expected} missing from {m:?}");
        }
        let names: Vec<&str> = d.functions.iter().map(|f| f.name.as_str()).collect();
        for expected in ["sheet_set", "title_block", "report", "sweep_table", "text_width", "format"] {
            assert!(names.contains(&expected), "{expected} is not in cadpole doc");
        }
        let wrapped = d.types.iter().find(|t| t.name == "Wrapped").unwrap();
        assert!(wrapped.methods.iter().any(|m| m.name == "curves"), "{:?}", wrapped.methods);
        let path = d.types.iter().find(|t| t.name == "Path").unwrap();
        let m: Vec<&str> = path.methods.iter().map(|m| m.name.as_str()).collect();
        for expected in ["line", "arc", "bezier", "spline", "tangent_arc", "on"] {
            assert!(m.contains(&expected), "Path.{expected} missing from {m:?}");
        }
        // `curve()` has `path()`'s vocabulary in space, plus the two things
        // only a section needs: closing, and running the other way.
        let curve = d.types.iter().find(|t| t.name == "Curve").unwrap();
        let m: Vec<&str> = curve.methods.iter().map(|m| m.name.as_str()).collect();
        for expected in ["line", "line_by", "arc", "bezier", "spline", "close", "reverse"] {
            assert!(m.contains(&expected), "Curve.{expected} missing from {m:?}");
        }
    }

    fn params_of(d: &cadpole_proto::v1::ApiDocResponse, name: &str) -> Vec<String> {
        let f = d.functions.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no {name}"));
        f.params.iter().map(|p| p.name.clone()).collect()
    }

    /// A signature says how each parameter may be passed, in Python's
    /// notation: everything before `/` is positional-only, everything after
    /// `*` (or `*args`) keyword-only. Without the markers an optional
    /// positional-only parameter printed as `r=...`, the same as a keyword,
    /// so `cylinder(r=5, h=10)` — the call the reference showed — failed
    /// with "extra named parameter(s)"; `math.log(8, 2)` and
    /// `math.round(1.26, digits=1)` failed the same way the other round.
    #[test]
    fn signatures_mark_positional_only_and_keyword_only_parameters() {
        let d = super::api_doc();
        assert_eq!(params_of(&d, "cylinder"), ["r", "h", "/", "*", "d", "height", "at", "name", "layer"]);
        assert_eq!(params_of(&d, "math.log"), ["x", "/", "*", "base"]);
        assert_eq!(params_of(&d, "math.round"), ["x", "digits", "/"]);
        // `*args` already ends the positional run, so no bare `*` follows it.
        let union = params_of(&d, "union");
        assert!(union.iter().any(|p| p.starts_with('*') && p.len() > 1), "{union:?}");
        assert_eq!(union.iter().filter(|p| *p == "*").count(), 0, "{union:?}");
        // The markers are bare in a rendered signature: required, no type, no default.
        let cyl = d.functions.iter().find(|f| f.name == "cylinder").unwrap();
        for m in cyl.params.iter().filter(|p| p.name == "/" || p.name == "*") {
            assert!(m.required && m.default_value.is_empty() && m.r#type.is_empty(), "{m:?}");
        }
        // Methods get the same treatment.
        let body = d.types.iter().find(|t| t.name == "Body").unwrap();
        let face = body.methods.iter().find(|m| m.name == "face").unwrap();
        assert!(face.params.iter().any(|p| p.name == "/"), "{:?}", face.params);
    }

    /// A module-level value (`math.pi`, `XY`) is a constant, not a function:
    /// listing it as `math.pi() -> float` told readers to call it, and
    /// calling a float is an error.
    #[test]
    fn constants_are_not_documented_as_functions() {
        let d = super::api_doc();
        for name in ["math.pi", "math.e", "math.tau", "XY", "XZ", "YZ"] {
            let f = d.functions.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no {name}"));
            assert!(f.constant, "{name} is a constant");
            assert!(f.params.is_empty(), "{name}");
        }
        assert_eq!(d.functions.iter().find(|f| f.name == "math.pi").unwrap().returns, "float");
        for name in ["cylinder", "math.sqrt", "sketch"] {
            assert!(!d.functions.iter().find(|f| f.name == name).unwrap().constant, "{name} is callable");
        }
    }
}
