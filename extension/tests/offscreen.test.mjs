import assert from "node:assert/strict";
import test from "node:test";

// The offscreen recorder shares the extension's message bus with the service worker. Messages
// from the popup are broadcast to every extension context, so only messages explicitly addressed
// to the offscreen document may be acted on here.
const listeners = [];
globalThis.chrome = {
  runtime: {
    onMessage: {
      addListener(listener) {
        listeners.push(listener);
      },
    },
  },
};
await import("../src/offscreen.js");

const listener = listeners[0];
assert.equal(typeof listener, "function", "offscreen registers a message listener");

function dispatch(message) {
  return new Promise((resolve) => {
    const kept = listener(message, {}, (response) => resolve({ kept, response }));
    if (kept === false) resolve({ kept, response: null });
  });
}

test("a broadcast stop-recording is not handled by the recorder", async () => {
  const { kept, response } = await dispatch({ type: "stop-recording" });
  assert.equal(kept, false);
  assert.equal(response, null);
});

test("a targeted stop-recording reaches the recorder", async () => {
  const { kept, response } = await dispatch({ type: "stop-recording", target: "offscreen" });
  assert.equal(kept, true);
  assert.equal(response.ok, false);
  assert.match(response.error, /No recording is running/);
});

test("an unrelated broadcast message is not handled by the recorder", async () => {
  const { kept, response } = await dispatch({ type: "start-recording", tabId: 7 });
  assert.equal(kept, false);
  assert.equal(response, null);
});
