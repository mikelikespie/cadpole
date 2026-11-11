// What the failure reporter does when the server will not take a report.
//
// Two different "no": a report the server *rejected* (a field it dislikes, a
// schema that moved) is worth retrying with a reduced body, because knowing
// that a page crashed is better than knowing nothing. A report the server
// *refused* -- 401 or 403, which in practice means `cadpole serve` restarted and
// this tab's token is last run's -- is not: the next one gets the same
// answer, and each user action was costing three dead requests, one for the
// write and two for the report about the write.

import assert from "node:assert/strict";
import { test } from "node:test";
import { FailureReporter, ReportRefused } from "../src/failure.js";

const context = {
  revision: () => 1,
  params: () => ({}),
  scriptSource: () => "",
  engineBuild: () => "test",
  engineKind: () => "worker",
};

/** Let the reporter's own `void this.flush(...)` run to completion. */
const settle = () => new Promise((r) => setTimeout(r, 0));

const failure = (message) => ({ kind: "CLIENT_FAILURE_KIND_EXCEPTION", message, operation: "test" });

test("a refused report is final: no reduced retry, and no further reports", async () => {
  const bodies = [];
  const reporter = new FailureReporter({
    enabled: true,
    context,
    send: (body) => {
      bodies.push(body);
      return Promise.reject(new ReportRefused("the server refused this tab's token (401)"));
    },
  });

  reporter.report(failure("first"));
  await settle();
  assert.equal(bodies.length, 1, "a refusal must not be retried with a reduced body");

  reporter.report(failure("second"));
  await settle();
  assert.equal(bodies.length, 1, "reporting should have stopped after the refusal");
});

test("a rejected report is still retried once, with only the fields that are certainly valid", async () => {
  const bodies = [];
  const reporter = new FailureReporter({
    enabled: true,
    context,
    send: (body) => {
      bodies.push(JSON.parse(body));
      // `null` is "the server would not take this one", which is the case the
      // reduced retry exists for.
      return Promise.resolve(bodies.length > 1 ? { accepted: true } : null);
    },
  });

  reporter.report(failure("rejected once"));
  await settle();
  assert.equal(bodies.length, 2);
  assert.equal(bodies[0].message, "rejected once");
  assert.equal(bodies[1].message, "rejected once");
  // The reduced body carries only what cannot be argued with.
  assert.deepEqual(Object.keys(bodies[1]).sort(), ["kind", "message", "operation", "surface"]);
});

test("an accepted report is sent once", async () => {
  const bodies = [];
  const reporter = new FailureReporter({
    enabled: true,
    context,
    send: (body) => {
      bodies.push(body);
      return Promise.resolve({ accepted: true });
    },
  });
  reporter.report(failure("fine"));
  await settle();
  assert.equal(bodies.length, 1);
});
