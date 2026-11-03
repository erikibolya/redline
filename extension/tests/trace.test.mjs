import test from "node:test";
import assert from "node:assert/strict";
import { consoleText, makeTrace, redactText, redactUrl } from "../src/lib/trace.js";

test("URL query tokens are redacted without touching unrelated parameters", () => {
  const url = redactUrl("https://app.test/x?token=abc&monkey=1&access_token=zzz&q=ok");
  assert.match(url, /token=%5Bredacted%5D/);
  assert.match(url, /access_token=%5Bredacted%5D/);
  assert.match(url, /monkey=1/);
  assert.match(url, /q=ok/);
  assert.doesNotMatch(url, /abc|zzz/);
});

test("text redaction hides bearer tokens and assignments", () => {
  const text = redactText("Authorization: Bearer abc.def-123 password=hunter2 code=9999");
  assert.doesNotMatch(text, /abc\.def|hunter2|9999/);
  assert.match(text, /Bearer \[redacted\]/);
  assert.match(text, /password=\[redacted\]/);
});

test("console arguments become bounded text", () => {
  assert.equal(consoleText(["failed", 42, { value: "x" }]), "failed 42 x");
  const long = consoleText(["x".repeat(3000)]);
  assert.ok(long.length <= 2048);
});

test("a trace has the server-validated top-level shape", () => {
  const trace = makeTrace({
    startedAt: Date.UTC(2026, 10, 1, 14, 0, 0),
    durationMs: 1234.4,
    page: { url: "https://app.test/?token=secret", title: "Checkout" },
    events: [{ t: 10, kind: "marker", label: "start" }],
  });
  assert.equal(trace.version, 1);
  assert.equal(trace.kind, "browser-flow");
  assert.equal(trace.duration_ms, 1234);
  assert.equal(trace.events.length, 1);
  assert.equal(trace.redaction.bodies, false);
  assert.match(trace.page.url, /token=%5Bredacted%5D/);
});
