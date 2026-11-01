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

test("video upload sends the clip, poster and trace as multipart", async () => {
  const calls = [];
  const original = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init });
    return new Response(JSON.stringify({ meta: { id: "v1" }, review_url: "http://x/s/v1" }), {
      status: 200,
    });
  };
  try {
    const api = new RedLineApi({ serverUrl: "https://redline.test", token: "tok" });
    const trace = { version: 1, kind: "browser-flow", events: [] };
    await api.uploadVideo({
      video: new Blob(["webm"], { type: "video/webm" }),
      poster: new Blob(["png"], { type: "image/png" }),
      trace,
      title: "Flow",
      url: "https://app.test",
      project: "daneek",
      durationMs: 1200,
      width: 1280,
      height: 720,
    });
    assert.equal(calls[0].url, "https://redline.test/api/sessions/video");
    const form = calls[0].init.body;
    assert.ok(form instanceof FormData);
    assert.equal(form.get("title"), "Flow");
    assert.equal(form.get("duration_ms"), "1200");
    assert.ok(form.get("video") instanceof Blob);
    assert.equal(await form.get("trace").text(), JSON.stringify(trace));
  } finally {
    globalThis.fetch = original;
  }
});

test("video submit sends annotations, contact sheet and one file per moment", async () => {
  const calls = [];
  const original = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init });
    return new Response(JSON.stringify({ ok: true }), { status: 200 });
  };
  try {
    const api = new RedLineApi({ serverUrl: "https://redline.test", token: "tok" });
    await api.submitVideo("v1", {
      annotations: [{ type: "rect", at: 1.25 }],
      width: 1280,
      height: 720,
      contactSheet: new Blob(["png"], { type: "image/png" }),
      frames: [{ at: 1.25, blob: new Blob(["frame"], { type: "image/png" }) }],
      note: "here",
    });
    const form = calls[0].init.body;
    assert.ok(form instanceof FormData);
    const annotations = JSON.parse(form.get("annotations"));
    assert.equal(annotations.annotations[0].at, 1.25);
    assert.equal(annotations.note, "here");
    assert.ok(form.get("png") instanceof Blob);
    assert.equal(form.getAll("frames").length, 1);
    assert.equal(form.getAll("frames")[0].name, "1.250.png");
  } finally {
    globalThis.fetch = original;
  }
});
