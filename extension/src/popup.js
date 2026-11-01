import { isConfigured, loadConfig } from "./lib/config.js";

const $ = (id) => document.getElementById(id);
const statusEl = $("status");
const captureButton = $("capture");
const recordButton = $("record");
const stopButton = $("stop");
const markButton = $("mark");
const timerEl = $("timer");

let configured = false;
let recording = null;
let tick = null;
let activeTabId = null;

function setStatus(text, kind = "") {
  statusEl.textContent = text;
  statusEl.className = `status ${kind}`.trim();
}

function elapsedText(startedAt) {
  const seconds = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
  return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
}

function render() {
  const active = Boolean(recording?.active);
  recordButton.hidden = active;
  stopButton.hidden = !active;
  markButton.hidden = !active;
  recordButton.disabled = !configured;
  captureButton.disabled = !configured || active;
  if (active) {
    timerEl.textContent = elapsedText(recording.startedAt);
    setStatus("Recording this tab. Stop & save when the flow is done.");
  } else {
    timerEl.textContent = "";
    if (configured) setStatus("");
  }
}

function startTick() {
  clearInterval(tick);
  tick = setInterval(() => {
    if (!recording?.active) return;
    timerEl.textContent = elapsedText(recording.startedAt);
  }, 1000);
}

async function refreshRecording() {
  const response = await chrome.runtime.sendMessage({ type: "get-recording" });
  recording = response?.recording || null;
  render();
  if (recording?.active) startTick();
  else clearInterval(tick);
}

async function init() {
  const config = await loadConfig();
  configured = isConfigured(config);
  $("not-configured").hidden = configured;
  $("connection").textContent = configured
    ? `Configured · ${new URL(config.serverUrl).host}`
    : "Not connected";
  $("dashboard").href = config.serverUrl || "#";
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (tab) {
    activeTabId = tab.id;
    $("page-title").textContent = tab.title || "Untitled page";
    $("page-url").textContent = tab.url || "";
  }
  await refreshRecording();
}

async function openOptions() {
  await chrome.runtime.openOptionsPage();
  window.close();
}

async function capture() {
  if (recording?.active) return;
  captureButton.disabled = true;
  setStatus("Capturing…");
  if (!activeTabId) {
    setStatus("No active tab", "err");
    captureButton.disabled = false;
    return;
  }

  const pending = chrome.runtime.sendMessage({
    type: "capture-and-create",
    tabId: activeTabId,
  });
  let panelOpened = false;
  try {
    await chrome.sidePanel.open({ tabId: activeTabId });
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

async function startRecording() {
  if (!activeTabId) return;
  recordButton.disabled = true;
  setStatus("Starting recorder…");
  try {
    const response = await chrome.runtime.sendMessage({
      type: "start-recording",
      tabId: activeTabId,
    });
    if (!response?.ok) throw new Error(response?.error || "Recording failed");
    recording = { active: true, ...response.recording };
    render();
    startTick();
  } catch (error) {
    setStatus(error.message || String(error), "err");
    recordButton.disabled = false;
  }
}

async function stopRecording() {
  stopButton.disabled = true;
  markButton.disabled = true;
  setStatus("Stopping and uploading flow…");
  try {
    const response = await chrome.runtime.sendMessage({ type: "stop-recording" });
    if (!response?.ok) throw new Error(response?.error || "Stop failed");
    recording = null;
    render();
    const capture = response.capture;
    if (capture?.reviewUrl) {
      await chrome.tabs.create({ url: capture.reviewUrl });
    }
    window.close();
  } catch (error) {
    setStatus(error.message || String(error), "err");
    stopButton.disabled = false;
    markButton.disabled = false;
  }
}

async function mark() {
  markButton.disabled = true;
  try {
    const response = await chrome.runtime.sendMessage({
      type: "add-marker",
      label: "manual marker",
    });
    if (!response?.ok) throw new Error(response?.error || "Marker failed");
    setStatus(`Marker added (${response.events} events so far).`);
  } catch (error) {
    setStatus(error.message || String(error), "err");
  } finally {
    markButton.disabled = false;
  }
}

$("options").addEventListener("click", openOptions);
$("configure").addEventListener("click", openOptions);
captureButton.addEventListener("click", capture);
recordButton.addEventListener("click", startRecording);
stopButton.addEventListener("click", stopRecording);
markButton.addEventListener("click", mark);
window.addEventListener("unload", () => clearInterval(tick));

init().catch((error) => setStatus(error.message || String(error), "err"));
