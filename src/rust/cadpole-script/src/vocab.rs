//! Vocabularies: the closed sets of strings a keyword argument accepts, each
//! exported as a frozen namespace named after the keyword it feeds.
//!
//! `faces(kind="cylinder")`, `loft(match="arclen")`, `union(kernel="mesh")`,
//! `material(preset="brass")`, `test(monotone="increasing")`: every one of
//! those strings is drawn from a closed list. A typo is a diagnostic from
//! inside the call (one that names the valid values), but a namespace
//! also catches it at the site, can complete it, and tells an assistant
//! writing a script the spellings. `kind`, `match`, `cap`, `kernel`, `preset`,
//! `monotone`, `hand` and `join` are globals, and **the attribute is the string**:
//! `kind.cylinder` evaluates to `"cylinder"`, so `kind.cylinder == "cylinder"`
//! is `True`, every existing script keeps working, the functions keep taking
//! the string, and nothing in the proto, the JSON views or the diagnostics
//! changes. What a namespace adds is checking where the name is written:
//! `kind.cylnder` is an error on that line, listing the members.
//!
//! **One source of truth.** Each [`Vocabulary`] is built from the same Rust
//! list its parser reads — `SectionMatch::ALL`, `Cap::ALL`,
//! `Fit::ALL`, `KernelChoice::ALL`, `Monotone::ALL`, `material_preset_names()`, and the
//! `FACE_KINDS`/`EDGE_KINDS` the selectors check — so the namespace cannot
//! offer a spelling the parser refuses or miss one it accepts. The tests at
//! the bottom hold that in both directions. `hand` is the one whose parser
//! is Starlark (`@cadpole/gears.star`): its list is `stdlib::HANDS`, and
//! `stdlib`'s tests hold the library to it.
//!
//! `join` is `Sketch.offset(join=)`'s, from `Join::ALL`. `flavor` is
//! `export(flavor=)`'s, from `cadpole_core::export::Flavor::ALL`.
//!
//! **Callable vocabularies**. `emboss` and `wrap` are the values of
//! `emboss(kind=)` and `wrap(method=)` *and* the functions: `emboss.raised`
//! is `"raised"` (`raise` is Starlark's reserved word, as `in` is), and
//! `emboss(plate, "plate/top", label, 1, kind=emboss.raised)` is the call. A [`Vocabulary`] with a `target` — the
//! native function, fetched from `crate::emboss`'s own `Globals` — forwards
//! `invoke` to it, documents itself as a type with a constructor, and is
//! otherwise the same value as the rest. The namespace is named after the
//! function rather than the keyword because `kind` is already the face
//! class vocabulary and one name must not carry a second meaning.
//!
//! What is deliberately *not* a vocabulary: the render `backend`/`edges`/
//! `grid` modes (CLI and rpc options; no script passes them) and
//! entity-reference kinds (a script never spells those). `planar` stays an accepted alias of
//! `kind.plane` in the selector but is not a member: a namespace lists each
//! thing once.

use std::cell::RefCell;
use std::fmt;

use allocative::Allocative;
use cadpole_core::animation::Ease;
use cadpole_core::assembly::Along;
use cadpole_core::emboss::{Kind as EmbossKind, Method};
use cadpole_core::export::Flavor;
use cadpole_core::kernel::KernelChoice;
use cadpole_core::loft::{Cap, SectionMatch};
use cadpole_core::skin::Fit;
use cadpole_core::render::material_preset_names;
use cadpole_core::sketch2d::Join;
use cadpole_core::sweep::Frame;
use starlark::collections::SmallMap;
use starlark::docs::{DocItem, DocMember, DocProperty, DocString, DocStringKind, DocType};
use starlark::environment::GlobalsBuilder;
use starlark::starlark_simple_value;
use starlark::typing::Ty;
use starlark::eval::{Arguments, Evaluator};
use starlark::values::{
    FrozenValue, Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value, ValueError,
};
use starlark_derive::starlark_value;

use crate::modeltest::Monotone;
use crate::values::{EDGE_KINDS, FACE_KINDS};

/// One closed vocabulary: the global `name`, the keyword it feeds (the same
/// word, except for a callable one), and the accepted spellings in
/// documentation order.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct Vocabulary {
    /// The global's name: the keyword it feeds (`kind` for `kind=`), or the
    /// function it is (`emboss`).
    pub name: &'static str,
    /// The keyword the members feed: the name itself, or `kind` for `emboss`.
    pub keyword: &'static str,
    /// Where that keyword is taken, for `cadpole doc`: "faces() and edges()".
    pub feeds: &'static str,
    /// The members. `kind.cylinder` is `members[1]`, the string itself.
    #[allocative(skip)]
    pub members: Vec<&'static str>,
    /// The native function this vocabulary is, when it is one:
    /// `emboss(...)` calls it with the arguments as given.
    #[allocative(skip)]
    pub target: Option<FrozenValue>,
}
starlark_simple_value!(Vocabulary);

impl Vocabulary {
    fn new(name: &'static str, feeds: &'static str, members: Vec<&'static str>) -> Vocabulary {
        Vocabulary { name, keyword: name, feeds, members, target: None }
    }

    /// A vocabulary that is also the function of the same name: its members
    /// are the values of `keyword=` on that function.
    fn callable(name: &'static str, keyword: &'static str, feeds: &'static str, members: Vec<&'static str>) -> Vocabulary {
        Vocabulary { name, keyword, feeds, members, target: Some(crate::emboss::native(name)) }
    }

    pub fn is_callable(&self) -> bool {
        self.target.is_some()
    }
}

/// The face classes followed by the edge-only ones: what `faces(kind=)` and
/// `edges(kind=)` accept between them, each spelling once.
fn kinds() -> Vec<&'static str> {
    let mut all: Vec<&'static str> = FACE_KINDS.to_vec();
    all.extend(EDGE_KINDS.iter().filter(|k| !FACE_KINDS.contains(k)));
    all
}

/// Every vocabulary, in the order `docs/starlark-api.md` lists them.
/// Each member list comes from the Rust list the matching parser reads.
pub fn all() -> Vec<Vocabulary> {
    vec![
        Vocabulary::new("kind", "faces() and edges()", kinds()),
        Vocabulary::new("match", "loft()", SectionMatch::ALL.iter().map(|m| m.name()).collect()),
        Vocabulary::new("cap", "loft()", Cap::ALL.iter().map(|c| c.name()).collect()),
        Vocabulary::new("fit", "loft(ruled=False, fit=)", Fit::ALL.iter().map(|f| f.name()).collect()),
        // `sweep_frame`, not `frame`: `frame()` is the global that
        // reads a frame off named faces, and a vocabulary cannot be called.
        Vocabulary::new("sweep_frame", "sweep_along(frame=) and pattern_along(frame=)", Frame::ALL.iter().map(|f| f.name()).collect()),
        Vocabulary::new(
            "kernel",
            "union(), subtract(), intersect(), split() and the pattern_*() calls",
            KernelChoice::ALL.iter().map(|k| k.name()).collect(),
        ),
        Vocabulary::new("preset", "Body.material()", material_preset_names().to_vec()),
        Vocabulary::new("monotone", "test()", Monotone::ALL.iter().map(|m| m.as_str()).collect()),
        Vocabulary::new(
            "hand",
            "helical(), herringbone() and internal() in @cadpole/gears.star",
            crate::stdlib::HANDS.to_vec(),
        ),
        Vocabulary::new("join", "Sketch.offset()", Join::ALL.iter().map(|j| j.name()).collect()),
        Vocabulary::new("along", "explode()", Along::ALL.iter().map(|a| a.name()).collect()),
        Vocabulary::new("ease", "animation() and keyframe()", Ease::ALL.iter().map(|e| e.name()).collect()),
        Vocabulary::new("flavor", "export()", Flavor::ALL.iter().map(|f| f.name()).collect()),
        // The paper sizes, named after drawing()'s own `sheet=`.
        Vocabulary::new("sheet", "drawing(sheet=) and sheet_set(size=)", cadpole_drawing::SheetSize::NAMES.to_vec()),
        Vocabulary::new(
            "repair",
            "import_mesh()",
            cadpole_import::mesh::Repair::ALL.iter().map(|r| r.name()).collect(),
        ),
        // `dxf_units`, not `units`: `units=` already means a drawing unit
        // *format* on `drawing()` and the `export_*` calls (the `units`
        // vocabulary below) and a CSS-style unit on `import_svg`,
        // and one name must not carry a second meaning (as with
        // `sweep_frame`).
        // The same unit names say what a mesh file's coordinates
        // are in, `import_mesh(units=)` — one parser behind both.
        Vocabulary::new(
            "dxf_units",
            "import_dxf(units=) and import_mesh(units=)",
            cadpole_import::dxf::Unit::ALL.iter().map(|u| u.name()).collect(),
        ),
        // The callable ones: the namespace is the function.
        Vocabulary::callable("emboss", "kind", "emboss()", EmbossKind::ALL.iter().map(|k| k.name()).collect()),
        Vocabulary::callable(
            "wrap",
            "method",
            "wrap() — and as `wrap=` on imprint() and emboss()",
            Method::ALL.iter().map(|m| m.name()).collect(),
        ),
        // The two constructions of a terrain — the namespace is the
        // function, as `emboss` is.
        Vocabulary::callable(
            "terrain",
            "method",
            "terrain() and terrain_from_contours()",
            cadpole_core::terrain::Method::ALL.iter().map(|m| m.name()).collect(),
        ),
        // The document's display convention: `units(units.ft_in)`
        // declares it, and the same members feed `units=` on the drawing
        // and export calls.
        Vocabulary::callable(
            "units",
            "units",
            "drawing(), export_svg(), export_pdf() and export_dxf() — and units(...) declares the document's",
            cadpole_drawing::units::UNIT_MEMBERS.to_vec(),
        ),
    ]
}

/// Installs every vocabulary as a global. Called from `globals()` and from
/// `api_doc`, so what a script can name and what `cadpole doc` lists are the
/// same registry.
pub(crate) fn vocab_globals(builder: &mut GlobalsBuilder) {
    for v in all() {
        builder.set(v.name, v);
    }
}

/// The shape of a "not one of these" message: `<what>; valid: a, b, c`.
/// The `kind=` string parser and a `kind.<typo>` miss both go through it, so
/// the two ways of misspelling a class read the same.
pub(crate) fn not_one_of(what: impl fmt::Display, valid: &[&str]) -> String {
    format!("{what}; valid: {}", valid.join(", "))
}

thread_local! {
    /// The last member lookup that failed, `(vocabulary, attribute)`.
    ///
    /// starlark-rust's `get_attr` returns an `Option`, so a value cannot
    /// raise its own error for a missing attribute: the evaluator builds
    /// `ValueError::NoAttr` from the *type* name, which is `vocabulary` for
    /// all of them. This is how `evaluate` learns which one was meant. The miss
    /// is recorded immediately before the evaluator raises, on the thread
    /// that evaluates, and `missing_member_message` checks the attribute
    /// matches before trusting it.
    static LAST_MISS: RefCell<Option<(&'static str, String)>> = const { RefCell::new(None) };
}

/// When `e` is Starlark's "no attribute" error on a vocabulary, the message
/// Cadpole reports instead: which namespace, which spelling, and the members,
/// in the shape of the string parsers' diagnostic. `None` for any other error.
pub(crate) fn missing_member_message(e: &starlark::Error) -> Option<String> {
    let starlark::ErrorKind::Value(inner) = e.kind() else { return None };
    let (ty, attr, hint) = match inner.downcast_ref::<ValueError>()? {
        ValueError::NoAttr(ty, attr) => (ty, attr, None),
        ValueError::NoAttrDidYouMean(ty, attr, hint) => (ty, attr, Some(hint)),
        _ => return None,
    };
    if ty != Vocabulary::TYPE {
        return None;
    }
    let (name, missed) = LAST_MISS.with(|m| m.borrow_mut().take())?;
    if &missed != attr {
        return None;
    }
    let v = all().into_iter().find(|v| v.name == name)?;
    let hint = hint.map(|h| format!(" (did you mean {name}.{h}?)")).unwrap_or_default();
    Some(not_one_of(format!("{name}.{attr} is not a member of {name}{hint}"), &v.members))
}

impl fmt::Display for Vocabulary {
    /// `kind: plane | cylinder | ...` — what `str()` and `repr()` print.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.members.join(" | "))
    }
}

#[starlark_value(type = "vocabulary")]
impl<'v> StarlarkValue<'v> for Vocabulary {
    /// The member *is* the string: `kind.cylinder` is `"cylinder"`.
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match self.members.iter().find(|m| **m == attribute) {
            Some(m) => Some(heap.alloc(*m)),
            None => {
                LAST_MISS.with(|m| *m.borrow_mut() = Some((self.name, attribute.to_owned())));
                None
            }
        }
    }

    /// `hasattr(kind, "x")` asks without failing, so it must not record a miss.
    fn has_attr(&self, attribute: &str, _heap: Heap<'v>) -> bool {
        self.members.contains(&attribute)
    }

    fn dir_attr(&self) -> Vec<String> {
        self.members.iter().map(|m| m.to_string()).collect()
    }

    /// How `cadpole doc` sees a vocabulary: a type whose attributes are the
    /// members, each a `str`, with the keyword it feeds in the summary. The
    /// members are in the summary too, so a reader that prints only the
    /// summary still gets them.
    fn documentation(&self) -> DocItem {
        let mut summary = format!(
            "Vocabulary for `{keyword}=` on {feeds}: {list}. The attribute is the string \
             (`{name}.{first} == \"{first}\"`), so `{keyword}=\"{first}\"` and `{keyword}={name}.{first}` \
             are the same call and both forms are accepted; a misspelt member is an error at the \
             site that lists these. `dir({name})` lists them too.",
            name = self.name,
            keyword = self.keyword,
            feeds = self.feeds,
            list = self.members.join(", "),
            first = self.members.first().copied().unwrap_or_default(),
        );
        let constructor = self.target.and_then(|t| match t.to_value().documentation() {
            DocItem::Member(DocMember::Function(f)) => Some(f),
            _ => None,
        });
        if constructor.is_some() {
            let call = if self.keyword == self.name {
                format!("`{name}({name}.{first})`", name = self.name, first = self.members.first().copied().unwrap_or_default())
            } else {
                format!(
                    "`{name}(..., {keyword}={name}.{first})`",
                    name = self.name,
                    keyword = self.keyword,
                    first = self.members.first().copied().unwrap_or_default()
                )
            };
            summary.push_str(&format!(
                " `{name}` is also the function `{name}(...)` — the namespace is the function — so the \
                 call reads {call}.",
                name = self.name,
            ));
        }
        let mut members = SmallMap::new();
        for m in &self.members {
            let docs = DocString::from_docstring(DocStringKind::Rust, &format!("The string {m:?}."));
            members.insert(m.to_string(), DocMember::Property(DocProperty { docs, typ: Ty::string() }));
        }
        DocItem::Type(DocType {
            docs: DocString::from_docstring(DocStringKind::Rust, &summary),
            members,
            ty: Self::get_type_starlark_repr(),
            constructor,
        })
    }

    /// A callable vocabulary is its function: the call is handed
    /// to the native with the arguments as given, and with the call site,
    /// which `eval_function`'s own frame would otherwise hide from it.
    fn invoke(
        &self,
        _me: Value<'v>,
        args: &Arguments<'v, '_>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<Value<'v>> {
        let Some(target) = self.target else {
            return Err(starlark::Error::new_other(anyhow::anyhow!(
                "{name} is a vocabulary (the values of `{name}=`: {}), not a function",
                self.members.join(", "),
                name = self.name
            )));
        };
        let positional: Vec<Value<'v>> = args.positions(eval.heap())?.collect();
        let owned: Vec<(String, Value<'v>)> =
            args.names_map()?.into_iter().map(|(k, v)| (k.as_str().to_owned(), v)).collect();
        let named: Vec<(&str, Value<'v>)> = owned.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let span = crate::store::call_span(eval);
        crate::emboss::forwarding(span, || eval.eval_function(target.to_value(), &positional, &named))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{errors, run};
    use cadpole_core::render::material_preset;

    fn by_name(name: &str) -> Vocabulary {
        all().into_iter().find(|v| v.name == name).unwrap_or_else(|| panic!("no vocabulary {name}"))
    }

    /// The drift test, one vocabulary at a time and in both directions: every
    /// member is a spelling the parser accepts and round-trips through it,
    /// and the parser's own list is exactly the members. For the enums that
    /// is `ALL` on both sides (and each enum's own test holds `ALL` complete);
    /// for the presets it is `material_preset_names()` against
    /// `material_preset()`; for `kind` it is the two selector lists.
    #[test]
    fn every_vocabulary_is_exactly_what_its_parser_accepts() {
        let m = by_name("match");
        assert_eq!(m.members, SectionMatch::ALL.iter().map(|x| x.name()).collect::<Vec<_>>());
        for (member, variant) in m.members.iter().zip(SectionMatch::ALL) {
            assert_eq!(SectionMatch::parse(member).unwrap(), variant);
            assert_eq!(variant.name(), *member);
        }
        assert!(SectionMatch::parse("arclength").is_err());

        let c = by_name("cap");
        assert_eq!(c.members, Cap::ALL.iter().map(|x| x.name()).collect::<Vec<_>>());
        for (member, variant) in c.members.iter().zip(Cap::ALL) {
            assert_eq!(Cap::parse(member).unwrap(), variant);
        }
        assert!(Cap::parse("flat").is_err());

        let fr = by_name("sweep_frame");
        assert_eq!(fr.members, Frame::ALL.iter().map(|x| x.name()).collect::<Vec<_>>());
        for (member, variant) in fr.members.iter().zip(Frame::ALL) {
            assert_eq!(Frame::parse(member).unwrap(), variant);
        }
        assert!(Frame::parse("bishop").is_err());

        let k = by_name("kernel");
        assert_eq!(k.members, KernelChoice::ALL.iter().map(|x| x.name()).collect::<Vec<_>>());
        for (member, variant) in k.members.iter().zip(KernelChoice::ALL) {
            assert_eq!(KernelChoice::parse(member).unwrap(), variant);
        }
        assert!(KernelChoice::parse("cuda").is_err());

        let mo = by_name("monotone");
        assert_eq!(mo.members, Monotone::ALL.iter().map(|x| x.as_str()).collect::<Vec<_>>());
        for (member, variant) in mo.members.iter().zip(Monotone::ALL) {
            assert_eq!(Monotone::parse(member), Some(variant));
        }
        assert_eq!(Monotone::parse("rising"), None);

        let p = by_name("preset");
        assert_eq!(p.members, material_preset_names());
        for member in &p.members {
            let mat = material_preset(member).unwrap_or_else(|| panic!("preset {member} does not parse"));
            assert_eq!(mat.name, *member);
        }
        assert!(material_preset("unobtainium").is_none());

        let j = by_name("join");
        assert_eq!(j.members, Join::ALL.iter().map(|x| x.name()).collect::<Vec<_>>());
        for (member, variant) in j.members.iter().zip(Join::ALL) {
            assert_eq!(Join::parse(member).unwrap(), variant);
        }
        assert!(Join::parse("square").is_err());

        // `kind` is the union of the two selector lists, each spelling once,
        // faces first so `kind.plane` leads the way the docs do.
        let kd = by_name("kind");
        for f in FACE_KINDS {
            assert!(kd.members.contains(f), "face class {f} is not a kind member");
        }
        for e in EDGE_KINDS {
            assert!(kd.members.contains(e), "curve class {e} is not a kind member");
        }
        for member in &kd.members {
            assert!(
                FACE_KINDS.contains(member) || EDGE_KINDS.contains(member),
                "kind.{member} is accepted by neither faces() nor edges()"
            );
        }
        let mut unique = kd.members.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), kd.members.len(), "a kind is listed twice: {:?}", kd.members);
        assert_eq!(&kd.members[..FACE_KINDS.len()], FACE_KINDS);

        let al = by_name("along");
        assert_eq!(al.members, Along::ALL.iter().map(|a| a.name()).collect::<Vec<_>>());
        for (member, variant) in al.members.iter().zip(Along::ALL) {
            assert_eq!(Along::parse(member), Some(variant));
        }
        assert_eq!(Along::parse("outward"), None);

        let fl = by_name("flavor");
        assert_eq!(fl.members, Flavor::ALL.iter().map(|x| x.name()).collect::<Vec<_>>());
        for (member, variant) in fl.members.iter().zip(Flavor::ALL) {
            assert_eq!(Flavor::parse(member).unwrap(), variant);
            assert_eq!(variant.name(), *member);
        }
        assert!(Flavor::parse("orca").is_err());

        let ea = by_name("ease");
        assert_eq!(ea.members, Ease::ALL.iter().map(|e| e.name()).collect::<Vec<_>>());
        for (member, variant) in ea.members.iter().zip(Ease::ALL) {
            assert_eq!(Ease::parse(member), Some(variant));
        }
        assert_eq!(Ease::parse("bounce"), None);

        let du = by_name("dxf_units");
        assert_eq!(du.members, cadpole_import::dxf::Unit::ALL.iter().map(|u| u.name()).collect::<Vec<_>>());
        for (member, variant) in du.members.iter().zip(cadpole_import::dxf::Unit::ALL) {
            assert_eq!(cadpole_import::dxf::Unit::parse(member).unwrap(), variant);
            assert_eq!(variant.name(), *member);
        }
        assert!(cadpole_import::dxf::Unit::parse("furlong").is_err());

        // The callable ones: `emboss` feeds `kind=` on `emboss()`,
        // `wrap` feeds `method=` on `wrap()`.
        let em = by_name("emboss");
        assert_eq!(em.members, EmbossKind::ALL.iter().map(|x| x.name()).collect::<Vec<_>>());
        for (member, variant) in em.members.iter().zip(EmbossKind::ALL) {
            assert_eq!(EmbossKind::parse(member).unwrap(), variant);
        }
        assert!(EmbossKind::parse("engrave").is_err());
        assert_eq!(em.keyword, "kind");
        assert!(em.is_callable());
        let wr = by_name("wrap");
        assert_eq!(wr.members, Method::ALL.iter().map(|x| x.name()).collect::<Vec<_>>());
        for (member, variant) in wr.members.iter().zip(Method::ALL) {
            assert_eq!(Method::parse(member).unwrap(), variant);
        }
        assert!(Method::parse("conformal").is_err());
        assert_eq!(wr.keyword, "method");
        assert!(wr.is_callable());

        // `sheet` against the paper sizes the parser takes.
        let sh = by_name("sheet");
        assert_eq!(sh.members, cadpole_drawing::SheetSize::NAMES.to_vec());
        for member in &sh.members {
            assert!(cadpole_drawing::SheetSize::parse(member).is_some(), "sheet.{member}");
        }
        for size in cadpole_drawing::SheetSize::ALL {
            assert!(sh.members.contains(&size.name.to_ascii_lowercase().as_str()), "{} is not a sheet member", size.name);
        }
        assert!(cadpole_drawing::SheetSize::parse("legal").is_none());

        // `terrain` against its two constructions.
        let t = by_name("terrain");
        assert_eq!(t.members, cadpole_core::terrain::Method::ALL.iter().map(|m| m.name()).collect::<Vec<_>>());
        for (member, variant) in t.members.iter().zip(cadpole_core::terrain::Method::ALL) {
            assert_eq!(cadpole_core::terrain::Method::parse(member).unwrap(), variant);
        }
        assert!(cadpole_core::terrain::Method::parse("spline").is_err());
        assert!(t.is_callable());

        // `units`: the six attribute spellings of the drawing
        // parser's units, each parsing to the unit at the same position of
        // its name list; callable, and it feeds `units=` under its own name.
        let un = by_name("units");
        assert_eq!(un.members, cadpole_drawing::units::UNIT_MEMBERS.to_vec());
        for (member, name) in un.members.iter().zip(cadpole_drawing::units::UNIT_NAMES) {
            assert_eq!(cadpole_drawing::LengthUnit::parse(member).unwrap().name(), *name);
        }
        assert!(cadpole_drawing::LengthUnit::parse("furlong").is_none());
        assert_eq!(un.keyword, "units");
        assert!(un.is_callable());
        for v in all() {
            let callable = ["emboss", "wrap", "terrain", "units"].contains(&v.name);
            assert_eq!(v.is_callable(), callable, "{}", v.name);
            // A callable vocabulary's keyword is the parameter it feeds (`kind`,
            // `method`) -- except `units`, whose declaration and whose `units=`
            // share the one name.
            assert_eq!(v.keyword == v.name, !v.is_callable() || v.name == "units", "{}", v.name);
        }

        // The registry is what the docs promise, by name and in order.
        let names: Vec<&str> = all().iter().map(|v| v.name).collect();
        assert_eq!(
            names,
            [
                "kind", "match", "cap", "fit", "sweep_frame", "kernel", "preset", "monotone", "hand", "join", "along", "ease", "flavor",
                "sheet", "repair", "dxf_units", "emboss", "wrap", "terrain", "units"
            ]
        );
    }

    /// Every namespace, every member, as the keyword argument it feeds, and
    /// equal to the string form: the whole point, checked through Starlark.
    /// The script is generated from the registry so a member added to a Rust
    /// list is exercised here without anyone editing this test.
    #[test]
    fn every_member_is_its_string_and_every_keyword_takes_it() {
        let mut src = String::from("load(\"@cadpole/gears.star\", \"helical\")\n");
        // The attribute is the string, for every member of every namespace.
        for v in all() {
            for m in &v.members {
                src.push_str(&format!("assert_eq({}.{m}, {m:?})\n", v.name));
                src.push_str(&format!("assert_eq(getattr({}, {m:?}), {m:?})\n", v.name));
            }
        }
        src.push_str(
            "rod = cylinder(5, 20, name=\"rod\")\n\
             assert_eq(rod.faces(kind=kind.cylinder).names(), rod.faces(kind=\"cylinder\").names())\n\
             assert_true(len(rod.faces(kind=kind.cylinder).names()) >= 1)\n\
             assert_eq(rod.faces(kind=kind.plane).names(), rod.faces(kind=\"plane\").names())\n\
             assert_eq(rod.edges(kind=kind.arc, allow_empty=True).names(), rod.edges(kind=\"arc\", allow_empty=True).names())\n",
        );
        // Every face class on faces() and every curve class on edges(); the
        // classes a rod does not have are allowed to select nothing.
        for f in FACE_KINDS {
            src.push_str(&format!("rod.faces(kind=kind.{f}, allow_empty=True)\n"));
        }
        for e in EDGE_KINDS {
            src.push_str(&format!("rod.edges(kind=kind.{e}, allow_empty=True)\n"));
        }
        src.push_str(
            "a = sketch(XY).rect(10, 10)\n\
             b = sketch(XY.offset(10)).rect(8, 8)\n",
        );
        for m in SectionMatch::ALL {
            let n = m.name();
            src.push_str(&format!(
                "l_{n} = loft([a, b], match=match.{n}, name=\"l_{n}\")\n\
                 s_{n} = loft([a, b], match=\"{n}\", name=\"s_{n}\")\n\
                 assert_close(l_{n}.volume(), s_{n}.volume())\n"
            ));
        }
        src.push_str(
            "c_plane = loft([a, b], cap=cap.plane, name=\"c_plane\")\n\
             assert_close(c_plane.volume(), l_index.volume())\n\
             assert_eq(cap.ruled, \"ruled\")\n\
             rail = curve((0, 0, 0)).line((0, 0, 10))\n",
        );
        for f in Frame::ALL {
            let n = f.name();
            src.push_str(&format!(
                "f_{n} = sweep_along(sketch(XY).circle(2), rail, frame=sweep_frame.{n}, name=\"f_{n}\")\n\
                 g_{n} = sweep_along(sketch(XY).circle(2), rail, frame=\"{n}\", name=\"g_{n}\")\n\
                 assert_close(f_{n}.volume(), g_{n}.volume())\n"
            ));
        }
        for k in KernelChoice::ALL {
            let n = k.name();
            src.push_str(&format!(
                "u_{n} = union(box(10, 10, 10), box(10, 10, 10, at=(5, 5, 5)), kernel=kernel.{n}, name=\"u_{n}\")\n\
                 v_{n} = union(box(10, 10, 10), box(10, 10, 10, at=(5, 5, 5)), kernel=\"{n}\", name=\"v_{n}\")\n\
                 assert_close(u_{n}.volume(), v_{n}.volume(), tol=1e-6)\n"
            ));
        }
        for (i, p) in material_preset_names().iter().enumerate() {
            src.push_str(&format!("m_{i} = box(1, 1, 1, name=\"m_{i}\").material(preset=preset.{p})\n"));
        }
        src.push_str(
            "wall = param(\"wall\", 2)\n\
             def vol():\n    return rod.volume()\n\
             test(\"up\", vol, sweep={\"wall\": [1, 2]}, monotone=monotone.increasing)\n\
             test(\"down\", vol, sweep={\"wall\": [1, 2]}, monotone=monotone.decreasing)\n",
        );
        for h in crate::stdlib::HANDS {
            src.push_str(&format!("h_{h} = helical(2, 12, 4, 15, hand=hand.{h}, name=\"h_{h}\")\n"));
        }
        src.push_str("plate = sketch(XY, name=\"plate\").rect(10, 10)\n");
        for j in Join::ALL {
            let n = j.name();
            src.push_str(&format!(
                "o_{n} = plate.offset(1, join=join.{n}, name=\"o_{n}\")\n\
                 p_{n} = plate.offset(1, join=\"{n}\", name=\"p_{n}\")\n\
                 assert_eq(o_{n}.loops, p_{n}.loops)\n"
            ));
        }
        // Every direction on explode(), as the member and as the string.
        src.push_str(
            "rod_p = part(rod, name=\"rod_p\")\n\
             asm = assembly([instance(rod_p, name=\"rod_a\"), instance(rod_p, at=(30, 0, 0), name=\"rod_b\")], name=\"pair\")\n",
        );
        for a in Along::ALL {
            let n = a.name();
            src.push_str(&format!("explode(asm, 5, along=along.{n})\nexplode(asm, 5, along=\"{n}\")\n"));
        }
        // Every easing on keyframe() and on animation(), as the member and as
        // the string.
        for e in Ease::ALL {
            let n = e.name();
            src.push_str(&format!(
                "animation(\"m_{n}\", [keyframe(0, wall=1, ease=ease.{n}), keyframe(1, wall=2)], ease=ease.{n})\n\
                 animation(\"s_{n}\", [keyframe(0, wall=1, ease=\"{n}\"), keyframe(1, wall=2)], ease=\"{n}\")\n"
            ));
        }
        // Every kind of emboss on a fresh plate, and every wrap method on a
        // cylinder, as the member and as the string.
        for k in EmbossKind::ALL {
            let n = k.name();
            let (height, extra) = match k {
                EmbossKind::Paint => ("", ", color=\"#ff0000\""),
                _ => (", 1", ""),
            };
            src.push_str(&format!(
                "e_{n} = emboss(box(40, 30, 5, name=\"e_{n}_plate\"), \"e_{n}_plate/top\", sketch(XY.offset(5)).rect(10, 6, center=(20, 15)){height}, kind=emboss.{n}, name=\"e_{n}\"{extra})\n\
                 f_{n} = emboss(box(40, 30, 5, name=\"f_{n}_plate\"), \"f_{n}_plate/top\", sketch(XY.offset(5)).rect(10, 6, center=(20, 15)){height}, kind=\"{n}\", name=\"f_{n}\"{extra})\n\
                 assert_close(e_{n}.volume(), f_{n}.volume(), tol=1e-9)\n"
            ));
        }
        src.push_str("mug = cylinder(20, 40, name=\"mug\")\nmark = sketch(XY).rect(6, 4)\n");
        for m in Method::ALL {
            let n = m.name();
            src.push_str(&format!(
                "w_{n} = wrap(mark, mug.face(\"mug/side[0.0]\"), method=wrap.{n}, origin=(0, 20, 20))\n\
                 x_{n} = wrap(mark, mug.face(\"mug/side[0.0]\"), method=\"{n}\", origin=(0, 20, 20))\n\
                 assert_eq(w_{n}.method, x_{n}.method)\n\
                 assert_eq(w_{n}.method, \"{n}\")\n"
            ));
        }
        // Every paper size on sheet_set(), as the member and as the string.
        for n in cadpole_drawing::SheetSize::NAMES {
            src.push_str(&format!(
                "ss_{n} = sheet_set(\"ss_{n}\", size=sheet.{n})\n\
                 st_{n} = sheet_set(\"st_{n}\", size=\"{n}\")\n\
                 assert_eq(ss_{n}.size(), st_{n}.size())\n"
            ));
        }
        // Every unit on drawing(), as the member and as the string;
        // the declaration itself is once per document, so the member's own
        // call is a separate script below.
        src.push_str("du = box(10, 10, 10, name=\"du\")\n");
        for m in cadpole_drawing::units::UNIT_MEMBERS {
            src.push_str(&format!(
                "d_{m} = drawing(du, view=\"front\", units=units.{m}, name=\"d_{m}\")\n\
                 e_{m} = drawing(du, view=\"front\", units=\"{m}\", name=\"e_{m}\")\n\
                 assert_eq(d_{m}.items, e_{m}.items)\n"
            ));
        }
        let ctx = run(&src);
        assert!(errors(&ctx).is_empty(), "{:#?}\n--- script ---\n{src}", errors(&ctx));
        for m in cadpole_drawing::units::UNIT_MEMBERS {
            let ctx = run(&format!("units(units.{m})\n"));
            assert!(errors(&ctx).is_empty(), "{m}: {:?}", errors(&ctx));
            assert_eq!(
                ctx.display_units.as_ref().map(|u| u.unit.as_str()),
                Some(cadpole_drawing::LengthUnit::parse(m).unwrap().name())
            );
        }
    }

    /// A plain vocabulary is not callable, and says so; a callable one is
    /// the function, with the call site kept.
    #[test]
    fn a_plain_vocabulary_is_not_callable_and_a_callable_one_is_its_function() {
        let ctx = run("x = kind(\"cylinder\")\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("kind is a vocabulary") && e[0].contains("not a function"), "{}", e[0]);

        let ctx = run(
            "assert_eq(type(emboss), \"vocabulary\")\n\
             assert_eq(str(emboss), \"emboss: raised | cut | paint | scribe\")\n\
             assert_eq(dir(wrap), [\"project\", \"unroll\"])\n\
             assert_true(hasattr(emboss, \"scribe\"))\n\
             plate = box(40, 30, 5, name=\"plate\")\n\
             out = emboss(plate, \"plate/top\", sketch(XY.offset(5)).rect(10, 6, center=(20, 15)), 2, kind=emboss.raised, name=\"r\")\n\
             assert_close(out.volume(), plate.volume() + 120, tol=1e-6)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// `kind.cylnder` fails where it is written, names the namespace, and
    /// lists the members in the same `; valid: ...` shape as the string
    /// parser's diagnostic, so the two misspellings read alike.
    #[test]
    fn an_unknown_member_is_an_error_at_the_site_that_lists_the_members() {
        let ctx = run("rod = cylinder(5, 20, name=\"rod\")\nside = rod.faces(kind=kind.cylnder)\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(
            e[0],
            "kind.cylnder is not a member of kind (did you mean kind.cylinder?); valid: plane, cylinder, \
             cone, sphere, torus, bspline, nurbs, revolved, line, intersection, circle, arc"
        );
        let d = ctx.diagnostics.iter().find(|d| d.message == e[0]).unwrap();
        assert_eq!(d.span.as_ref().map(|s| s.start_line), Some(2), "the error is on the line that misspells it");

        // No near miss: no hint, same list.
        let ctx = run("x = kernel.gpu\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0], "kernel.gpu is not a member of kernel; valid: auto, brep, mesh");

        // The string form's diagnostic ends the same way.
        let ctx = run("rod = cylinder(5, 20, name=\"rod\")\nside = rod.faces(kind=\"cylnder\")\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(
            e[0].ends_with("; valid: plane, cylinder, cone, sphere, torus, bspline, nurbs, revolved"),
            "{}",
            e[0]
        );
        assert!(e[0].starts_with("kind=\"cylnder\" is not a face class"), "{}", e[0]);

        // An attribute error on any other type is left alone.
        let ctx = run("p = XY.nomal\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("has no attribute `nomal`"), "{}", e[0]);
    }

    /// `dir`, `str`, `repr`, `type`, `hasattr`, and that nothing can be
    /// assigned into a namespace.
    #[test]
    fn a_vocabulary_introspects_and_is_frozen() {
        let ctx = run(
            "assert_eq(type(kind), \"vocabulary\")\n\
             assert_eq(type(preset), \"vocabulary\")\n\
             assert_eq(str(match), \"match: index | arclen\")\n\
             assert_eq(repr(match), \"match: index | arclen\")\n\
             assert_eq(str(kernel), \"kernel: auto | brep | mesh\")\n\
             assert_eq(dir(match), [\"arclen\", \"index\"])\n\
             assert_eq(dir(cap), [\"plane\", \"ruled\"])\n\
             assert_true(hasattr(kernel, \"mesh\"))\n\
             assert_true(not hasattr(kernel, \"cuda\"))\n\
             assert_eq(getattr(kernel, \"cuda\", \"fallback\"), \"fallback\")\n\
             assert_eq(len(dir(kind)), 12)\n\
             assert_eq(len(dir(preset)), 18)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));

        let ctx = run("kernel.mesh = \"gpu\"\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "a namespace is not assignable: {e:?}");
        assert!(e[0].contains("vocabulary") || e[0].contains("mmutable"), "{}", e[0]);
    }

    /// Inside a `test()` body the miss is reported by the test runner rather
    /// than by `evaluate`, and it reads the same there.
    #[test]
    fn an_unknown_member_inside_a_test_body_reads_the_same() {
        use crate::{EvalInput, ScriptFile, run_tests};
        let input = EvalInput {
            scripts: vec![ScriptFile {
                path: "main.star".into(),
                source: "rod = cylinder(5, 20, name=\"rod\")\n\
                         def sides():\n    return rod.faces(kind=kind.cylnder).count()\n\
                         test(\"sides\", sides)\n"
                    .into(),
            }],
            ..Default::default()
        };
        let run = run_tests(input, &[], 1);
        assert!(errors(&run.base.ctx).is_empty(), "{:?}", errors(&run.base.ctx));
        assert_eq!(run.outcomes.len(), 1);
        assert!(!run.outcomes[0].ok);
        assert!(
            run.outcomes[0].message.starts_with("kind.cylnder is not a member of kind (did you mean kind.cylinder?); valid: plane, cylinder"),
            "{}",
            run.outcomes[0].message
        );
        assert_eq!(run.outcomes[0].span.as_ref().map(|s| s.start_line), Some(3));
    }

    /// A script variable of the same name shadows the global, as any Starlark
    /// variable does: `examples/gridfinity_bin.star` has had
    /// `kernel = param("kernel", ...)` since before the namespace existed and
    /// must go on working.
    #[test]
    fn a_script_variable_may_shadow_a_vocabulary() {
        let ctx = run(
            "kernel = param(\"kernel\", \"mesh\", choices=[\"auto\", \"brep\", \"mesh\"])\n\
             kind = \"cylinder\"\n\
             u = union(box(10, 10, 10), box(10, 10, 10, at=(5, 0, 0)), kernel=kernel, name=\"u\")\n\
             assert_true(len(cylinder(5, 20, name=\"rod\").faces(kind=kind).names()) >= 1)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }
}
