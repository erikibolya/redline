import test from "node:test";
import assert from "node:assert/strict";
import { normalizeServerUrl, originPattern } from "../src/lib/config.js";
import { projectChoices } from "../src/lib/projects.js";

test("server URLs are normalized to an origin with an optional path", () => {
  assert.equal(normalizeServerUrl("https://redline.ibolya.cz/"), "https://redline.ibolya.cz");
  assert.equal(normalizeServerUrl(" http://127.0.0.1:8788/ "), "http://127.0.0.1:8788");
  assert.equal(
    normalizeServerUrl("https://example.test/redline/"),
    "https://example.test/redline",
  );
});

test("bad or non-http server URLs are refused", () => {
  assert.throws(() => normalizeServerUrl(""), /required/i);
  assert.throws(() => normalizeServerUrl("javascript:alert(1)"), /http/i);
  assert.throws(() => normalizeServerUrl("not a url"), /invalid/i);
});

test("the permission pattern is the server origin", () => {
  assert.equal(
    originPattern("https://redline.ibolya.cz/some/path"),
    "https://redline.ibolya.cz/*",
  );
});

test("project choices keep Inbox first, dedupe the server Inbox and keep a saved project", () => {
  const choices = projectChoices(
    [
      { slug: "inbox", name: "Inbox" },
      { slug: "redline", name: "RedLine" },
      { slug: "redline", name: "Duplicate" },
      { slug: "", name: "Ignored" },
    ],
    "actiunit-au-net",
  );
  assert.deepEqual(
    choices.map((choice) => choice.value),
    ["", "redline", "actiunit-au-net"],
  );
  assert.equal(choices[1].label, "RedLine");
  assert.equal(choices[2].label, "actiunit-au-net");
});

test("an inbox save is shown as the empty Inbox choice", () => {
  assert.deepEqual(
    projectChoices([{ slug: "redline", name: "RedLine" }], "inbox").map((choice) => choice.value),
    ["", "redline"],
  );
});
