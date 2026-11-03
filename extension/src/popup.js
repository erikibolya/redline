import { RedLineApi } from "./lib/api.js";
import { isConfigured, loadConfig, saveConfig } from "./lib/config.js";
import { fillProjectSelect } from "./lib/projects.js";
import { applyTheme, preferredTheme, toggleTheme } from "./lib/theme.js";

const $ = (id) => document.getElementById(id);
const statusEl = $("status");
const captureButton = $("capture");
const captureOfflineButton = $("capture-offline");
const recordButton = $("record");
const recordOfflineButton = $("record-offline");
const stopButton = $("stop");
const markButton = $("mark");
const timerEl = $("timer");
const projectRow = $("project-row");
const projectSelect = $("project");

let configured = false;
let recording = null;
let tick = null;
let activeTabId = null;
let projectSave = Promise.resolve();

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
  const local = Boolean(recording?.local);
  recordButton.hidden = active;
  recordOfflineButton.hidden = active;
  stopButton.hidden = !active;
  markButton.hidden = !active;
  recordButton.disabled = !configured || active;
  captureButton.disabled = !configured || active;
  captureOfflineButton.disabled = active;
  recordOfflineButton.disabled = active;
  if (active) {
    timerEl.textContent = elapsedText(recording.startedAt);
    setStatus(
      local
        ? "Recording this tab offline. Stop & save when the flow is done."
        : "Recording this tab. Stop & save when the flow is done.",
    );
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
  applyTheme(preferredTheme());
  const config = await loadConfig();
  configured = isConfigured(config);
  $("not-configured").hidden = configured;
  projectRow.hidden = !configured;
  projectSelect.disabled = !configured;
  $("connection").textContent = configured
    ? `Configured · ${new URL(config.serverUrl).host}`
    : "Offline available";
  $("dashboard").href = config.serverUrl || "#";
  $("dashboard").hidden = !configured;
  if (configured) {
    fillProjectSelect(projectSelect, [], config.project || "");
    await loadProjects(config).catch(() => {});
    projectSelect.value = config.project || "";
  }
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

/// The popup is a quick capture surface: the project chosen here is saved into the same config the
/// background worker reads when it creates the session, so the options page and the popup cannot
/// disagree about where the review goes.
async function loadProjects(config) {
  const api = new RedLineApi(config);
  const data = await api.projects();
  const current = projectSelect.value || config.project || "";
  fillProjectSelect(projectSelect, data.projects || [], current);
}

async function rememberProject() {
  const project = projectSelect.value;
  try {
    const config = await loadConfig();
    if (config.project === project) return;
    await saveConfig({ ...config, project });
  } catch (error) {
    setStatus(error.message || String(error), "err");
  }
}

function queueProjectSave() {
  projectSave = rememberProject();
  return projectSave;
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
  await projectSave;

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

async function captureOffline() {
  if (recording?.active) return;
  captureOfflineButton.disabled = true;
  setStatus("Capturing offline…");
  if (!activeTabId) {
    setStatus("No active tab", "err");
    captureOfflineButton.disabled = false;
    return;
  }

  const pending = chrome.runtime.sendMessage({ type: "capture-local", tabId: activeTabId });
  let panelOpened = false;
  try {
    await chrome.sidePanel.open({ tabId: activeTabId });
    panelOpened = true;
  } catch {
    // Fall back to the side panel page in a normal tab.
  }

  try {
    const response = await pending;
    if (!response?.ok) throw new Error(response?.error || "Capture failed");
    if (!panelOpened) {
      await chrome.tabs.create({ url: chrome.runtime.getURL("src/sidepanel.html") });
    }
    window.close();
  } catch (error) {
    setStatus(error.message || String(error), "err");
    captureOfflineButton.disabled = false;
  }
}

async function startRecording(local = false) {
  if (!activeTabId) return;
  await projectSave;
  const button = local ? recordOfflineButton : recordButton;
  button.disabled = true;
  setStatus(local ? "Starting offline recorder…" : "Starting recorder…");
  try {
    const response = await chrome.runtime.sendMessage({
      type: "start-recording",
      tabId: activeTabId,
      local,
    });
    if (!response?.ok) throw new Error(response?.error || "Recording failed");
    recording = { active: true, ...response.recording };
    render();
    startTick();
  } catch (error) {
    setStatus(error.message || String(error), "err");
    button.disabled = false;
  }
}

async function openLocalPanel() {
  if (!activeTabId) return false;
  try {
    await chrome.sidePanel.open({ tabId: activeTabId });
    return true;
  } catch {
    return false;
  }
}

async function stopRecording() {
  const wasLocal = Boolean(recording?.local);
  stopButton.disabled = true;
  markButton.disabled = true;
  setStatus(wasLocal ? "Stopping and saving offline…" : "Stopping and uploading flow…");
  let panelOpened = false;
  if (wasLocal) panelOpened = await openLocalPanel();
  try {
    const response = await chrome.runtime.sendMessage({ type: "stop-recording" });
    if (!response?.ok) throw new Error(response?.error || "Stop failed");
    recording = null;
    render();
    const capture = response.capture;
    if (wasLocal) {
      if (!panelOpened) {
        await chrome.tabs.create({ url: chrome.runtime.getURL("src/sidepanel.html") });
      }
    } else if (capture?.reviewUrl) {
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
$("theme").addEventListener("click", () => toggleTheme());
$("close").addEventListener("click", () => window.close());
projectSelect.addEventListener("change", queueProjectSave);
captureButton.addEventListener("click", capture);
captureOfflineButton.addEventListener("click", captureOffline);
recordButton.addEventListener("click", () => startRecording(false));
recordOfflineButton.addEventListener("click", () => startRecording(true));
stopButton.addEventListener("click", stopRecording);
markButton.addEventListener("click", mark);
window.addEventListener("unload", () => clearInterval(tick));

init()
  .catch((error) => setStatus(error.message || String(error), "err"))
  .finally(() => {
    // The extension e2e (and a slow popup) can wait for the stored config and project list to land
    // instead of racing the async init with a click.
    document.body.dataset.ready = "1";
  });
