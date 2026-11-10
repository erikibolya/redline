import test from "node:test";
import assert from "node:assert/strict";
import {
  normalizeServerUrl,
  originPattern,
  projectForCapture,
  rememberProject,
} from "../src/lib/config.js";

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


function fakeChromeStorage() {
  const store = new Map();
  globalThis.chrome = {
    storage: {
      local: {
        async get(key) {
          return { [key]: store.get(key) };
        },
        async set(values) {
          for (const [key, value] of Object.entries(values)) store.set(key, value);
        },
      },
    },
  };
  return store;
}

test("the last project is remembered per page origin and as the global fallback", async () => {
  const store = fakeChromeStorage();
  assert.equal(await projectForCapture("https://app.test/one"), null);

  await rememberProject("https://app.test/one", "e2e-repo");
  assert.equal(await projectForCapture("https://app.test/two"), "e2e-repo");
  assert.equal(await projectForCapture("https://other.test/one"), "e2e-repo");

  await rememberProject("https://other.test/one", "other-project");
  assert.equal(await projectForCapture("https://app.test/three"), "e2e-repo");
  assert.equal(await projectForCapture("https://third.test/one"), "other-project");

  await rememberProject("https://app.test/four", "");
  assert.equal(await projectForCapture("https://app.test/five"), "");
  assert.equal(await projectForCapture("https://third.test/two"), "");
  assert.equal(await projectForCapture("chrome://extensions"), "");
  assert.equal(store.get("redlineProjectMemory").byOrigin["https://app.test"], "");
  delete globalThis.chrome;
});
