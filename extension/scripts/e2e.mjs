/**
 * Extension end-to-end test. Assumes a loaded unpacked extension with host permission for
 * REDLINE_BASE, a running RedLine server and a Chromium CDP endpoint.
 *
 *   REDLINE_BASE=http://127.0.0.1:8788 REDLINE_TOKEN=e2e-token CHROME_URL=http://127.0.0.1:9222 \
 *     node extension/scripts/e2e.mjs
 */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";

const BASE = (process.env.REDLINE_BASE || "http://127.0.0.1:8788").replace(/\/+$/, "");
const TOKEN = process.env.REDLINE_TOKEN || "e2e-token";
const CHROME = process.env.CHROME_URL || "http://127.0.0.1:9222";
const repo = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));

function authed(init = {}) {
  return {
    ...init,
    headers: {
      Authorization: `Bearer ${TOKEN}`,
      ...(init.headers || {}),
    },
  };
}

async function json(pathname, init = {}) {
  const response = await fetch(`${BASE}${pathname}`, authed(init));
  const data = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(`${init.method || "GET"} ${pathname}: ${response.status} ${JSON.stringify(data)}`);
  return data;
}

async function extensionId(browser) {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    const targets = await (await fetch(`${CHROME}/json/list`)).json();
    const worker = targets.find(
      (target) => target.type === "service_worker" && target.url.includes("background.js"),
    );
    if (worker) return new URL(worker.url).host;
    if (attempt === 4) {
      const page = await browser.newPage();
      await page.goto("chrome://extensions/");
      await new Promise((resolve) => setTimeout(resolve, 500));
      await page.close();
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("extension service worker not found");
}

async function configure(browser, id) {
  const page = await browser.newPage();
  await page.goto(`chrome-extension://${id}/src/options.html`, { waitUntil: "domcontentloaded" });
  await page.evaluate(({ base, token }) => {
    document.querySelector("#server-url").value = base;
    document.querySelector("#token").value = token;
  }, { base: BASE, token: TOKEN });
  await page.click("#save");
  await page.waitForFunction(
    () => document.querySelector("#status")?.textContent.includes("Saved"),
    { timeout: 10000 },
  );
  await page.close();
}

async function createImageSession() {
  const image = fs.readFileSync(path.join(repo, "desktop/app/icons/128x128.png")).toString("base64");
  const created = await json("/api/sessions", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      title: "Extension image e2e",
      url: "https://app.test/checkout",
      image_base64: `data:image/png;base64,${image}`,
    }),
  });
  return created.meta.id;
}

async function createVideoSession() {
  const video = fs.readFileSync(path.join(repo, "extension/tests/fixtures/flow.webm"));
  const trace = fs.readFileSync(path.join(repo, "extension/tests/fixtures/trace.json"));
  const form = new FormData();
  form.append("title", "Extension video e2e");
  form.append("url", "https://app.test/checkout");
  form.append("duration_ms", "1500");
  form.append("width", "320");
  form.append("height", "180");
  form.append("video", new Blob([video], { type: "video/webm" }), "flow.webm");
  form.append("trace", new Blob([trace], { type: "application/json" }), "trace.json");
  const created = await json("/api/sessions/video", { method: "POST", body: form });
  return created.meta.id;
}

async function openCapture(browser, id, sessionId, kind) {
  const page = await browser.newPage();
  await page.goto(`chrome-extension://${id}/src/sidepanel.html`, { waitUntil: "domcontentloaded" });
  await page.evaluate(
    async ({ sessionId, base, kind }) => {
      await chrome.storage.session.set({
        activeCapture: {
          sessionId,
          serverUrl: base,
          reviewUrl: `${base}/s/${sessionId}?t=e2e-token`,
          title: kind === "video" ? "Extension video e2e" : "Extension image e2e",
          kind,
        },
      });
    },
    { sessionId, base: BASE, kind },
  );
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#workspace:not([hidden])", { timeout: 15000 });
  return page;
}

async function drawRect(page) {
  const box = await page.$eval("#canvas", (canvas) => {
    const rect = canvas.getBoundingClientRect();
    return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
  });
  await page.mouse.move(box.x + box.width * 0.2, box.y + box.height * 0.2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.7, box.y + box.height * 0.7, { steps: 5 });
  await page.mouse.up();
  await page.waitForFunction(() => document.querySelector("#count")?.textContent === "1");
}

async function submit(page) {
  await page.click("#submit");
  await page.waitForFunction(
    () => document.querySelector("#status")?.textContent.includes("Submitted"),
    { timeout: 30000 },
  );
}

const browser = await puppeteer.connect({ browserURL: CHROME, defaultViewport: null });
try {
  const id = await extensionId(browser);
  await configure(browser, id);

  const imageId = await createImageSession();
  const imagePage = await openCapture(browser, id, imageId, "image");
  await drawRect(imagePage);
  await submit(imagePage);
  await imagePage.close();
  const image = await json(`/api/sessions/${imageId}`);
  assert.equal(image.meta.status, "submitted");
  assert.equal(image.annotations.annotations.length, 1);
  assert.equal(image.annotations.annotations[0].width, 3);

  const videoId = await createVideoSession();
  const videoPage = await openCapture(browser, id, videoId, "video");
  await videoPage.waitForSelector("#trace-panel:not([hidden])", { timeout: 10000 });
  const traceRows = await videoPage.$$eval("#trace-list .trace-row", (rows) => rows.length);
  assert.ok(traceRows >= 2, `expected trace error rows, got ${traceRows}`);
  await videoPage.select("#trace-filter", "all");
  await new Promise((resolve) => setTimeout(resolve, 150));
  assert.ok((await videoPage.$$eval("#trace-list .trace-row", (rows) => rows.length)) >= 4);
  await videoPage.evaluate(() => {
    const timeline = document.querySelector("#timeline");
    timeline.value = "0.5";
    timeline.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await drawRect(videoPage);
  await submit(videoPage);
  await videoPage.close();
  const video = await json(`/api/sessions/${videoId}`);
  assert.equal(video.meta.status, "submitted");
  assert.equal(video.meta.trace.kind, "browser-flow");
  assert.equal(video.annotations.annotations[0].at, 0.5);
  assert.ok(video.annotations.frames.length >= 1, "expected at least one annotated frame");

  console.log("extension e2e: ok");
} finally {
  await browser.disconnect();
}
