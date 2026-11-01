import test from "node:test";
import assert from "node:assert/strict";
import { ApiError, RedLineApi } from "../src/lib/api.js";

test("image-session creation sends the bearer token and URL-encoded image payload", async () => {
  const calls = [];
  const original = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init });
    return new Response(JSON.stringify({ meta: { id: "s1" }, review_url: "http://x/s/s1" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  try {
    const api = new RedLineApi({ serverUrl: "https://redline.test/", token: "tok" });
    const created = await api.createImageSession({
      imageDataUrl: "data:image/png;base64,AAA",
      title: "Checkout",
      url: "https://app.test/checkout",
      project: "daneek",
    });
    assert.equal(created.meta.id, "s1");
    assert.equal(calls.length, 1);
    assert.equal(calls[0].url, "https://redline.test/api/sessions");
    assert.equal(calls[0].init.headers.Authorization, "Bearer tok");
    const body = JSON.parse(calls[0].init.body);
    assert.equal(body.image_base64, "data:image/png;base64,AAA");
    assert.equal(body.project, "daneek");
    assert.equal(body.agent, "extension");
  } finally {
    globalThis.fetch = original;
  }
});

test("submit sends annotations, dimensions, png and note", async () => {
  const calls = [];
  const original = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init });
    return new Response(JSON.stringify({ ok: true }), { status: 200 });
  };
  try {
    const api = new RedLineApi({ serverUrl: "https://redline.test", token: "tok" });
    await api.submit("s1", {
      annotations: [{ type: "rect" }],
      width: 100,
      height: 50,
      png: "data:image/png;base64,AAA",
      note: "look here",
    });
    const body = JSON.parse(calls[0].init.body);
    assert.equal(calls[0].url, "https://redline.test/api/sessions/s1/annotations");
    assert.equal(body.annotations.length, 1);
    assert.equal(body.width, 100);
    assert.equal(body.note, "look here");
  } finally {
    globalThis.fetch = original;
  }
});

test("API errors keep the status and server message", async () => {
  const original = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(JSON.stringify({ error: "unauthorized" }), { status: 401 });
  try {
    const api = new RedLineApi({ serverUrl: "https://redline.test", token: "bad" });
    await assert.rejects(
      () => api.health(),
      (error) => error instanceof ApiError && error.status === 401 && error.message === "unauthorized",
    );
  } finally {
    globalThis.fetch = original;
  }
});
