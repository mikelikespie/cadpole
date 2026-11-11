//! `RunTests`: the runner behind `cadpole test`, the `run_tests` MCP tool and
//! the route. The work is `cadpole_script::run_tests`;
//! this is the proto adapter, kept as thin as `print_check`'s.

use std::collections::HashMap;

use cadpole_core::context::{Diagnostic, Span};
use cadpole_proto::v1 as pb;
use cadpole_script::{TestOutcome, point_label};

use crate::limits;

pub fn run_tests(req: &pb::RunTestsRequest) -> anyhow::Result<pb::RunTestsResponse> {
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("test: no document"))?;
    let mut input =
        crate::eval_input(doc, &HashMap::new(), req.tolerance, req.boolean_tolerance, &HashMap::new(), limits::default_limits());
    // One memo across the base evaluation and every sweep cell: a
    // cell re-runs only the ops downstream of the parameters it moves.
    input.memo = Some(std::sync::Arc::new(cadpole_core::memo::Memo::new()));
    let run = cadpole_script::run_tests(input, &req.names, req.threads as usize);
    let diagnostics: Vec<pb::Diagnostic> = run.base.ctx.diagnostics.iter().map(Diagnostic::to_proto).collect();
    let built = !diagnostics.iter().any(|d| d.severity() == pb::Severity::Error);
    let results: Vec<pb::TestResult> = run.outcomes.iter().map(result_to_proto).collect();
    let passed = results.iter().filter(|r| r.ok).count() as u32;
    let failed = results.len() as u32 - passed;
    Ok(pb::RunTestsResponse {
        results,
        diagnostics,
        ok: built && failed == 0,
        passed,
        failed,
        declared: run.base.tests.len() as u32,
    })
}

fn result_to_proto(o: &TestOutcome) -> pb::TestResult {
    pb::TestResult {
        name: o.name.clone(),
        ok: o.ok,
        message: o.message.clone(),
        span: o.span.as_ref().map(Span::to_proto),
        params: o.point.iter().map(|(k, v)| (k.clone(), v.to_proto())).collect(),
        label: point_label(&o.point),
        value: o.value,
        eval_ms: o.eval_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> pb::Document {
        crate::document_from_scripts("t", vec![("main.star".into(), src.into())])
    }

    /// The response carries what `cadpole test` prints: each result's name,
    /// verdict, message, sweep point (as a map and as the runner's label),
    /// returned value and the counts, and `ok` is the whole verdict.
    #[test]
    fn run_tests_reports_each_result_with_its_sweep_point() {
        let src = "w = param(\"w\", 10)\nb = box(w, 10, 10, name=\"b\")\n\
                   def solid():\n    assert_solid(b)\n\
                   def vol():\n    assert_true(w < 30, msg=\"too wide\")\n    return b.volume()\n\
                   test(\"a solid\", solid)\n\
                   test(\"volume\", vol, sweep={\"w\": [10, 20, 30]}, monotone=\"increasing\")\n";
        let resp = run_tests(&pb::RunTestsRequest { document: Some(doc(src)), ..Default::default() }).unwrap();
        assert!(!resp.ok);
        assert_eq!((resp.declared, resp.passed, resp.failed), (2, 3, 1));
        assert_eq!(resp.results.len(), 4);
        assert!(resp.results[0].ok && resp.results[0].name == "a solid" && resp.results[0].label.is_empty());
        let bad = &resp.results[3];
        assert_eq!((bad.ok, bad.name.as_str(), bad.label.as_str()), (false, "volume", "w=30"));
        assert_eq!(bad.message, "assert_true failed: too wide");
        assert_eq!(bad.params.get("w").and_then(|v| v.value.clone()), Some(pb::param_value::Value::Number(30.0)));
        assert_eq!(bad.span.as_ref().map(|s| s.start_line), Some(6));
        assert_eq!(resp.results[1].value.map(f64::round), Some(1000.0));

        // A document that does not build: diagnostics, no results, not ok.
        let resp = run_tests(&pb::RunTestsRequest { document: Some(doc("b = box(-1, 1, 1)\n")), ..Default::default() }).unwrap();
        assert!(!resp.ok && resp.results.is_empty());
        assert!(resp.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error));

        // Only the named test runs, and an override travels with the document.
        let mut d = doc(src);
        d.param_overrides.insert("w".into(), pb::ParamValue { value: Some(pb::param_value::Value::Number(5.0)) });
        let resp =
            run_tests(&pb::RunTestsRequest { document: Some(d), names: vec!["a solid".into()], ..Default::default() })
                .unwrap();
        assert!(resp.ok, "{:?}", resp.results);
        assert_eq!(resp.results.len(), 1);
    }
}
