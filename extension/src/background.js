import { RedLineApi } from "./lib/api.js";
import { isConfigured, loadConfig } from "./lib/config.js";
import { consoleText, makeTrace, redactText, redactUrl } from "./lib/trace.js";

const ACTIVE_KEY = "activeCapture";
const RECORDING_KEY = "recording";
const MAX_EVENTS = 5000;

let recording = null;
let persistTimer = null;
let stopInFlight = null;

async function activeTab(tabId) {
  if (tabId) return chrome.tabs.get(tabId);
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab) throw new Error("No active tab");
  return tab;
}

function assertRecordable(tab) {
  if (!/^https?:/i.test(tab.url || "")) {
    throw new Error("This page cannot be captured; open an http(s) page");
  }
}

async function captureAndCreate({ tabId } = {}) {
  const config = await loadConfig();
  if (!isConfigured(config)) {
    const error = new Error("Connect RedLine in the extension options first");
    error.code = "not-configured";
    throw error;
  }
  const tab = await activeTab(tabId);
  if (!tab.windowId) throw new Error("The active tab has no window");
  assertRecordable(tab);
  const imageDataUrl = await chrome.tabs.captureVisibleTab(tab.windowId, { format: "png" });
  const api = new RedLineApi(config);
  const created = await api.createImageSession({
    imageDataUrl,
    title: tab.title || "Screenshot",
    url: tab.url || "",
    project: config.project,
    branch: config.branch,
  });
  const capture = {
    sessionId: created.meta.id,
    serverUrl: config.serverUrl,
    reviewUrl: created.review_url,
    title: created.meta.title,
    createdAt: new Date().toISOString(),
  };
  await chrome.storage.session.set({ [ACTIVE_KEY]: capture });
  chrome.runtime.sendMessage({ type: "capture-created", capture }).catch(() => {});
  return { capture, meta: created.meta };
}

// ------------------------------------------------------------------ recording

async function ensureRecording() {
  if (recording) return recording;
  const stored = await chrome.storage.session.get(RECORDING_KEY);
  const saved = stored[RECORDING_KEY];
  if (!saved) return null;
  recording = { ...saved, requests: new Map() };
  return recording;
}

function persistRecordingSoon() {
  if (persistTimer) return;
  persistTimer = setTimeout(() => {
    persistTimer = null;
    if (!recording) return;
    const { tabId, startedAt, title, url, serverUrl, events, truncated } = recording;
    chrome.storage.session
      .set({
        [RECORDING_KEY]: { tabId, startedAt, title, url, serverUrl, events, truncated },
      })
      .catch(() => {});
  }, 500);
}

async function offscreen(message) {
  const response = await chrome.runtime.sendMessage({ ...message, target: "offscreen" });
  if (!response?.ok) throw new Error(response?.error || "Recorder failed");
  return response;
}

async function ensureOffscreen() {
  if (await chrome.offscreen.hasDocument?.()) return;
  try {
    await chrome.offscreen.createDocument({
      url: "src/offscreen.html",
      reasons: ["USER_MEDIA"],
      justification: "Record a browser flow so the human can annotate it in RedLine",
    });
  } catch (error) {
    if (!/single offscreen|already exists/i.test(error.message || "")) throw error;
  }
}

async function closeOffscreen() {
  try {
    await chrome.offscreen.closeDocument();
  } catch {
    // already gone
  }
}

async function attachDebugger(tabId) {
  try {
    await chrome.debugger.attach({ tabId }, "1.3");
  } catch (error) {
    if (!/already attached/i.test(error.message || "")) throw error;
  }
  await chrome.debugger.sendCommand({ tabId }, "Network.enable", {
    maxTotalBufferSize: 20 * 1024 * 1024,
    maxResourceBufferSize: 5 * 1024 * 1024,
    maxPostDataSize: 0,
  });
  await chrome.debugger.sendCommand({ tabId }, "Runtime.enable");
  await chrome.debugger.sendCommand({ tabId }, "Log.enable");
  await chrome.debugger.sendCommand({ tabId }, "Page.enable");
}

async function detachDebugger(tabId) {
  try {
    await chrome.debugger.detach({ tabId });
  } catch {
    // the tab may have closed or the user detached it
  }
}

async function injectContent(tabId) {
  try {
    await chrome.scripting.executeScript({
      target: { tabId, allFrames: true },
      files: ["src/content.js"],
    });
  } catch {
    // Restricted frames are not fatal; video/console/network still work.
  }
}

async function startRecording({ tabId } = {}) {
  if (stopInFlight) throw new Error("A recording is being saved");
  const existing = await ensureRecording();
  if (existing) throw new Error("A recording is already running");
  const config = await loadConfig();
  if (!isConfigured(config)) {
    const error = new Error("Connect RedLine in the extension options first");
    error.code = "not-configured";
    throw error;
  }
  const tab = await activeTab(tabId);
  assertRecordable(tab);
  await ensureOffscreen();
  let started;
  try {
    const streamId = await chrome.tabCapture.getMediaStreamId({ targetTabId: tab.id });
    started = await offscreen({ type: "start-recording", streamId });
    await attachDebugger(tab.id);
    await injectContent(tab.id);
  } catch (error) {
    await offscreen({ type: "discard-recording" }).catch(() => {});
    await detachDebugger(tab.id);
    await closeOffscreen();
    throw error;
  }
  recording = {
    tabId: tab.id,
    startedAt: started.startedAt,
    title: tab.title || "Browser flow",
    url: tab.url || "",
    serverUrl: config.serverUrl,
    events: [],
    truncated: false,
    requests: new Map(),
  };
  await chrome.storage.session.set({
    [RECORDING_KEY]: {
      tabId: recording.tabId,
      startedAt: recording.startedAt,
      title: recording.title,
      url: recording.url,
      serverUrl: recording.serverUrl,
      events: [],
      truncated: false,
    },
  });
  await chrome.action.setBadgeBackgroundColor({ color: "#e11d48" });
  await chrome.action.setBadgeText({ text: "REC" });
  return {
    startedAt: recording.startedAt,
    tabId: recording.tabId,
    title: recording.title,
    url: recording.url,
  };
}

async function addTraceEvent(event) {
  const current = await ensureRecording();
  if (!current) return;
  if (current.events.length >= MAX_EVENTS) {
    current.truncated = true;
    persistRecordingSoon();
    return;
  }
  const t = Math.max(0, Math.round(Date.now() - current.startedAt));
  current.events.push({ ...event, t });
  persistRecordingSoon();
}

async function addMarker({ label = "marker" } = {}) {
  const current = await ensureRecording();
  if (!current) throw new Error("No recording is running");
  await addTraceEvent({ kind: "marker", label: redactText(label, 200) });
  return { events: current.events.length };
}

async function recordingState() {
  const current = await ensureRecording();
  if (!current) return { active: false };
  return {
    active: true,
    startedAt: current.startedAt,
    tabId: current.tabId,
    title: current.title,
    url: current.url,
    events: current.events.length,
    truncated: current.truncated,
  };
}

function networkEvent(request, kind) {
  return {
    kind: "network",
    method: request.method,
    url: request.url,
    status: request.status || 0,
    duration_ms: Math.max(0, Math.round(Date.now() - request.at)),
    resource: request.resource || "",
    ...(kind === "failed" ? { failed: true, error: "request failed" } : {}),
  };
}

async function debuggerEvent(tabId, method, params) {
  const current = await ensureRecording();
  if (!current || current.tabId !== tabId) return;
  if (method === "Network.requestWillBeSent") {
    const url = redactUrl(params.request?.url || "");
    if (
      !url ||
      url.startsWith(current.serverUrl) ||
      url.startsWith("chrome-extension://") ||
      url.startsWith("data:")
    ) {
      return;
    }
    current.requests.set(params.requestId, {
      at: Date.now(),
      method: String(params.request?.method || "GET").slice(0, 16),
      url,
      resource: String(params.type || "").slice(0, 64),
      status: 0,
    });
  } else if (method === "Network.responseReceived") {
    const request = current.requests.get(params.requestId);
    if (request) request.status = Number(params.response?.status) || 0;
  } else if (method === "Network.loadingFinished") {
    const request = current.requests.get(params.requestId);
    if (!request) return;
    current.requests.delete(params.requestId);
    await addTraceEvent(networkEvent(request, "finished"));
  } else if (method === "Network.loadingFailed") {
    const request = current.requests.get(params.requestId);
    if (!request) return;
    current.requests.delete(params.requestId);
    await addTraceEvent(networkEvent(request, "failed"));
  } else if (method === "Runtime.consoleAPICalled") {
    const text = consoleText(params.args);
    if (text) {
      await addTraceEvent({
        kind: "console",
        level: String(params.type || "log").slice(0, 32),
        text,
      });
    }
  } else if (method === "Runtime.exceptionThrown") {
    const details = params.exceptionDetails || {};
    const text = redactText(
      details.exception?.description || details.text || "Uncaught exception",
    );
    await addTraceEvent({ kind: "exception", text });
  } else if (method === "Log.entryAdded") {
    const entry = params.entry || {};
    const text = redactText(`${entry.source || "browser"}: ${entry.text || ""}`);
    if (text) {
      await addTraceEvent({
        kind: "console",
        level: String(entry.level || "info").slice(0, 32),
        text,
      });
    }
  } else if (method === "Page.frameNavigated") {
    await addTraceEvent({ kind: "navigation", url: redactUrl(params.frame?.url || "") });
  } else if (method === "Page.navigatedWithinDocument") {
    await addTraceEvent({ kind: "navigation", url: redactUrl(params.url || "") });
  }
}

async function stopRecording() {
  if (stopInFlight) return stopInFlight;
  stopInFlight = (async () => {
    const current = await ensureRecording();
    if (!current) throw new Error("No recording is running");
    const config = await loadConfig();
    await detachDebugger(current.tabId);
    chrome.tabs.sendMessage(current.tabId, { type: "redline-stop" }).catch(() => {});
    const stopped = await offscreen({ type: "stop-recording" });
    const trace = makeTrace({
      startedAt: current.startedAt,
      durationMs: stopped.durationMs,
      page: { url: current.url, title: current.title },
      events: current.events,
      truncated: current.truncated,
    });
    let created;
    try {
      created = await offscreen({
        type: "upload-recording",
        config: { serverUrl: config.serverUrl, token: config.token },
        trace,
        title: current.title,
        url: current.url,
        note: "",
        project: config.project,
        branch: config.branch,
      });
    } catch (error) {
      // Keep the stopped clip in the offscreen document for a later retry.
      throw error;
    }
    const capture = {
      sessionId: created.meta.id,
      serverUrl: config.serverUrl,
      reviewUrl: created.review_url,
      title: created.meta.title,
      kind: "video",
      createdAt: new Date().toISOString(),
    };
    await chrome.storage.session.set({ [ACTIVE_KEY]: capture });
    await chrome.storage.session.remove(RECORDING_KEY);
    recording = null;
    await chrome.action.setBadgeText({ text: "" });
    await closeOffscreen();
    chrome.runtime.sendMessage({ type: "capture-created", capture }).catch(() => {});
    return { capture, meta: created.meta, trace: trace.summary || null };
  })();
  try {
    return await stopInFlight;
  } finally {
    stopInFlight = null;
  }
}

async function discardRecording() {
  const current = await ensureRecording();
  if (current) {
    await detachDebugger(current.tabId);
    chrome.tabs.sendMessage(current.tabId, { type: "redline-stop" }).catch(() => {});
  }
  await offscreen({ type: "discard-recording" }).catch(() => {});
  await closeOffscreen();
  await chrome.storage.session.remove(RECORDING_KEY);
  recording = null;
  await chrome.action.setBadgeText({ text: "" });
}

// ------------------------------------------------------------------ events

chrome.debugger.onEvent.addListener((source, method, params) => {
  if (source.tabId) debuggerEvent(source.tabId, method, params).catch(() => {});
});

chrome.debugger.onDetach.addListener((source) => {
  ensureRecording()
    .then((current) => {
      if (current && current.tabId === source.tabId && !stopInFlight) {
        current.truncated = true;
        persistRecordingSoon();
      }
    })
    .catch(() => {});
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status !== "complete") return;
  ensureRecording()
    .then((current) => {
      if (current && current.tabId === tabId) injectContent(tabId);
    })
    .catch(() => {});
});

chrome.tabs.onRemoved.addListener((tabId) => {
  ensureRecording()
    .then((current) => {
      if (current && current.tabId === tabId) {
        current.truncated = true;
        persistRecordingSoon();
      }
    })
    .catch(() => {});
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.target === "offscreen") return false;
  (async () => {
    try {
      if (message?.type === "capture-and-create") {
        sendResponse({ ok: true, ...(await captureAndCreate(message)) });
      } else if (message?.type === "get-active-capture") {
        const stored = await chrome.storage.session.get(ACTIVE_KEY);
        sendResponse({ ok: true, capture: stored[ACTIVE_KEY] || null });
      } else if (message?.type === "clear-active-capture") {
        await chrome.storage.session.remove(ACTIVE_KEY);
        sendResponse({ ok: true });
      } else if (message?.type === "start-recording") {
        sendResponse({ ok: true, recording: await startRecording(message) });
      } else if (message?.type === "stop-recording") {
        sendResponse({ ok: true, ...(await stopRecording()) });
      } else if (message?.type === "get-recording") {
        sendResponse({ ok: true, recording: await recordingState() });
      } else if (message?.type === "add-marker") {
        sendResponse({ ok: true, ...(await addMarker(message)) });
      } else if (message?.type === "discard-recording") {
        await discardRecording();
        sendResponse({ ok: true });
      } else if (message?.type === "redline-action") {
        if (message.action) {
          await addTraceEvent({
            kind: "action",
            action: redactText(message.action.action || "", 32),
            selector: redactText(message.action.selector || "", 1024),
            tag: redactText(message.action.tag || "", 32),
            role: redactText(message.action.role || "", 64),
            label: redactText(message.action.label || "", 200),
            ...(message.action.value
              ? { value: redactText(message.action.value, 200) }
              : {}),
          });
        }
        sendResponse({ ok: true });
      } else {
        sendResponse({ ok: false, error: `unknown message: ${message?.type}` });
      }
    } catch (error) {
      sendResponse({
        ok: false,
        error: error.message || String(error),
        code: error.code || "failed",
        status: error.status,
      });
    }
  })();
  return true;
});

chrome.runtime.onInstalled.addListener(() => {
  try {
    chrome.sidePanel
      ?.setPanelBehavior?.({ openPanelOnActionClick: false })
      ?.catch?.(() => {});
  } catch {
    // Older Chromium builds do not expose sidePanel; the popup falls back to a review tab.
  }
});
