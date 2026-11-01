import { RedLineApi } from "./lib/api.js";
import { isConfigured, loadConfig } from "./lib/config.js";

const ACTIVE_KEY = "activeCapture";

async function activeTab(tabId) {
  if (tabId) return chrome.tabs.get(tabId);
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab) throw new Error("No active tab");
  return tab;
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
  if (!/^https?:/i.test(tab.url || "")) {
    throw new Error("This page cannot be captured; open an http(s) page");
  }
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

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
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
