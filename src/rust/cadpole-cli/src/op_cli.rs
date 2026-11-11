//! `cadpole op`: the CLI front-end generated from the operation registry.
//!
//! Every rpc of `cadpole.v1.CadpoleOps` becomes a subcommand named after the rpc
//! (with its `(cadpole.op).cli_name` and aliases accepted too), and every
//! non-hidden field of its request message becomes a flag. Nothing here knows
//! what an operation *is*; `cadpole-ops` runs it and this prints the JSON.
//!
//! Flag generation covers the top level of the request message plus, one level
//! down, any field that opted in with an explicit `(cadpole.field).flag` (that is
//! how `camera.preset` becomes `--view`). Anything deeper is set with `--json`,
//! which is merged under the flags.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Arg, ArgAction, ArgMatches, Command};
use cadpole_ops::{Host, MethodInfo};
use prost_reflect::{Kind, MessageDescriptor};
use serde_json::{Map, Value, json};

/// The operation names `cadpole op` accepts, in registry order. Exists so the
/// cross-front-end test in main.rs can compare the three name sets.
#[cfg_attr(not(test), allow(dead_code))]
pub fn op_names() -> Vec<String> {
    cadpole_ops::registry().iter().filter(|m| !m.hidden).map(|m| m.name.clone()).collect()
}

/// `SCREAMING_SNAKE` of a CamelCase name, for enum value prefixes.
fn screaming(name: &str) -> String {
    cadpole_ops::snake_case(name).to_ascii_uppercase()
}

fn leaked(s: String) -> &'static str {
    // clap wants &'static str for names and help; the registry is itself
    // static, so leaking the handful of strings we derive from it is honest.
    Box::leak(s.into_boxed_str())
}

/// Fields of `md` that become flags: the top level, plus one level of nested
/// message fields that asked for a flag by name.
fn flag_fields(md: &MessageDescriptor) -> Vec<(Vec<String>, prost_reflect::FieldDescriptor, cadpole_ops::FieldOpts)> {
    let mut out: Vec<(Vec<String>, prost_reflect::FieldDescriptor, cadpole_ops::FieldOpts)> = Vec::new();
    for f in md.fields() {
        let o = cadpole_ops::field_opts(&f);
        if o.hidden {
            continue;
        }
        if let (Kind::Message(inner), false, false) = (f.kind(), f.is_list(), f.is_map()) {
            // A singular message: only its explicitly flagged scalars surface.
            for sub in inner.fields() {
                let so = cadpole_ops::field_opts(&sub);
                if so.hidden || !cadpole_ops::has_field_opts(&sub) || so.flag.is_empty() || matches!(sub.kind(), Kind::Message(_)) || sub.is_list() || sub.is_map() {
                    continue;
                }
                if out.iter().any(|(_, _, e)| e.flag == so.flag) {
                    continue;
                }
                out.push((vec![f.name().to_owned(), sub.name().to_owned()], sub.clone(), so));
            }
            continue;
        }
        if out.iter().any(|(_, _, e)| e.flag == o.flag) {
            continue;
        }
        out.push((vec![f.name().to_owned()], f, o));
    }
    out
}

fn field_help(f: &prost_reflect::FieldDescriptor, o: &cadpole_ops::FieldOpts) -> String {
    // The proto comment is the help text; the example, if any, is appended.
    let mut h = cadpole_ops::summary(&cadpole_ops::field_doc(f));
    if h.is_empty() {
        h = format!("{} ({})", f.name(), kind_name(f));
    }
    if !o.example.is_empty() {
        h.push_str(&format!(" [e.g. {}]", o.example));
    }
    h
}

fn kind_name(f: &prost_reflect::FieldDescriptor) -> String {
    if f.is_map() {
        return "NAME=VALUE".into();
    }
    match f.kind() {
        Kind::Bool => "BOOL".into(),
        Kind::String => "TEXT".into(),
        Kind::Bytes => "BASE64".into(),
        Kind::Enum(e) => e.values().map(|v| v.name().to_owned()).collect::<Vec<_>>().join("|"),
        Kind::Message(m) => m.name().to_ascii_uppercase(),
        _ => "NUMBER".into(),
    }
}

/// The generated `cadpole op` command.
pub fn command() -> Command {
    let mut cmd = Command::new("op")
        .about("Call any Cadpole operation by name (generated from cadpole.v1.CadpoleOps)")
        .long_about(
            "Every operation Cadpole has is an rpc of `cadpole.v1.CadpoleOps` in proto/cadpole/v1/ops.proto.\n\
             `cadpole op --list` shows them; `cadpole op <Method> --help` shows one operation's flags,\n\
             which are generated from its request message. `--json` takes a whole request as\n\
             proto3 JSON and individual flags override it. Output is proto3 JSON on stdout; the exit\n\
             status is 1 when the response carries an error diagnostic (or RunTests/PrintCheck are not\n\
             `ok`), the JSON still describing it, and 2 for a usage error.",
        )
        .arg(Arg::new("list").long("list").action(ArgAction::SetTrue).help("List the operations and exit"))
        .arg(Arg::new("doc").long("doc").global(true).value_name("FILE").help("Document (a .star script or a packed document) for operations that take one"))
        .arg(Arg::new("server").long("server").global(true).value_name("URL").env("CADPOLE_SERVER").help("A running `cadpole serve` for session operations: unix:/path/to.sock or http://host:port (default: the running one, found in the runtime directory, when the operation needs a session and no --doc is given)"))
        .arg(Arg::new("token").long("token").global(true).value_name("TOKEN").env("CADPOLE_TOKEN").help("Bearer token for --server"))
        .arg(Arg::new("json").long("json").global(true).value_name("JSON").help("The whole request as proto3 JSON; flags override its fields"))
        .arg(Arg::new("compact").long("compact").global(true).action(ArgAction::SetTrue).help("Print the response as one line instead of indented"));

    for m in cadpole_ops::registry().iter().filter(|m| !m.hidden) {
        let md = match cadpole_ops::pool().get_message_by_name(&m.input) {
            Some(md) => md,
            None => continue,
        };
        let mut sub = Command::new(leaked(m.name.clone()))
            .about(leaked(m.summary()))
            .long_about(leaked(m.doc.clone()))
            .aliases(
                std::iter::once(m.cli_name.clone())
                    .chain(std::iter::once(m.snake.clone()))
                    .chain(m.aliases.iter().cloned())
                    .filter(|a| a != &m.name)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .map(leaked)
                    .collect::<Vec<_>>(),
            );
        if !m.image_field.is_empty() {
            sub = sub.arg(
                Arg::new("out")
                    .long("out")
                    .short('o')
                    .value_name("FILE")
                    .help(leaked(format!("Write the response's `{}` PNG here", m.image_field))),
            );
        }
        let mut positional_index = 1;
        for (_, f, o) in flag_fields(&md) {
            let id = leaked(f.name().to_owned());
            let mut arg = Arg::new(id).help(leaked(field_help(&f, &o))).value_name(leaked(kind_name(&f)));
            if o.positional {
                arg = arg.index(positional_index);
                positional_index += 1;
            } else {
                arg = arg.long(leaked(o.flag.clone()));
                if let Some(c) = o.short.chars().next() {
                    arg = arg.short(c);
                }
            }
            arg = match (f.kind(), f.is_list() || f.is_map()) {
                (Kind::Bool, false) => arg.action(ArgAction::SetTrue).num_args(0),
                (_, true) => arg.action(ArgAction::Append),
                _ => arg.action(ArgAction::Set),
            };
            if o.required {
                arg = arg.required(true);
            }
            sub = sub.arg(arg);
        }
        cmd = cmd.subcommand(sub);
    }
    cmd
}

/// A `cadpole.v1.ParamValue` as proto3 JSON.
fn param_value_json(v: &cadpole_engine::cadpole_core::ParamValue) -> Value {
    use cadpole_engine::cadpole_core::ParamValue as P;
    match v {
        P::Number(n) => json!({"number": n}),
        P::Bool(b) => json!({"boolean": b}),
        P::Text(s) => json!({"text": s}),
        P::Vector(v) => json!({"vector": {"x": v.x, "y": v.y, "z": v.z}}),
    }
}

fn scalar_json(f: &prost_reflect::FieldDescriptor, raw: &str) -> anyhow::Result<Value> {
    Ok(match f.kind() {
        Kind::Bool => json!(matches!(raw, "true" | "1" | "yes")),
        Kind::String => json!(raw),
        Kind::Bytes => json!(raw),
        Kind::Enum(e) => {
            let wanted = format!("{}_{}", screaming(e.name()), raw.to_ascii_uppercase());
            let name = e
                .values()
                .find(|v| v.name() == raw || v.name() == wanted)
                .map(|v| v.name().to_owned())
                .unwrap_or_else(|| raw.to_owned());
            json!(name)
        }
        // A layer override is spelt as `cadpole render` spells it,
        // `envelope=off`; JSON is still accepted for a client that has it.
        Kind::Message(m) if m.full_name() == "cadpole.v1.LayerState" && !raw.trim_start().starts_with('{') => {
            let st = cadpole_engine::parse_layer_state(raw)?;
            let mut o = Map::new();
            o.insert("name".into(), json!(st.name));
            if let Some(v) = st.visible {
                o.insert("visible".into(), json!(v));
            }
            if let Some(a) = st.opacity {
                o.insert("opacity".into(), json!(a));
            }
            Value::Object(o)
        }
        Kind::Message(_) => serde_json::from_str(raw).map_err(|e| anyhow::anyhow!("{}: expected JSON: {e}", f.name()))?,
        _ => raw.parse::<f64>().map(|n| json!(n)).map_err(|_| anyhow::anyhow!("{}: expected a number, got {raw:?}", f.name()))?,
    })
}

fn set_path(root: &mut Value, path: &[String], value: Value) {
    let mut cur = root;
    for key in &path[..path.len() - 1] {
        if !cur.get(key).map(|v| v.is_object()).unwrap_or(false) {
            cur[key] = Value::Object(Map::new());
        }
        cur = cur.get_mut(key).unwrap();
    }
    cur[path.last().unwrap().clone()] = value;
}

/// Build a request from the parsed flags, merged over `--json`.
pub fn request_json(md: &MessageDescriptor, matches: &ArgMatches, base: Value) -> anyhow::Result<Value> {
    let mut out = if base.is_object() { base } else { json!({}) };
    for (path, f, _) in flag_fields(md) {
        let id = f.name();
        if !matches.contains_id(id) || matches.value_source(id) == Some(clap::parser::ValueSource::DefaultValue) {
            continue;
        }
        if matches!(f.kind(), Kind::Bool) && !f.is_list() && !f.is_map() {
            if matches.get_flag(id) {
                set_path(&mut out, &path, json!(true));
            }
            continue;
        }
        let Some(values) = matches.get_many::<String>(id) else { continue };
        let values: Vec<&String> = values.collect();
        if f.is_map() {
            let Kind::Message(entry) = f.kind() else { continue };
            let value_kind = entry.map_entry_value_field().kind();
            let mut map = Map::new();
            let mut order = Vec::new();
            for raw in &values {
                let (k, v) = raw.split_once('=').ok_or_else(|| anyhow::anyhow!("--{}: expected NAME=VALUE, got {raw:?}", f.name()))?;
                let (k, v) = (k.trim().to_owned(), v.trim());
                order.push(Value::String(k.clone()));
                let entry_value = match &value_kind {
                    Kind::Message(m) if m.full_name() == "cadpole.v1.ParamValue" => param_value_json(&cadpole_engine::parse_value(v)),
                    Kind::Message(m) if m.full_name() == "cadpole.v1.ParamValues" => {
                        json!({"values": v.split(',').map(|x| param_value_json(&cadpole_engine::parse_value(x.trim()))).collect::<Vec<_>>()})
                    }
                    Kind::Message(_) => serde_json::from_str(v)?,
                    _ => scalar_json(&entry.map_entry_value_field(), v)?,
                };
                map.insert(k, entry_value);
            }
            set_path(&mut out, &path, Value::Object(map));
            // A proto3 map is unordered; where the message has a companion
            // ordering field (sweeps), record the order the flags were given.
            if md.get_field_by_name("param_order").is_some() && f.name() == "params" {
                out["param_order"] = Value::Array(order);
            }
            continue;
        }
        if f.is_list() {
            let items: anyhow::Result<Vec<Value>> = values.iter().map(|v| scalar_json(&f, v)).collect();
            set_path(&mut out, &path, Value::Array(items?));
            continue;
        }
        set_path(&mut out, &path, scalar_json(&f, values.last().unwrap())?);
    }
    Ok(out)
}

/// The first sentence of a summary, clipped to `max` characters. `cadpole op
/// --list` is a table; the whole paragraph belongs in `--help`.
///
/// "e.g." and "i.e." end in a period too, so a full stop only ends the
/// sentence when the letter before it is not itself preceded by one.
fn headline(s: &str, max: usize) -> String {
    let b = s.as_bytes();
    let mut out = s;
    for (i, _) in s.match_indices(". ") {
        if i >= 2 && b[i - 2] == b'.' {
            continue;
        }
        out = &s[..i + 1];
        break;
    }
    match out.char_indices().nth(max) {
        Some((i, _)) => format!("{}...", out[..i].trim_end()),
        None => out.to_owned(),
    }
}

fn print_list() {
    println!("{:<14} {:<14} {}", "OPERATION", "CLI NAME", "WHAT IT DOES");
    for m in cadpole_ops::registry().iter().filter(|m| !m.hidden) {
        let mut tags = Vec::new();
        if m.needs_session {
            tags.push("session");
        }
        if !m.image_field.is_empty() {
            tags.push("image");
        }
        if m.idempotent {
            tags.push("read-only");
        }
        let tags = if tags.is_empty() { String::new() } else { format!("  [{}]", tags.join(", ")) };
        println!("{:<14} {:<14} {}{}", m.name, m.cli_name, headline(&m.summary(), 88), tags);
    }
    println!("\n`cadpole op <OPERATION> --help` for one operation's flags; `--json` for the whole request.");
}

/// Pick the host: a session when `--server` is given; the running server,
/// found by discovery, for a session operation with no `--doc`; otherwise the
/// document named by `--doc` (or nothing, which is an error only if an
/// operation needs a document).
pub fn host(server: Option<&str>, token: Option<String>, doc: Option<&PathBuf>, needs_session: bool) -> anyhow::Result<Box<dyn Host>> {
    if server.is_some() || (needs_session && doc.is_none()) {
        let tokens = match token.filter(|t| !t.is_empty()) {
            Some(t) => crate::runtime::TokenSource::Explicit(t),
            None => crate::runtime::TokenSource::Owner { file: None },
        };
        let endpoint = crate::runtime::Endpoint::new(server, tokens)?;
        return Ok(Box::new(crate::ops_host::HttpHost::on(std::sync::Arc::new(endpoint), format!("cli:{}", std::process::id()))));
    }
    // No session: the document `--doc` names, if any, and this process's own
    // solve jobs for `Solve`, `ListJobs`, `WaitJobs` and `CancelJobs`.
    let doc = doc.map(|p| crate::input::load(p)).transpose()?;
    let mut settings = crate::opt_jobs::jobs::Settings::new(format!("cadpole op (pid {})", std::process::id()));
    settings.when_landed = "evaluate again once it lands; `cadpole op Solve` waits for it".into();
    Ok(Box::new(crate::opt_jobs::ops::LocalHost::new(doc, settings)))
}

/// Run `cadpole op ...`. `args` is everything after the verb.
pub fn run(args: &[String]) -> anyhow::Result<ExitCode> {
    let cmd = command();
    let m = match cmd.try_get_matches_from(std::iter::once("op".to_owned()).chain(args.iter().cloned())) {
        Ok(m) => m,
        // `--help` and `--version` are clap "errors" carrying the text to
        // print; they belong on stdout with exit 0, not on stderr with exit 2.
        Err(e) => {
            let help = matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            );
            e.print()?;
            return Ok(if help { ExitCode::SUCCESS } else { ExitCode::from(2) });
        }
    };
    let Some((name, sub)) = m.subcommand() else {
        if m.get_flag("list") {
            print_list();
            return Ok(ExitCode::SUCCESS);
        }
        print_list();
        return Ok(ExitCode::from(2));
    };
    let info: &MethodInfo = cadpole_ops::method(name).ok_or_else(|| anyhow::anyhow!("unknown operation {name}"))?;
    let md = cadpole_ops::pool().get_message_by_name(&info.input).ok_or_else(|| anyhow::anyhow!("no descriptor for {}", info.input))?;

    let base = match sub.get_one::<String>("json") {
        Some(s) => serde_json::from_str(s).map_err(|e| anyhow::anyhow!("--json: {e}"))?,
        None => json!({}),
    };
    let request = request_json(&md, sub, base)?;
    let doc = sub.get_one::<String>("doc").map(PathBuf::from);
    let token = sub.get_one::<String>("token").cloned();
    let host = host(sub.get_one::<String>("server").map(String::as_str), token, doc.as_ref(), info.needs_session)?;

    let mut out = cadpole_ops::call_json(&info.name, &request, host.as_ref())?;
    // An image field is written to --out (and dropped from the JSON) rather
    // than printed as several megabytes of base64.
    if !info.image_field.is_empty() {
        if let Some(path) = sub.get_one::<String>("out") {
            use base64::Engine as _;
            // `path` says where the image went, so it is there only when
            // one was written: a response without one (a failed render)
            // must not point at a file that is not there.
            if let Some(b64) = out.get(&info.image_field).and_then(|v| v.as_str()) {
                let png = base64::engine::general_purpose::STANDARD.decode(b64)?;
                std::fs::write(path, &png).with_context(|| format!("writing {path}"))?;
                eprintln!("wrote {path}");
                if let Some(o) = out.as_object_mut() {
                    o.remove(&info.image_field);
                    o.insert("path".to_owned(), json!(path));
                }
            }
        }
    }
    if sub.get_flag("compact") {
        println!("{out}");
    } else {
        println!("{}", serde_json::to_string_pretty(&out)?);
    }
    Ok(if response_failed(&info.name, &out) { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

/// Whether an operation's JSON response reports a failure, for the exit
/// code (1 when the document has errors, the JSON still
/// describing them): an error diagnostic anywhere in it -- the response's
/// own, a model's, a sweep cell's -- or, for the two operations whose `ok`
/// is a verdict the way `cadpole test` and `cadpole check` read it, `ok`
/// false (absent, because default fields are skipped). Solve's `ok` is also
/// false while a job is merely pending, so only its diagnostics count.
fn response_failed(method: &str, out: &Value) -> bool {
    fn has_error(v: &Value) -> bool {
        match v {
            Value::Object(m) => {
                let here = m.get("diagnostics").and_then(Value::as_array).is_some_and(|ds| {
                    ds.iter().any(|d| matches!(d.get("severity"), Some(s) if s == "SEVERITY_ERROR" || s == 3))
                });
                here || m.values().any(has_error)
            }
            Value::Array(a) => a.iter().any(has_error),
            _ => false,
        }
    }
    let verdict = matches!(method, "RunTests" | "PrintCheck") && out.get("ok") != Some(&Value::Bool(true));
    verdict || has_error(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exit 1 when the document has errors, the JSON still
    /// describing them. `cadpole op` printed an Export with no `data` and
    /// an error diagnostic, then exited 0, so a pipeline shipped an empty
    /// file; and `op RunTests` passed on a failing test that `cadpole test`
    /// fails on.
    #[test]
    fn a_response_with_an_error_is_a_failed_command() {
        let err = json!({"severity": "SEVERITY_ERROR", "message": "boom"});
        let warn = json!({"severity": "SEVERITY_WARNING", "message": "hm"});
        assert!(response_failed("Export", &json!({"diagnostics": [err]})));
        assert!(!response_failed("Export", &json!({"data": "AAAA", "diagnostics": [warn]})));
        assert!(response_failed("Evaluate", &json!({"model": {"diagnostics": [warn, err]}})), "nested, as Evaluate's model");
        assert!(response_failed("Sweep", &json!({"cells": [{"label": "w=1"}, {"label": "w=2", "diagnostics": [err]}]})));
        assert!(!response_failed("Evaluate", &json!({"model": {}})));
        // `ok` is skipped when false (skip_default_fields), so absent is false.
        assert!(response_failed("RunTests", &json!({"passed": 1, "failed": 1})));
        assert!(!response_failed("RunTests", &json!({"ok": true, "passed": 2})));
        assert!(response_failed("PrintCheck", &json!({"bodies": []})));
        // Solve's `ok` is also false while jobs are merely pending.
        assert!(!response_failed("Solve", &json!({"pending": 1})));

        let Some(bin) = crate::tests::cadpole_binary() else { return };
        let dir = std::env::temp_dir().join(format!("cadpole-op-exit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.star"), "b = box(10, 10, 10)\nc = nosuchfeature(b)\n").unwrap();
        std::fs::write(dir.join("ok.star"), "b = box(10, 10, 10)\n").unwrap();
        let op = |doc: &str| {
            std::process::Command::new(&bin)
                .args(["op", "Export", "--doc", doc, "--format", "EXPORT_FORMAT_STL", "--compact"])
                .current_dir(&dir)
                .output()
                .unwrap()
        };
        let bad = op("bad.star");
        assert_eq!(bad.status.code(), Some(1), "{}", String::from_utf8_lossy(&bad.stderr));
        assert!(String::from_utf8_lossy(&bad.stdout).contains("SEVERITY_ERROR"), "the JSON still says why");
        assert_eq!(op("ok.star").status.code(), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_generated_command_has_one_subcommand_per_operation() {
        let cmd = command();
        let names: Vec<String> = cmd.get_subcommands().map(|c| c.get_name().to_owned()).collect();
        assert_eq!(names, op_names());
        let render = cmd.get_subcommands().find(|c| c.get_name() == "Render").unwrap();
        let flags: Vec<&str> = render.get_arguments().filter_map(|a| a.get_long()).collect();
        // Generated from RenderRequest's own fields, plus --out for the PNG.
        for expected in ["width", "height", "samples", "param", "draft", "out"] {
            assert!(flags.contains(&expected), "Render is missing --{expected}: {flags:?}");
        }
        assert!(flags.contains(&"view"), "camera.preset opted into --view with (cadpole.field).flag");
        assert!(!flags.contains(&"document"), "hidden fields must not become flags");
        // --json/--doc/--server/--token are global, so they live on the parent.
        let globals: Vec<&str> = cmd.get_arguments().filter_map(|a| a.get_long()).collect();
        for expected in ["json", "doc", "server", "token", "list"] {
            assert!(globals.contains(&expected), "`cadpole op` is missing --{expected}: {globals:?}");
        }
    }

    #[test]
    fn flags_become_a_proto3_json_request() {
        let cmd = command();
        let m = cmd
            .try_get_matches_from(["op", "Render", "--width", "320", "--view", "front", "--draft", "--param", "wall=2.4", "--param", "tall=true"])
            .unwrap();
        let sub = m.subcommand_matches("Render").unwrap();
        let md = cadpole_ops::pool().get_message_by_name("cadpole.v1.RenderRequest").unwrap();
        let req = request_json(&md, sub, json!({"height": 240})).unwrap();
        assert_eq!(req["width"], 320.0);
        assert_eq!(req["height"], 240, "--json supplies what the flags do not");
        assert_eq!(req["camera"]["preset"], "front");
        assert_eq!(req["draft"], true);
        assert_eq!(req["param_overrides"]["wall"]["number"], 2.4);
        assert_eq!(req["param_overrides"]["tall"]["boolean"], true);
    }

    /// `--layer` is spelt as `cadpole render` spells it and lands as a
    /// `LayerState` per flag; JSON is still taken for a client that has it.
    #[test]
    fn layer_flags_become_layer_states() {
        let cmd = command();
        let m = cmd
            .try_get_matches_from(["op", "Render", "--layer", "envelope=off", "--layer", "terrain=0.4", "--layer", r#"{"name":"site","visible":true}"#])
            .unwrap();
        let sub = m.subcommand_matches("Render").unwrap();
        let md = cadpole_ops::pool().get_message_by_name("cadpole.v1.RenderRequest").unwrap();
        let req = request_json(&md, sub, json!({})).unwrap();
        assert_eq!(req["layers"], json!([{"name": "envelope", "visible": false}, {"name": "terrain", "visible": true, "opacity": 0.4}, {"name": "site", "visible": true}]));
        let bad = command().try_get_matches_from(["op", "Render", "--layer", "envelope=maybe"]).unwrap();
        assert!(request_json(&md, bad.subcommand_matches("Render").unwrap(), json!({})).is_err());
    }

    #[test]
    fn sweep_flags_carry_value_lists_and_their_order() {
        let cmd = command();
        let m = cmd.try_get_matches_from(["op", "Sweep", "--param", "w=30,40,50", "--param", "h=10,20"]).unwrap();
        let sub = m.subcommand_matches("Sweep").unwrap();
        let md = cadpole_ops::pool().get_message_by_name("cadpole.v1.SweepRequest").unwrap();
        let req = request_json(&md, sub, json!({})).unwrap();
        assert_eq!(req["params"]["w"]["values"].as_array().unwrap().len(), 3);
        assert_eq!(req["params"]["h"]["values"][1]["number"], 20.0);
        assert_eq!(req["param_order"], json!(["w", "h"]));
    }

    #[test]
    fn the_listing_is_one_line_per_operation() {
        for m in cadpole_ops::registry() {
            let line = headline(&m.summary(), 88);
            assert!(!line.contains('\n'), "{}: {line}", m.name);
            assert!(line.chars().count() <= 91, "{}: {line}", m.name);
        }
        assert_eq!(headline("One. Two.", 88), "One.");
        assert_eq!(headline("Values, e.g. two of them. And more.", 88), "Values, e.g. two of them.");
        assert_eq!(headline("A very long sentence indeed", 6), "A very...");
    }

    #[test]
    fn enum_values_accept_the_short_spelling() {
        let cmd = command();
        let m = cmd.try_get_matches_from(["op", "Export", "--format", "obj"]).unwrap();
        let sub = m.subcommand_matches("Export").unwrap();
        let md = cadpole_ops::pool().get_message_by_name("cadpole.v1.ExportRequest").unwrap();
        let req = request_json(&md, sub, json!({})).unwrap();
        assert_eq!(req["format"], "EXPORT_FORMAT_OBJ");
    }

    /// The plate-image fields of `Export` are flags like
    /// any other, and the response's contact sheet is the operation's image
    /// field, so `cadpole op Export --out sheet.png` writes it and the MCP
    /// front-end hands it over as an image block.
    #[test]
    fn export_plate_image_flags_round_trip_and_the_sheet_is_its_image() {
        let cmd = command();
        let m = cmd
            .try_get_matches_from([
                "op",
                "Export",
                "--format",
                "threemf",
                "--flavor",
                "bambu",
                "--plate-images",
                "--plate-image-width",
                "800",
                "--plate-image-height",
                "600",
                "--plate-image-background",
                "studio",
                "--plate-image-backend",
                "path_trace",
                "--out",
                "sheet.png",
            ])
            .unwrap();
        let sub = m.subcommand_matches("Export").unwrap();
        let md = cadpole_ops::pool().get_message_by_name("cadpole.v1.ExportRequest").unwrap();
        let req = request_json(&md, sub, json!({})).unwrap();
        assert_eq!(req["format"], "EXPORT_FORMAT_THREEMF");
        assert_eq!(req["flavor"], "bambu");
        assert_eq!(req["plate_images"], true);
        assert_eq!(req["plate_image_width"], 800.0);
        assert_eq!(req["plate_image_height"], 600.0);
        assert_eq!(req["plate_image_background"], "studio");
        assert_eq!(req["plate_image_backend"], "RENDER_BACKEND_PATH_TRACE");
        assert_eq!(sub.get_one::<String>("out").map(String::as_str), Some("sheet.png"));
        let export = cadpole_ops::registry().iter().find(|m| m.name == "Export").unwrap();
        assert_eq!(export.image_field, "plate_sheet");
        // And the request decodes to the typed message the engine reads.
        use cadpole_engine::cadpole_proto::prost::Message as _;
        let bytes = cadpole_ops::json_to_proto("cadpole.v1.ExportRequest", &req).unwrap();
        let typed = cadpole_engine::cadpole_proto::v1::ExportRequest::decode(bytes.as_slice()).unwrap();
        assert!(typed.plate_images);
        assert_eq!((typed.plate_image_width, typed.plate_image_height), (800, 600));
        assert_eq!(typed.plate_image_backend(), cadpole_engine::cadpole_proto::v1::RenderBackend::PathTrace);
    }
}
