import fs from "node:fs";
import test from "node:test";
import assert from "node:assert/strict";
import {
  buildAnnotationsJson,
  buildReviewMarkdown,
  buildZip,
  frameFileName,
  traceSummary,
} from "../src/lib/package.js";

const trace = {
  version: 1,
  kind: "browser-flow",
  started_at: "2026-11-03T10:00:00.000Z",
  duration_ms: 2000,
  events: [
    { t: 100, kind: "action", action: "click", selector: "#save", label: "Save" },
    { t: 200, kind: "network", method: "POST", url: "https://app.test/api", status: 500 },
    { t: 300, kind: "console", level: "error", text: "boom" },
    { t: 400, kind: "marker", label: "total stale" },
  ],
};

const annotations = [
  {
    id: "a1",
    type: "rect",
    color: "#e11d48",
    width: 3,
    start: { x: 10, y: 20 },
    end: { x: 110, y: 70 },
    at: 0.5,
  },
  {
    id: "a2",
    type: "text",
    color: "#e11d48",
    width: 3,
    x: 30,
    y: 40,
    text: "Should be $80.00",
    at: 0.5,
  },
];

test("a trace summary counts the server-visible kinds and highlights failures", () => {
  const { summary, highlights } = traceSummary(trace);
  assert.deepEqual(summary, {
    console: 1,
    console_errors: 1,
    network: 1,
    network_failed: 1,
    actions: 1,
    markers: 1,
  });
  assert.equal(highlights.length, 2);
  assert.match(highlights[0].text, /POST https:\/\/app\.test\/api → 500/);
  assert.equal(highlights[1].text, "boom");
});

test("review.md contains the request, media, trace and numbered annotations", () => {
  const markdown = buildReviewMarkdown({
    id: "local-1",
    title: "Checkout",
    url: "https://shop.example/checkout",
    note: "total is stale",
    media: { kind: "video", duration_ms: 2000, width: 880, height: 800 },
    width: 880,
    height: 800,
    annotations,
    frames: [{ at: 0.5 }],
    trace,
  });
  assert.match(markdown, /# Visual review: Checkout/);
  assert.match(markdown, /https:\/\/shop\.example\/checkout/);
  assert.match(markdown, /## Browser trace/);
  assert.match(markdown, /1 actions/);
  assert.match(markdown, /1\. \*\*Rectangle\*\* @ 00:00\.500/);
  assert.match(markdown, /2\. \*\*Text note\*\* @ 00:00\.500 — Should be \$80\.00/);
  assert.match(markdown, /AI-PROMPT\.md/);
});

test("annotations.json keeps the RedLine shape and frame names", () => {
  const payload = buildAnnotationsJson({
    id: "local-1",
    title: "Checkout",
    url: "https://shop.example/checkout",
    note: "total is stale",
    media: null,
    width: 880,
    height: 800,
    annotations,
    frames: [{ at: 0.5 }],
    trace: null,
  });
  assert.equal(payload.session, "local-1");
  assert.equal(payload.image.file, "annotated.png");
  assert.equal(payload.annotations.length, 2);
  assert.equal(payload.frames[0].file, "frames/0.500.png");
  assert.equal(payload.trace, undefined);
  assert.equal(frameFileName(1.25), "frames/1.250.png");
});

function readZip(blob) {
  return blob.arrayBuffer().then((buffer) => {
    const bytes = new Uint8Array(buffer);
    const view = new DataView(buffer);
    let eocd = -1;
    for (let i = bytes.length - 22; i >= 0; i -= 1) {
      if (view.getUint32(i, true) === 0x06054b50) {
        eocd = i;
        break;
      }
    }
    assert.notEqual(eocd, -1, "EOCD not found");
    const count = view.getUint16(eocd + 10, true);
    const centralOffset = view.getUint32(eocd + 16, true);
    const files = new Map();
    let cursor = centralOffset;
    for (let index = 0; index < count; index += 1) {
      assert.equal(view.getUint32(cursor, true), 0x02014b50, "central header");
      const crc = view.getUint32(cursor + 16, true);
      const size = view.getUint32(cursor + 24, true);
      const nameLength = view.getUint16(cursor + 28, true);
      const extraLength = view.getUint16(cursor + 30, true);
      const commentLength = view.getUint16(cursor + 32, true);
      const localOffset = view.getUint32(cursor + 42, true);
      const name = new TextDecoder().decode(bytes.slice(cursor + 46, cursor + 46 + nameLength));
      assert.equal(view.getUint32(localOffset, true), 0x04034b50, "local header");
      const localNameLength = view.getUint16(localOffset + 26, true);
      const localExtraLength = view.getUint16(localOffset + 28, true);
      const dataStart = localOffset + 30 + localNameLength + localExtraLength;
      files.set(name, { crc, size, data: bytes.slice(dataStart, dataStart + size) });
      cursor += 46 + nameLength + extraLength + commentLength;
    }
    return files;
  });
}

test("the ZIP fallback is a readable store-only archive", async () => {
  const blob = await buildZip([
    { name: "review.md", data: "# Review\n" },
    { name: "frames/0.500.png", data: new Blob([new Uint8Array([1, 2, 3])]) },
  ]);
  assert.equal(blob.type, "application/zip");
  const files = await readZip(blob);
  assert.ok(files.has("review.md"));
  assert.ok(files.has("frames/0.500.png"));
  assert.equal(new TextDecoder().decode(files.get("review.md").data), "# Review\n");
  assert.deepEqual([...files.get("frames/0.500.png").data], [1, 2, 3]);
  assert.equal(files.get("review.md").crc, 0xb67b0ceb);
});

test("the action opens the side panel and exposes the overlay to the page", () => {
  const manifest = JSON.parse(
    fs.readFileSync(new URL("../manifest.json", import.meta.url), "utf8"),
  );
  assert.equal(manifest.action.default_popup, undefined);
  assert.equal(manifest.side_panel.default_path, "src/sidepanel.html");
  const resources = (manifest.web_accessible_resources || []).flatMap((entry) => entry.resources);
  assert.ok(resources.includes("src/overlay.html"));
  assert.ok(resources.includes("src/overlay.js"));
});
