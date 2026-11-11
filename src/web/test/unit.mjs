// Entry point of `//src/web:unit_test`: the pure modules of the web UI on
// node's own test runner (`node:test`), with no browser and no framework.
//
// What belongs here is logic that is a function of its inputs -- the build
// plan, the job runner's policies, the shape of the stylesheet -- where a
// Playwright test would spend forty seconds starting servers to check a
// value. What does not belong here is anything that needs a DOM: that is
// `e2e/`. The modules under test are the transpiled output of `:ts`, i.e. the
// same JavaScript esbuild bundles into app.js.
//
// One process, so a failure in any file fails the target: `node:test` runs
// every test registered by the modules imported below and sets the exit code.

import "./apidoc.test.mjs";
import "./attachments.test.mjs";
import "./branding.test.mjs";
import "./devserver-static.test.mjs";
import "./drawing-buffer.test.mjs";
import "./engine-policy.test.mjs";
import "./event-stream.test.mjs";
import "./evaluator.test.mjs";
import "./failure.test.mjs";
import "./input.test.mjs";
import "./insets.test.mjs";
import "./jobs.test.mjs";
import "./jobs-coalesce.test.mjs";
import "./opening.test.mjs";
import "./param-sync.test.mjs";
import "./params.test.mjs";
import "./params-panel.test.mjs";
import "./reply-stream.test.mjs";
import "./section.test.mjs";
import "./session-writes.test.mjs";
import "./share-hash.test.mjs";
import "./styles.test.mjs";
import "./units.test.mjs";
import "./viewer.test.mjs";
