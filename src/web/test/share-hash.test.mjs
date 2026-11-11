// The static app's share link, `#ex=<id>&p=<name>:<value>,...`.
//
// The link is read before anything else on start-up (LocalApi's
// constructor), so a value it cannot decode must not take the app down with
// it: a `%` cut off by a chat client, a hand-typed `100%`, `%zz`. Before this
// was fixed each of them put up "Cadpole could not start / URIError".

import assert from "node:assert/strict";
import { test } from "node:test";
import { parseHash } from "../src/api.js";

test("a malformed percent-escape in an override does not throw", () => {
  for (const h of ["#ex=plate-boss&p=width:%", "#ex=plate-boss&p=width:%zz", "#ex=plate-boss&p=label:100%"]) {
    const got = parseHash(h);
    assert.equal(got.exampleId, "plate-boss", h);
  }
  // `100%` is what was typed, and nothing else it could mean.
  assert.deepEqual(parseHash("#ex=e&p=label:100%25").overrides, { label: "100%" });
  assert.deepEqual(parseHash("#ex=e&p=label:100%,width:12").overrides, { label: "100%", width: 12 });
});

test("a link the app wrote still round-trips", () => {
  // writeHash encodes a string value, then URLSearchParams encodes it again.
  const params = new URLSearchParams();
  params.set("ex", "e");
  params.set("p", `label:${encodeURIComponent("a,b:50%")},width:12,on:true`);
  assert.deepEqual(parseHash(`#${params.toString()}`).overrides, { label: "a,b:50%", width: 12, on: true });
});
