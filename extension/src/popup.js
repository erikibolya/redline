import { isConfigured, loadConfig } from "./lib/config.js";

const $ = (id) => document.getElementById(id);
const statusEl = $("status");
const captureButton = $("capture");

function setStatus(text, kind = "") {
  statusEl.textContent = text;
  statusEl.className = `status ${kind}`.trim();
}

async function init() {
  const config = await loadConfig();
  const configured = isConfigured(config);
  $("not-configured").hidden = configured;
  captureButton.disabled = !configured;
  $("connection").textContent = configured
    ? `Configured · ${new URL(config.serverUrl).host}`
    : "Not connected";
  $("dashboard").href = config.serverUrl || "#";
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (tab) {
    $("page-title").textContent = tab.title || "Untitled page";
    $("page-url").textContent = tab.url || "";
  }
}

async function openOptions() {
  await chrome.runtime.openOptionsPage();
  window.close();
}

async function capture() {
  captureButton.disabled = true;
  setStatus("Capturing…");
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab) {
    setStatus("No active tab", "err");
    captureButton.disabled = false;
    return;
  }

  // Start the capture first (the service worker stores it even if the popup closes), then open
  // the side panel while the click still has user activation. The panel listens for the
  // capture-created message and also re-reads storage on load.
  const pending = chrome.runtime.sendMessage({ type: "capture-and-create", tabId: tab.id });
  let panelOpened = false;
  try {
    await chrome.sidePanel.open({ tabId: tab.id });
    panelOpened = true;
  } catch {
    // Fall back to opening the review in a tab after the capture completes.
  }

  try {
    const response = await pending;
    if (!response?.ok) throw new Error(response?.error || "Capture failed");
    if (!panelOpened) {
      await chrome.tabs.create({ url: response.capture.reviewUrl });
    }
    window.close();
  } catch (error) {
    setStatus(error.message || String(error), "err");
    captureButton.disabled = false;
  }
}

$("options").addEventListener("click", openOptions);
$("configure").addEventListener("click", openOptions);
captureButton.addEventListener("click", capture);
init().catch((error) => setStatus(error.message || String(error), "err"));
