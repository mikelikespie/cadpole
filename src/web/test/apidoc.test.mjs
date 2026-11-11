// The API reference as the web app renders it, whichever engine answered:
// a constant reads as `name: type` from the server too, and the `/` and `*`
// markers stay in the signature but are not rows of the parameters table.

import assert from "node:assert/strict";
import { test } from "node:test";
import { apiDocFromResponse } from "../src/remote-engine.js";
import { signature, tableParams } from "../src/ui/apidoc.js";

const param = (name, required = true) => ({ name, type: name === "/" || name === "*" ? "" : "float", doc: "", required, defaultValue: "" });

test("a constant from the server reads as a value, not a call", () => {
  const doc = apiDocFromResponse({
    functions: [
      { name: "math.pi", doc: "", params: [], returns: "float", onType: "", constant: true },
      { name: "cylinder", doc: "", params: [param("r"), param("h"), param("/"), param("*")], returns: "Body", onType: "", constant: false },
    ],
    types: [],
  });
  assert.equal(signature(doc.functions[0]), "math.pi: float");
  assert.equal(signature(doc.functions[1]), "cylinder(r, h, /, *) -> Body");
});

test("the signature markers are not parameters", () => {
  const rows = tableParams([param("r"), param("h"), param("/"), param("*"), param("name", false)].map((p) => ({ ...p, default_value: p.defaultValue })));
  assert.deepEqual(rows.map((p) => p.name), ["r", "h", "name"]);
});
