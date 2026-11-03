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

const EXTENSION_NAME = "RedLine Capture";
// Chrome ships built-in background service workers (both named background.js). Matching the exact
// path from this manifest is the difference between testing the extension and testing Chrome.
const WORKER_PATH = "/src/background.js";

async function evaluateInWorker(worker, expression) {
  const ws = new WebSocket(worker.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", reject, { once: true });
  });
  try {
    return await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("service worker evaluate timed out")), 5000);
      ws.addEventListener("message", (event) => {
        const message = JSON.parse(event.data);
        if (message.id !== 1) return;
        clearTimeout(timer);
        if (message.error) reject(new Error(JSON.stringify(message.error)));
        else resolve(message.result);
      });
      ws.send(
        JSON.stringify({
          id: 1,
          method: "Runtime.evaluate",
          params: { expression, awaitPromise: true, returnByValue: true },
        }),
      );
    });
  } finally {
    ws.close();
  }
}

function isExtensionWorker(target) {
  if (target.type !== "service_worker") return false;
  try {
    return new URL(target.url).pathname === WORKER_PATH;
  } catch {
    return false;
  }
}

async function serviceWorker(browser) {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    const targets = await (await fetch(`${CHROME}/json/list`)).json();
    const worker = targets.find(isExtensionWorker);
    if (worker) {
      // The worker URL identifies the right file; the manifest name proves the right extension.
      const name = await evaluateInWorker(worker, "chrome.runtime.getManifest().name");
      if (name?.result?.value !== EXTENSION_NAME) {
        throw new Error(
          `${WORKER_PATH} belongs to "${name?.result?.value}", not ${EXTENSION_NAME}`,
        );
      }
      return worker;
    }
    if (attempt === 4) {
      const page = await browser.newPage();
      await page.goto("chrome://extensions/");
      await new Promise((resolve) => setTimeout(resolve, 500));
      await page.close();
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("RedLine Capture service worker not found");
}

async function extensionId(browser) {
  return new URL((await serviceWorker(browser)).url).host;
}

// Chrome 159 headless (and some headful builds) refuses a page.goto() from a normal tab into a
// chrome-extension:// URL. Let the extension's own service worker create the tab instead, then
// attach to the page target.
async function openExtensionPage(browser, pathname) {
  const worker = await serviceWorker(browser);
  const url = `${pathname}?e2e=${Date.now()}-${Math.random().toString(16).slice(2)}`;
  await evaluateInWorker(
    worker,
    `chrome.tabs.create({url: chrome.runtime.getURL(${JSON.stringify(url)})})`,
  );
  const target = await browser.waitForTarget(
    (candidate) => candidate.type() === "page" && candidate.url().endsWith(url),
    { timeout: 10000 },
  );
  const page = await target.page();
  if (!page) throw new Error(`cannot attach to extension page ${pathname}`);
  return page;
}

async function configure(browser, id) {
  const page = await openExtensionPage(browser, "src/options.html");
  await page.waitForFunction(() => document.body.dataset.ready === "1");
  // The Project select and Branch input share one row. "(optional)" is a hint on the Branch line,
  // not its own row: as a bare flex item it pushed the Branch input a full line down (2026-11-03).
  const layout = await page.evaluate(() => {
    const project = document.querySelector("#project");
    const branch = document.querySelector("#branch");
    const label = branch.closest("label");
    const hint = label.querySelector(".muted");
    return {
      projectTop: project.getBoundingClientRect().top,
      branchTop: branch.getBoundingClientRect().top,
      labelTop: label.getBoundingClientRect().top,
      hintTop: hint.getBoundingClientRect().top,
      hintText: hint.textContent,
    };
  });
  assert.equal(layout.hintText, "(optional)");
  assert.ok(
    Math.abs(layout.projectTop - layout.branchTop) <= 1,
    `Project select and Branch input must align (${layout.projectTop} vs ${layout.branchTop})`,
  );
  assert.ok(
    layout.hintTop - layout.labelTop <= 6,
    `the (optional) hint must stay on the Branch line (${layout.hintTop} vs ${layout.labelTop})`,
  );
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
  const page = await openExtensionPage(browser, "src/sidepanel.html");
  await page.waitForFunction(() => document.querySelector("#workspace"));
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

async function openLocalCapture(
  browser,
  {
    id,
    kind,
    title,
    url,
    imageBase64 = "",
    videoBase64 = "",
    posterBase64 = "",
    trace = null,
    width = 0,
    height = 0,
    durationMs = 0,
  },
) {
  const page = await openExtensionPage(browser, "src/sidepanel.html");
  await page.waitForFunction(() => document.querySelector("#workspace"));
  await page.evaluate(
    async ({ id, kind, title, url, imageBase64, videoBase64, posterBase64, trace, width, height, durationMs }) => {
      const bytes = (base64) => {
        const binary = atob(base64);
        const out = new Uint8Array(binary.length);
        for (let index = 0; index < binary.length; index += 1) out[index] = binary.charCodeAt(index);
        return out;
      };
      const db = await new Promise((resolve, reject) => {
        const request = indexedDB.open("redline-local", 1);
        request.onupgradeneeded = () => {
          if (!request.result.objectStoreNames.contains("captures")) {
            request.result.createObjectStore("captures", { keyPath: "id" });
          }
        };
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      const capture = {
        id,
        sessionId: id,
        mode: "local",
        kind,
        title,
        url,
        note: "offline export",
        createdAt: new Date().toISOString(),
        annotations: [],
        width,
        height,
        durationMs,
      };
      if (kind === "video") {
        capture.videoBlob = new Blob([bytes(videoBase64)], { type: "video/webm" });
        capture.posterBlob = new Blob([bytes(posterBase64)], { type: "image/png" });
        capture.trace = trace;
      } else {
        capture.imageBlob = new Blob([bytes(imageBase64)], { type: "image/png" });
      }
      await new Promise((resolve, reject) => {
        const tx = db.transaction("captures", "readwrite");
        tx.objectStore("captures").put(capture);
        tx.oncomplete = resolve;
        tx.onerror = () => reject(tx.error);
      });
      await chrome.storage.session.set({
        activeCapture: { id, sessionId: id, mode: "local", kind, title, url },
      });
    },
    { id, kind, title, url, imageBase64, videoBase64, posterBase64, trace, width, height, durationMs },
  );
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#workspace:not([hidden])", { timeout: 15000 });
  return page;
}

async function installPickerMock(page) {
  await page.evaluate(() => {
    const collected = [];
    globalThis.__exported = collected;
    const makeDir = (prefix = "") => ({
      async getDirectoryHandle(name) {
        return makeDir(`${prefix}${name}/`);
      },
      async getFileHandle(name) {
        const full = `${prefix}${name}`;
        return {
          async createWritable() {
            return {
              async write(data) {
                if (data instanceof Blob) {
                  collected.push({
                    name: full,
                    type: data.type,
                    bytes: data.size,
                    text:
                      data.type.startsWith("text/") ||
                      full.endsWith(".md") ||
                      full.endsWith(".json")
                        ? await data.text()
                        : null,
                  });
                } else {
                  collected.push({ name: full, text: String(data) });
                }
              },
              async close() {},
            };
          },
        };
      },
    });
    globalThis.__redlineTestPicker = async () => makeDir();
  });
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
  assert.equal(
    await imagePage.$eval("#video-bar", (element) => getComputedStyle(element).display),
    "none",
    "an image review must not show the video timeline",
  );
  await drawRect(imagePage);
  await submit(imagePage);
  await imagePage.close();
  const image = await json(`/api/sessions/${imageId}`);
  assert.equal(image.meta.status, "submitted");
  assert.equal(image.annotations.annotations.length, 1);
  assert.equal(image.annotations.annotations[0].width, 3);

  const videoId = await createVideoSession();
  const videoPage = await openCapture(browser, id, videoId, "video");
  assert.notEqual(
    await videoPage.$eval("#video-bar", (element) => getComputedStyle(element).display),
    "none",
    "a video review must show the video timeline",
  );
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

  const iconBase64 = fs
    .readFileSync(path.join(repo, "desktop/app/icons/128x128.png"))
    .toString("base64");
  const localPage = await openLocalCapture(browser, {
    id: "local-e2e-image",
    kind: "image",
    title: "Local image e2e",
    url: "https://app.test/local",
    imageBase64: iconBase64,
  });
  assert.equal(
    (await localPage.$eval("#submit", (element) => element.textContent)).trim(),
    "Export review",
  );
  assert.equal(await localPage.$eval("#open-redline", (element) => element.hidden), true);
  await drawRect(localPage);
  await installPickerMock(localPage);
  await localPage.click("#submit");
  await localPage.waitForFunction(
    () => document.querySelector("#status")?.textContent.includes("Exported"),
    { timeout: 15000 },
  );
  const exported = await localPage.evaluate(() => globalThis.__exported);
  const names = exported.map((file) => file.name);
  assert.ok(names.includes("review.md"), `missing review.md: ${names}`);
  assert.ok(names.includes("annotations.json"), `missing annotations.json: ${names}`);
  assert.ok(names.includes("annotated.png"), `missing annotated.png: ${names}`);
  assert.ok(names.includes("AI-PROMPT.md"), `missing AI-PROMPT.md: ${names}`);
  const review = exported.find((file) => file.name === "review.md").text;
  assert.match(review, /Local image e2e/);
  assert.match(review, /Rectangle/);
  const payload = JSON.parse(exported.find((file) => file.name === "annotations.json").text);
  assert.equal(payload.session, "local-e2e-image");
  assert.equal(payload.annotations.length, 1);
  assert.equal(payload.image.file, "annotated.png");
  assert.ok(exported.find((file) => file.name === "annotated.png").bytes > 0);
  await localPage.close();

  const videoBase64 = fs
    .readFileSync(path.join(repo, "extension/tests/fixtures/flow.webm"))
    .toString("base64");
  const trace = JSON.parse(
    fs.readFileSync(path.join(repo, "extension/tests/fixtures/trace.json"), "utf8"),
  );
  const localVideo = await openLocalCapture(browser, {
    id: "local-e2e-video",
    kind: "video",
    title: "Local video e2e",
    url: "https://app.test/flow",
    videoBase64,
    posterBase64: iconBase64,
    trace,
    width: 320,
    height: 180,
    durationMs: 1500,
  });
  await localVideo.waitForSelector("#video-bar:not([hidden])", { timeout: 10000 });
  await localVideo.waitForSelector("#trace-panel:not([hidden])", { timeout: 10000 });
  await localVideo.evaluate(() => {
    const timeline = document.querySelector("#timeline");
    timeline.value = "0.5";
    timeline.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await new Promise((resolve) => setTimeout(resolve, 200));
  await drawRect(localVideo);
  await installPickerMock(localVideo);
  await localVideo.click("#submit");
  await localVideo.waitForFunction(
    () => document.querySelector("#status")?.textContent.includes("Exported"),
    { timeout: 15000 },
  );
  const videoExport = await localVideo.evaluate(() => globalThis.__exported);
  const videoNames = videoExport.map((file) => file.name);
  for (const name of [
    "review.md",
    "annotations.json",
    "AI-PROMPT.md",
    "video.webm",
    "poster.png",
    "trace.json",
    "frames/0.500.png",
    "contact-sheet.png",
  ]) {
    assert.ok(videoNames.includes(name), `missing ${name}: ${videoNames}`);
  }
  const videoReview = videoExport.find((file) => file.name === "review.md").text;
  assert.match(videoReview, /Local video e2e/);
  assert.match(videoReview, /Browser trace/);
  const videoPayload = JSON.parse(
    videoExport.find((file) => file.name === "annotations.json").text,
  );
  assert.equal(videoPayload.media.kind, "video");
  assert.equal(videoPayload.image.file, "poster.png");
  assert.equal(videoPayload.frames[0].file, "frames/0.500.png");
  assert.equal(videoPayload.trace.kind, "browser-flow");
  assert.ok(videoExport.find((file) => file.name === "video.webm").bytes > 0);
  await localVideo.close();

  console.log("extension e2e: ok");
} finally {
  await browser.disconnect();
}
