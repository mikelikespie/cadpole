//! Display layers: named groups a viewer switches on and off and
//! ghosts without re-evaluating the script.
//!
//! A layer is metadata. `layer(name, ...)` in a script declares one with
//! its own defaults ([`LayerDecl`]); a body names its layer in
//! [`crate::BodyRecord::layer`] and carries that name through every
//! operation that carries its material. Nothing here touches geometry, and
//! nothing downstream of the evaluation — the memo, the tessellation, the
//! exports — reads it. What reads it is a picture: the web viewer's
//! per-layer state, and a render's [`LayerOverride`]s (`cadpole render --layer
//! envelope=off`, `view(layers=)` in a keyframe), which [`effective`] folds
//! over the declarations to say how a body is drawn.

use crate::Span;
use cadpole_proto::v1 as pb;

/// One `layer(...)` declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerDecl {
    pub name: String,
    pub doc: String,
    /// Drawn unless something says otherwise.
    pub visible: bool,
    /// 0..1; below 1 the layer's bodies are ghosted.
    pub opacity: f64,
    /// A tint for a UI's swatch, linear RGB; none by default.
    pub color: Option<[f64; 3]>,
    /// Where the declaration is, for the error that names a redeclaration.
    pub span: Option<Span>,
}

impl LayerDecl {
    /// The settings a redeclaration has to repeat: everything but the span.
    pub fn same_settings(&self, other: &LayerDecl) -> bool {
        self.name == other.name
            && self.doc == other.doc
            && self.visible == other.visible
            && self.opacity == other.opacity
            && self.color == other.color
    }

    pub fn to_proto(&self, order: usize) -> pb::Layer {
        pb::Layer {
            name: self.name.clone(),
            doc: self.doc.clone(),
            visible: self.visible,
            opacity: self.opacity,
            color: self.color.map(|c| pb::Color { r: c[0], g: c[1], b: c[2] }),
            order: order as u32,
        }
    }
}

/// What a render or a keyframe changes about one layer: a field left
/// `None` keeps the declaration's own value. The empty name is the implicit
/// layer of untagged bodies.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerOverride {
    pub name: String,
    pub visible: Option<bool>,
    pub opacity: Option<f64>,
}

impl LayerOverride {
    /// The spelling `cadpole render --layer` takes, and `view(layers=)` maps
    /// to: `NAME=on`, `NAME=off`, or `NAME=<opacity>` with the opacity in
    /// 0..1 (`envelope=0.3`), which also shows the layer. The name is kept
    /// as written; [`resolve_implicit`] is what turns `other` into the
    /// implicit layer once the declarations are known.
    pub fn parse(spec: &str) -> Result<LayerOverride, String> {
        let Some((name, value)) = spec.split_once('=') else {
            return Err(format!("--layer {spec:?}: expected NAME=on, NAME=off or NAME=<opacity 0..1>"));
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() {
            return Err(format!("--layer {spec:?}: the layer name is empty"));
        }
        let mut out = LayerOverride { name: name.to_owned(), visible: None, opacity: None };
        match value.to_ascii_lowercase().as_str() {
            "on" | "true" | "yes" | "show" => out.visible = Some(true),
            "off" | "false" | "no" | "hide" | "hidden" => out.visible = Some(false),
            _ => {
                let o: f64 = value
                    .trim_end_matches('%')
                    .parse()
                    .map_err(|_| format!("--layer {spec:?}: expected on, off or an opacity between 0 and 1, got {value:?}"))?;
                let o = if value.ends_with('%') { o / 100.0 } else { o };
                if !(0.0..=1.0).contains(&o) || o.is_nan() {
                    return Err(format!("--layer {spec:?}: an opacity is between 0 and 1, got {value:?}"));
                }
                out.visible = Some(true);
                out.opacity = Some(o);
            }
        }
        Ok(out)
    }

    pub fn to_proto(&self) -> pb::LayerState {
        pb::LayerState { name: self.name.clone(), visible: self.visible, opacity: self.opacity }
    }

    pub fn from_proto(s: &pb::LayerState) -> LayerOverride {
        LayerOverride { name: s.name.clone(), visible: s.visible, opacity: s.opacity.map(|o| o.clamp(0.0, 1.0)) }
    }

    /// `later` written over `self`, field by field: the hold rule a
    /// keyframe's `view(layers=)` follows.
    pub fn over(&self, later: &LayerOverride) -> LayerOverride {
        LayerOverride {
            name: self.name.clone(),
            visible: later.visible.or(self.visible),
            opacity: later.opacity.or(self.opacity),
        }
    }
}

/// `other` names the implicit layer of untagged bodies — `cadpole render
/// --layer other=off`, `view(layers={"other": False})` — unless the script
/// declares a layer called `other`, in which case it names that one. The
/// implicit layer's own name is the empty string, which no `layer(...)`
/// can declare.
pub fn resolve_implicit(overrides: &[LayerOverride], declared: &[LayerDecl]) -> Vec<LayerOverride> {
    let declared_other = declared.iter().any(|l| l.name == "other");
    overrides
        .iter()
        .map(|o| {
            if o.name == "other" && !declared_other {
                LayerOverride { name: String::new(), ..o.clone() }
            } else {
                o.clone()
            }
        })
        .collect()
}

/// How a body on `layer` is drawn: its layer's declared defaults with every
/// matching override written over them, last one winning. A body on no
/// layer (the empty name) is drawn, opaque, unless an override names the
/// implicit layer. A named layer nothing declares — a body tagged by a
/// name the script never declared, which the script side refuses, or a
/// stale override — reads as drawn and opaque.
pub fn effective(layer: &str, declared: &[LayerDecl], overrides: &[LayerOverride]) -> (bool, f64) {
    let (mut visible, mut opacity) = match declared.iter().find(|d| d.name == layer) {
        Some(d) => (d.visible, d.opacity),
        None => (true, 1.0),
    };
    for o in overrides.iter().filter(|o| o.name == layer) {
        if let Some(v) = o.visible {
            visible = v;
        }
        if let Some(a) = o.opacity {
            opacity = a.clamp(0.0, 1.0);
        }
    }
    (visible, opacity)
}

/// Fold a later list of overrides over an earlier one, per layer.
pub fn merge_overrides(base: &[LayerOverride], later: &[LayerOverride]) -> Vec<LayerOverride> {
    let mut out: Vec<LayerOverride> = base.to_vec();
    for l in later {
        match out.iter_mut().find(|o| o.name == l.name) {
            Some(o) => *o = o.over(l),
            None => out.push(l.clone()),
        }
    }
    out
}

/// The layer a name typed by a user picks: a declared layer by its
/// own name, or the implicit layer of untagged bodies as `other` -- the
/// spelling `cadpole render --layer other=off` already uses -- unless the
/// script declares a layer called `other`. The answer is the layer's name
/// as a body records it (the empty string for the implicit layer); any
/// other name is an error that lists the declared layers, so a typo in
/// `cadpole check --layer` or `bodies(layer=)` is never an empty selection.
pub fn resolve_layer(name: &str, declared: &[LayerDecl]) -> Result<String, String> {
    let name = name.trim();
    if declared.iter().any(|d| d.name == name) {
        return Ok(name.to_owned());
    }
    if name == "other" || name.is_empty() {
        return Ok(String::new());
    }
    let known: Vec<&str> = declared.iter().map(|d| d.name.as_str()).collect();
    Err(if known.is_empty() {
        format!("no layer {name:?}: the script declares no layers (`other` is the implicit layer of untagged bodies)")
    } else {
        format!(
            "no layer {name:?}; the declared layers are {} (and `other`, the implicit layer of untagged bodies)",
            known.join(", ")
        )
    })
}

/// Is a body on `layer` drawn when nothing overrides it: the declaration's
/// `visible`; true for the implicit layer and for a name nothing declares.
/// A whole-document export and `cadpole check` leave out a body that is not
/// shown.
pub fn shown(layer: &str, declared: &[LayerDecl]) -> bool {
    effective(layer, declared, &[]).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_layer_name_resolves_or_lists_the_declared_ones() {
        let d = [decl("walls", true, 1.0), decl("site", false, 1.0)];
        assert_eq!(resolve_layer("walls", &d).unwrap(), "walls");
        assert_eq!(resolve_layer(" site ", &d).unwrap(), "site");
        // `other` is the implicit layer, unless a layer of that name exists.
        assert_eq!(resolve_layer("other", &d).unwrap(), "");
        assert_eq!(resolve_layer("other", &[decl("other", true, 1.0)]).unwrap(), "other");
        let e = resolve_layer("wals", &d).unwrap_err();
        assert!(e.contains("\"wals\"") && e.contains("walls, site") && e.contains("other"), "{e}");
        assert!(resolve_layer("walls", &[]).unwrap_err().contains("declares no layers"));
        assert!(shown("walls", &d) && !shown("site", &d) && shown("", &d) && shown("undeclared", &d));
    }

    fn decl(name: &str, visible: bool, opacity: f64) -> LayerDecl {
        LayerDecl { name: name.into(), doc: String::new(), visible, opacity, color: None, span: None }
    }

    #[test]
    fn parse_takes_on_off_and_an_opacity() {
        let d = [decl("envelope", true, 1.0)];
        assert_eq!(LayerOverride::parse("envelope=off").unwrap(), LayerOverride { name: "envelope".into(), visible: Some(false), opacity: None });
        assert_eq!(LayerOverride::parse("envelope=on").unwrap(), LayerOverride { name: "envelope".into(), visible: Some(true), opacity: None });
        assert_eq!(LayerOverride::parse("terrain=0.4").unwrap(), LayerOverride { name: "terrain".into(), visible: Some(true), opacity: Some(0.4) });
        assert_eq!(LayerOverride::parse("terrain=40%").unwrap().opacity, Some(0.4));
        // The implicit layer is `other` unless a layer of that name exists.
        let other = [LayerOverride::parse("other=off").unwrap()];
        assert_eq!(resolve_implicit(&other, &d)[0].name, "");
        assert_eq!(resolve_implicit(&other, &[decl("other", true, 1.0)])[0].name, "other");
        for bad in ["envelope", "envelope=", "envelope=maybe", "envelope=1.5", "envelope=-1", "=off"] {
            assert!(LayerOverride::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn effective_folds_overrides_over_the_declaration() {
        let d = [decl("envelope", true, 1.0), decl("ghost", false, 0.3)];
        assert_eq!(effective("envelope", &d, &[]), (true, 1.0));
        assert_eq!(effective("ghost", &d, &[]), (false, 0.3));
        assert_eq!(effective("", &d, &[]), (true, 1.0));
        assert_eq!(effective("unknown", &d, &[]), (true, 1.0));
        let o = [LayerOverride::parse("envelope=0.5").unwrap(), LayerOverride::parse("ghost=on").unwrap()];
        assert_eq!(effective("envelope", &d, &o), (true, 0.5));
        assert_eq!(effective("ghost", &d, &o), (true, 0.3));
        // Later overrides win.
        let o2 = merge_overrides(&o, &[LayerOverride::parse("envelope=off").unwrap()]);
        assert_eq!(effective("envelope", &d, &o2), (false, 0.5));
        assert_eq!(effective("", &d, &resolve_implicit(&[LayerOverride::parse("other=off").unwrap()], &d)), (false, 1.0));
    }
}
