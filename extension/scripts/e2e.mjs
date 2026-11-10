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

async function jsonWith(token, pathname, init = {}) {
  const response = await fetch(`${BASE}${pathname}`, {
    ...init,
    headers: {
      Authorization: `Bearer ${token}`,
      ...(init.headers || {}),
    },
  });
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
        else if (message.result?.exceptionDetails) {
          const details = message.result.exceptionDetails;
          reject(new Error(details.exception?.description || details.text));
        } else resolve(message.result);
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
  // Some headless builds start with no normal browser window, so `chrome.tabs.create` answers
  // "No current window"; creating the window with the URL is the same destination.
  await evaluateInWorker(
    worker,
    `(async () => {
      const target = chrome.runtime.getURL(${JSON.stringify(url)});
      try {
        await chrome.tabs.create({ url: target });
      } catch (error) {
        if (!String(error).includes("No current window")) throw error;
        await chrome.windows.create({ url: target });
      }
    })()`,
  );
  const target = await browser.waitForTarget(
    (candidate) => candidate.type() === "page" && candidate.url().endsWith(url),
    { timeout: 10000 },
  );
  const page = await target.page();
  if (!page) throw new Error(`cannot attach to extension page ${pathname}`);
  return page;
}

async function configure(browser, id, token = TOKEN) {
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
  }, { base: BASE, token });
  await page.click("#save");
  await page.waitForFunction(
    () => document.querySelector("#status")?.textContent.includes("Saved"),
    { timeout: 10000 },
  );
  await page.close();
}

async function createImageSession(token = TOKEN, { title = "Extension image e2e", project } = {}) {
  const image = fs.readFileSync(path.join(repo, "desktop/app/icons/128x128.png")).toString("base64");
  const created = await jsonWith(token, "/api/sessions", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      title,
      url: "https://app.test/checkout",
      image_base64: `data:image/png;base64,${image}`,
      ...(project ? { project } : {}),
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

const TESTER_USERNAME = "e2e-tester";
const TESTER_PASSWORD = "correct horse battery staple";

/** The browser e2e normally creates this user; keep the extension e2e runnable on its own. */
async function ensureTester() {
  const users = await json("/api/users");
  let user = users.users.find((entry) => entry.username === TESTER_USERNAME);
  if (!user) {
    const created = await json("/api/users", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        username: TESTER_USERNAME,
        display_name: "E2E Tester",
        password: TESTER_PASSWORD,
      }),
    });
    user = created.user;
  }
  await json("/api/projects", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name: "e2e-repo" }),
  });
  const members = await json("/api/projects/e2e-repo/members");
  const next = members.members
    .filter((entry) => entry.user_id !== user.id)
    .map((entry) => ({ user_id: entry.user_id, role: entry.role }));
  next.push({ user_id: user.id, role: "tester" });
  await json("/api/projects/e2e-repo/members", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ members: next }),
  });
  return user;
}

/** A PAT belongs to the tester, not the legacy server token: the side panel must see `tester`. */
async function createTesterToken() {
  const user = await ensureTester();
  const login = await fetch(`${BASE}/api/auth/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username: TESTER_USERNAME, password: TESTER_PASSWORD }),
  });
  if (!login.ok) {
    throw new Error(`tester login: ${login.status} ${await login.text()}`);
  }
  const cookie = login.headers
    .getSetCookie()
    .map((value) => value.split(";")[0])
    .join("; ");
  const csrf = /(?:^|; )redline_csrf=([^;]+)/.exec(cookie)?.[1];
  if (!csrf) throw new Error("tester login did not set the CSRF cookie");
  const response = await fetch(`${BASE}/api/me/tokens`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-CSRF-Token": csrf,
      Cookie: cookie,
    },
    body: JSON.stringify({ name: "Extension tester e2e", projects: ["e2e-repo"] }),
  });
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(`tester PAT: ${response.status} ${JSON.stringify(data)}`);
  }
  return { user, token: data.token };
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
    tabId = null,
  },
) {
  const page = await openExtensionPage(browser, "src/sidepanel.html");
  await page.waitForFunction(() => document.querySelector("#workspace"));
  await page.evaluate(
    async ({ id, kind, title, url, tabId, imageBase64, videoBase64, posterBase64, trace, width, height, durationMs }) => {
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
        activeCapture: { id, sessionId: id, tabId, mode: "local", kind, title, url },
      });
    },
    { id, kind, title, url, tabId, imageBase64, videoBase64, posterBase64, trace, width, height, durationMs },
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

async function clickPanel(page, selector) {
  await page.evaluate((value) => {
    const element = document.querySelector(value);
    if (!element) throw new Error(`missing ${value}`);
    element.click();
  }, selector);
}

async function createTargetPage(browser) {
  const marker = `e2e-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  const page = await browser.newPage();
  await page.goto(`${BASE}/health?e2e=${marker}`, { waitUntil: "domcontentloaded" });
  const worker = await serviceWorker(browser);
  const tabs = await evaluateInWorker(worker, "chrome.tabs.query({})");
  const tab = tabs.result.value.find((entry) => entry.url.includes(marker));
  if (!tab) throw new Error("target tab not found");
  return { page, tabId: tab.id, marker };
}

async function waitOverlayFrame(page) {
  await page.bringToFront();
  for (let attempt = 0; attempt < 60; attempt += 1) {
    const frame = page.frames().find((candidate) => candidate.url().includes("/src/overlay.html"));
    if (frame) {
      const box = await frame
        .$eval("#canvas", (canvas) => {
          const rect = canvas.getBoundingClientRect();
          return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
        })
        .catch(() => null);
      if (box && box.width > 0 && box.height > 0) return { frame, box };
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("overlay canvas did not lay out");
}

async function openOverlayCapture(browser, sessionId, tabId, title = "Extension image e2e") {
  const capture = {
    sessionId,
    tabId,
    serverUrl: BASE,
    reviewUrl: `${BASE}/s/${sessionId}`,
    title,
    kind: "image",
    createdAt: new Date().toISOString(),
  };
  const page = await openExtensionPage(browser, "src/sidepanel.html");
  await page.waitForFunction(() => document.querySelector("#workspace"));
  await page.evaluate(async ({ value }) => {
    await chrome.storage.session.set({ activeCapture: value });
  }, { value: capture });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => !document.querySelector("#workspace")?.hidden, { timeout: 15000 });
  return page;
}

async function drawRectInFrame(page, frame) {
  await page.bringToFront();
  const box = await frame.$eval("#canvas", (canvas) => {
    const rect = canvas.getBoundingClientRect();
    return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
  });
  await page.mouse.move(box.x + box.width * 0.2, box.y + box.height * 0.2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.7, box.y + box.height * 0.7, { steps: 5 });
  await page.mouse.up();
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
  // A previous local e2e run can leave an active capture in this profile; start the control-deck
  // checks from the empty state.
  await evaluateInWorker(await serviceWorker(browser), 'chrome.storage.session.remove("activeCapture")');

  // The side panel owns the connection form; the gear must not open a separate options tab.
  const configuredPanel = await openExtensionPage(browser, "src/sidepanel.html");
  await configuredPanel.waitForFunction(() => !document.querySelector("#empty")?.hidden);
  await clickPanel(configuredPanel, "#settings");
  await configuredPanel.waitForFunction(
    () => !document.querySelector("#settings-panel")?.hidden,
    { timeout: 10000 },
  );
  const settings = await configuredPanel.evaluate(() => ({
    server: document.querySelector("#settings-server").value,
    token: document.querySelector("#settings-token").value,
  }));
  assert.equal(settings.server, BASE);
  assert.equal(settings.token, TOKEN);
  await clickPanel(configuredPanel, "#settings-save");
  await configuredPanel.waitForFunction(
    () => document.querySelector("#settings-status")?.textContent.includes("Saved"),
    { timeout: 10000 },
  );
  await configuredPanel.waitForFunction(
    () => document.querySelector("#settings-panel")?.hidden
      && !document.querySelector("#empty")?.hidden,
    { timeout: 10000 },
  );
  // An extension/options tab is not capturable: say which tab is visible and disable the button
  // instead of looking like a dead action (2026-11-07).
  await configuredPanel.waitForFunction(
    () => {
      const context = document.querySelector("#capture-context")?.textContent || "";
      return context.includes("Switch to the web page") &&
        Boolean(document.querySelector("#capture")?.disabled);
    },
    { timeout: 10000 },
  );
  const captureContext = await configuredPanel.$eval("#capture-context", (element) => element.textContent.trim());
  assert.match(captureContext, /Switch to the web page/);
  await configuredPanel.close();

  const target = await createTargetPage(browser);
  const imageId = await createImageSession();
  const imagePanel = await openOverlayCapture(browser, imageId, target.tabId);
  assert.equal(
    await imagePanel.$eval("#video-bar", (element) => getComputedStyle(element).display),
    "none",
    "an image review must not show the video timeline",
  );
  const overlay = await waitOverlayFrame(target.page);
  assert.equal(await overlay.frame.$("#close"), null, "the overlay must not have its own close button");
  await clickPanel(imagePanel, '[data-tool="arrow"]');
  await imagePanel.waitForFunction(
    () => document.querySelector('[data-tool="arrow"]')?.classList.contains("active"),
    { timeout: 5000 },
  );
  await overlay.frame.waitForFunction(
    () => document.body.dataset.tool === "arrow",
    { timeout: 5000 },
  );
  await drawRectInFrame(target.page, overlay.frame);
  await imagePanel.waitForFunction(
    () => document.querySelector("#count")?.textContent === "1",
    { timeout: 10000 },
  );
  await imagePanel.waitForFunction(
    () => document.querySelector("#list .ann-row")?.textContent.includes("Arrow"),
    { timeout: 5000 },
  );
  // Closing the panel must not lose the round: pause, resume from the draft, then submit.
  await clickPanel(imagePanel, "#pause-overlay");
  await target.page.waitForFunction(
    () => !document.querySelector("#redline-overlay-host"),
    { timeout: 10000 },
  );
  await imagePanel.waitForFunction(
    () => !document.querySelector("#resume-overlay")?.hidden,
    { timeout: 10000 },
  );
  await clickPanel(imagePanel, "#resume-overlay");
  const resumed = await waitOverlayFrame(target.page);
  await imagePanel.waitForFunction(
    () => document.querySelector("#count")?.textContent === "1",
    { timeout: 10000 },
  );
  await resumed.frame.waitForFunction(
    () => document.body.dataset.tool === "arrow",
    { timeout: 5000 },
  );
  await imagePanel.type("#note", "overlay note");
  await clickPanel(imagePanel, "#submit");
  await target.page.waitForFunction(
    () => !document.querySelector("#redline-overlay-host"),
    { timeout: 20000 },
  );
  const image = await json(`/api/sessions/${imageId}`);
  assert.equal(image.meta.status, "submitted");
  assert.equal(image.annotations.annotations.length, 1);
  assert.equal(image.annotations.annotations[0].type, "arrow");
  assert.equal(image.annotations.annotations[0].width, 3);
  assert.equal(image.meta.prompt, "overlay note");
  await imagePanel.close();
  await target.page.close();

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
  const localTarget = await createTargetPage(browser);
  const localImageId = `local-e2e-image-${Date.now()}`;
  const localPanel = await openLocalCapture(browser, {
    id: localImageId,
    kind: "image",
    title: "Local image e2e",
    url: "https://app.test/local",
    imageBase64: iconBase64,
    tabId: localTarget.tabId,
  });
  assert.equal(
    (await localPanel.$eval("#submit", (element) => element.textContent)).trim(),
    "Export review",
  );
  assert.equal(await localPanel.$eval("#open-redline", (element) => element.hidden), true);
  assert.equal(
    await localPanel.$eval("#local-warning", (element) => element.hidden),
    false,
    "a local capture must show the offline warning",
  );
  const localOverlay = await waitOverlayFrame(localTarget.page);
  await drawRectInFrame(localTarget.page, localOverlay.frame);
  try {
    await localPanel.waitForFunction(
      () => document.querySelector("#count")?.textContent === "1",
      { timeout: 10000, polling: 100 },
    );
  } catch (error) {
    console.log("local count panel", await localPanel.evaluate(() => ({
      status: document.querySelector("#status")?.textContent,
      emptyStatus: document.querySelector("#empty-status")?.textContent,
      count: document.querySelector("#count")?.textContent,
      subtitle: document.querySelector("#subtitle")?.textContent,
      title: document.querySelector("#title")?.textContent,
    })));
    console.log("local count overlay", await localOverlay.frame
      .$eval("#status", (element) => element.textContent)
      .catch(() => "no overlay status"));
    throw error;
  }
  await installPickerMock(localPanel);
  await clickPanel(localPanel, "#submit");
  try {
    await localPanel.waitForFunction(
      () => document.querySelector("#status")?.textContent.includes("Exported"),
      { timeout: 30000, polling: 100 },
    );
  } catch (error) {
    console.log("local panel state", await localPanel.evaluate(() => ({
      status: document.querySelector("#status")?.textContent,
      emptyStatus: document.querySelector("#empty-status")?.textContent,
      submit: document.querySelector("#submit")?.textContent,
      count: document.querySelector("#count")?.textContent,
      workspace: document.querySelector("#workspace")?.hidden,
      exported: globalThis.__exported?.length || 0,
    })));
    console.log("local overlay state", await localOverlay.frame
      .$eval("#status", (element) => element.textContent)
      .catch(() => "no overlay status"));
    throw error;
  }
  const exported = await localPanel.evaluate(() => globalThis.__exported);
  const names = exported.map((file) => file.name);
  assert.ok(names.includes("review.md"), `missing review.md: ${names}`);
  assert.ok(names.includes("annotations.json"), `missing annotations.json: ${names}`);
  assert.ok(names.includes("annotated.png"), `missing annotated.png: ${names}`);
  assert.ok(names.includes("AI-PROMPT.md"), `missing AI-PROMPT.md: ${names}`);
  const review = exported.find((file) => file.name === "review.md").text;
  assert.match(review, /Local image e2e/);
  assert.match(review, /Rectangle/);
  const payload = JSON.parse(exported.find((file) => file.name === "annotations.json").text);
  assert.equal(payload.session, localImageId);
  assert.equal(payload.annotations.length, 1);
  assert.equal(payload.image.file, "annotated.png");
  assert.ok(exported.find((file) => file.name === "annotated.png").bytes > 0);
  await localPanel.close();
  await localTarget.page.close();

  const videoBase64 = fs
    .readFileSync(path.join(repo, "extension/tests/fixtures/flow.webm"))
    .toString("base64");
  const trace = JSON.parse(
    fs.readFileSync(path.join(repo, "extension/tests/fixtures/trace.json"), "utf8"),
  );
  const localVideoId = `local-e2e-video-${Date.now()}`;
  const localVideo = await openLocalCapture(browser, {
    id: localVideoId,
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

  // A tester's capture must not wake the agent: it is saved as a request for a developer.
  const tester = await createTesterToken();
  await configure(browser, id, tester.token);
  const testerTarget = await createTargetPage(browser);
  const testerTask = await createImageSession(tester.token, {
    title: "Extension tester e2e",
    project: "e2e-repo",
  });
  const testerPanel = await openOverlayCapture(
    browser,
    testerTask,
    testerTarget.tabId,
    "Extension tester e2e",
  );
  const testerLabel = await testerPanel.$eval("#submit", (element) => ({
    text: element.textContent.trim(),
    disabled: element.disabled,
  }));
  assert.equal(testerLabel.text, "Send to developer");
  assert.equal(testerLabel.disabled, false);
  const testerOverlay = await waitOverlayFrame(testerTarget.page);
  await drawRectInFrame(testerTarget.page, testerOverlay.frame);
  await testerPanel.waitForFunction(
    () => document.querySelector("#count")?.textContent === "1",
    { timeout: 10000 },
  );
  await clickPanel(testerPanel, "#submit");
  await testerTarget.page.waitForFunction(
    () => !document.querySelector("#redline-overlay-host"),
    { timeout: 20000 },
  );
  await testerPanel.waitForFunction(
    () => !document.querySelector("#empty")?.hidden
      && document.querySelector("#empty-status")?.textContent.includes("Sent to a developer"),
    { timeout: 30000 },
  );
  assert.equal(
    await testerPanel.$eval("#open-capture", (element) => element.hidden),
    false,
    "the tester can open the requested task in RedLine",
  );
  const requested = await json(`/api/sessions/${testerTask}`);
  assert.equal(requested.meta.status, "open");
  assert.ok(requested.meta.agent_requested_at, "the tester request must be stamped");
  assert.equal(requested.meta.agent_requested_by, tester.user.id);
  assert.equal(requested.annotations.annotations.length, 1);
  // A developer sends the saved round to the agent; the request stamp clears.
  const sent = await json(`/api/sessions/${testerTask}/annotations`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      annotations: requested.annotations.annotations,
      width: requested.annotations.width,
      height: requested.annotations.height,
      note: "developer approved",
    }),
  });
  assert.equal(sent.meta.status, "submitted");
  assert.ok(!sent.meta.agent_requested_at, "the developer submit clears the request");
  await testerPanel.close();
  await testerTarget.page.close();

  // One capture button: without a server it captures locally and shows the orange warning.
  await evaluateInWorker(await serviceWorker(browser), 'chrome.storage.local.remove("redlineConfig")');
  await evaluateInWorker(await serviceWorker(browser), 'chrome.storage.session.remove("activeCapture")');
  const localLiveTarget = await createTargetPage(browser);
  const localLivePanel = await openExtensionPage(browser, "src/sidepanel.html");
  await localLivePanel.waitForFunction(() => !document.querySelector("#empty")?.hidden);
  await localLiveTarget.page.bringToFront();
  await localLivePanel.waitForFunction(
    () => !document.querySelector("#capture")?.disabled
      && (document.querySelector("#capture-context")?.textContent || "").includes("Current tab"),
    { timeout: 10000, polling: 100 },
  );
  await clickPanel(localLivePanel, "#capture");
  await localLiveTarget.page.waitForSelector("#redline-overlay-host", { timeout: 15000 });
  await localLivePanel.waitForFunction(
    () => !document.querySelector("#workspace")?.hidden
      && !document.querySelector("#local-warning")?.hidden,
    { timeout: 15000, polling: 100 },
  );
  const localWarning = await localLivePanel.$eval("#local-warning", (element) => ({
    text: element.textContent,
    hidden: element.hidden,
  }));
  assert.equal(localWarning.hidden, false);
  assert.match(localWarning.text, /saved locally/i);
  const localLiveOverlay = await waitOverlayFrame(localLiveTarget.page);
  await drawRectInFrame(localLiveTarget.page, localLiveOverlay.frame);
  await localLivePanel.waitForFunction(
    () => document.querySelector("#count")?.textContent === "1",
    { timeout: 10000, polling: 100 },
  );
  assert.equal(
    (await localLivePanel.$eval("#submit", (element) => element.textContent)).trim(),
    "Export review",
  );
  await installPickerMock(localLivePanel);
  await clickPanel(localLivePanel, "#submit");
  await localLivePanel.waitForFunction(
    () => document.querySelector("#status")?.textContent.includes("Exported"),
    { timeout: 30000, polling: 100 },
  );
  const liveExport = await localLivePanel.evaluate(() => globalThis.__exported);
  assert.ok(
    liveExport.length >= 4,
    `local export files: ${liveExport.map((file) => file.name)}`,
  );
  await localLivePanel.close();
  await localLiveTarget.page.close();

  console.log("extension e2e: ok");
} finally {
  await browser.disconnect();
}
