import { RedLineApi } from "./lib/api.js";
import {
  ensurePermission,
  isConfigured,
  loadConfig,
  normalizeServerUrl,
  saveConfig,
} from "./lib/config.js";
import { deleteCapture, getCapture, putCapture } from "./lib/local-store.js";
import {
  AI_PROMPT,
  buildAnnotationsJson,
  buildReviewMarkdown,
  buildZip,
  frameFileName,
} from "./lib/package.js";
import {
  annotationVisibleAt,
  composePng,
  createAnnotation,
  drawAnnotation,
  drawAnnotations,
  drawScene,
  finishAnnotation,
  hitTest,
  moveAnnotation,
  TOOLS,
} from "./lib/annotations.js";

const $ = (id) => document.getElementById(id);
const canvas = $("canvas");
const ctx = canvas.getContext("2d");
const textEditor = $("text-editor");
const noteInput = $("note");
const statusEl = $("status");
const emptyStatusEl = $("empty-status");
const captureContextEl = $("capture-context");
const settingsStatusEl = $("settings-status");
const video = $("video");
let settingsReturn = null;

const state = {
  config: null,
  capture: null,
  api: null,
  meta: null,
  bitmap: null,
  mediaKind: "image",
  width: 0,
  height: 0,
  video: null,
  videoUrl: null,
  trace: null,
  traceEvents: [],
  traceFilter: "errors",
  ghosts: false,
  frameLoop: null,
  annotations: [],
  selectedId: null,
  tool: "select",
  color: "#e11d48",
  strokeWidth: 3,
  undo: [],
  redo: [],
  drawing: null,
  preview: null,
  drag: null,
  textPoint: null,
  readOnly: false,
  submitted: false,
  role: "owner",
  requested: false,
  local: false,
  localData: null,
  saveTimer: null,
  overlayMode: false,
  overlayActive: false,
  overlayTabId: null,
  undoCount: 0,
  redoCount: 0,
  submitting: false,
  captureAllowed: false,
};

function setStatus(text, kind = "") {
  statusEl.textContent = text;
  statusEl.className = `status ${kind}`.trim();
  // The empty view has its own status line; without this a failed capture looked like a dead
  // button (2026-11-07).
  if (!$("empty").hidden) {
    emptyStatusEl.textContent = text;
    emptyStatusEl.className = `status ${kind}`.trim();
  }
}

function setSettingsStatus(text, kind = "") {
  settingsStatusEl.textContent = text;
  settingsStatusEl.className = `status ${kind}`.trim();
}

async function loadSettingsProjects(config) {
  const api = new RedLineApi(config);
  const data = await api.projects();
  const current = settingsProjectValue();
  const select = $("settings-project");
  select.innerHTML = '<option value="">Inbox</option>';
  for (const project of data.projects || []) {
    const option = document.createElement("option");
    option.value = project.slug;
    option.textContent = project.name || project.slug;
    select.append(option);
  }
  if (current && [...select.options].some((option) => option.value === current)) {
    select.value = current;
  }
}

function settingsProjectValue() {
  return $("settings-project").value || "";
}

async function openSettings() {
  if (!$("settings-panel").hidden) return;
  settingsReturn = {
    empty: !$("empty").hidden,
    workspace: !$("workspace").hidden,
  };
  $("empty").hidden = true;
  $("workspace").hidden = true;
  $("settings-panel").hidden = false;
  const config = await loadConfig();
  $("settings-server").value = config.serverUrl || "";
  $("settings-token").value = config.token || "";
  $("settings-branch").value = config.branch || "";
  const select = $("settings-project");
  select.innerHTML = '<option value="">Inbox</option>';
  if (config.project) {
    const option = document.createElement("option");
    option.value = config.project;
    option.textContent = config.project;
    select.append(option);
    select.value = config.project;
  }
  setSettingsStatus("");
  if (isConfigured(config)) {
    try {
      await loadSettingsProjects(config);
      select.value = config.project || "";
    } catch {
      // An invalid token should not block editing the form; Save/Test reports the error.
    }
  }
  $("settings-server").focus();
}

function closeSettings() {
  if ($("settings-panel").hidden) return;
  $("settings-panel").hidden = true;
  const back = settingsReturn || { empty: true, workspace: false };
  settingsReturn = null;
  $("empty").hidden = !back.empty;
  $("workspace").hidden = !back.workspace;
  if (back.workspace) {
    render();
    return;
  }
  loadConfig()
    .then((config) => {
      if (!isConfigured(config) && $("empty").hidden === false) {
        setStatus("Connect RedLine for server mode, or save a local capture.", "");
      }
    })
    .catch(() => {});
}

async function saveSettings() {
  const config = {
    serverUrl: normalizeServerUrl($("settings-server").value),
    token: $("settings-token").value.trim(),
    project: settingsProjectValue(),
    branch: $("settings-branch").value.trim(),
  };
  if (!config.token) throw new Error("Token is required");
  setSettingsStatus("Saving…");
  if (!(await ensurePermission(config.serverUrl))) {
    throw new Error("Access to the RedLine server was not granted");
  }
  const saved = await saveConfig(config);
  let projectsOk = true;
  await loadSettingsProjects(saved).catch(() => {
    projectsOk = false;
  });
  setSettingsStatus(
    projectsOk
      ? "Saved. You can capture a page now."
      : "Saved, but the server rejected the token or URL. Check them and press Test.",
    projectsOk ? "ok" : "err",
  );
  if (!state.capture && projectsOk) {
    settingsReturn = { empty: true, workspace: false };
    closeSettings();
    setStatus("Connected. Click Screenshot current tab.", "ok");
  }
}

async function testSettings() {
  const config = {
    serverUrl: normalizeServerUrl($("settings-server").value),
    token: $("settings-token").value.trim(),
    project: settingsProjectValue(),
    branch: $("settings-branch").value.trim(),
  };
  if (!config.token) throw new Error("Token is required");
  setSettingsStatus("Testing…");
  if (!(await ensurePermission(config.serverUrl))) {
    throw new Error("Access to the RedLine server was not granted");
  }
  const api = new RedLineApi(config);
  const health = await api.health();
  await loadSettingsProjects(config);
  setSettingsStatus(`Connected to RedLine ${health.version || ""}`.trim(), "ok");
}

function setBusy(busy) {
  for (const element of document.querySelectorAll("button, input, select, textarea")) {
    if (element.id === "settings") continue;
    if (element.id === "capture") {
      element.disabled = busy || !state.captureAllowed;
      continue;
    }
    element.disabled = busy || (state.readOnly && !["open-redline", "settings"].includes(element.id));
  }
  if (!busy && state.readOnly) {
    $("open-redline").disabled = false;
    $("settings").disabled = false;
  }
}

async function refreshCaptureContext() {
  try {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    if (!tab) {
      state.captureAllowed = false;
      captureContextEl.textContent = "No active tab.";
      captureContextEl.className = "capture-context err";
      $("capture").disabled = true;
      return;
    }
    const url = tab.url || tab.pendingUrl || "";
    const capturable = /^https?:/i.test(url);
    state.captureAllowed = capturable;
    captureContextEl.textContent = capturable
      ? `Current tab: ${tab.title || url}`
      : `This is ${tab.title || url}. Switch to the web page tab you want to capture.`;
    captureContextEl.className = `capture-context ${capturable ? "" : "err"}`.trim();
    $("capture").disabled = !capturable;
    $("capture").title = capturable ? "" : "Switch to an http(s) page tab first.";
  } catch (error) {
    state.captureAllowed = false;
    captureContextEl.textContent = error.message || String(error);
    captureContextEl.className = "capture-context err";
    $("capture").disabled = true;
  }
}

function showEmpty(message = "", keepCapture = false) {
  $("workspace").hidden = true;
  $("empty").hidden = false;
  $("open-capture").hidden = true;
  $("video-bar").hidden = true;
  $("trace-panel").hidden = true;
  $("overlay-banner").hidden = true;
  $("pause-overlay").hidden = true;
  $("capture-new").hidden = true;
  $("resume-overlay").hidden = true;
  $("stage").hidden = false;
  $("title").textContent = "RedLine Capture";
  $("subtitle").textContent = "No capture yet";
  $("empty-status").textContent = message || "";
  $("empty-status").className = `status ${message ? "err" : ""}`.trim();
  state.overlayMode = false;
  state.overlayActive = false;
  state.submitting = false;
  state.undoCount = 0;
  state.redoCount = 0;
  if (!keepCapture) {
    state.capture = null;
    state.meta = null;
    state.annotations = [];
    state.selectedId = null;
    state.local = false;
    state.localData = null;
  }
  if (state.video) state.video.pause();
  if (message) setStatus(message, "err");
  refreshCaptureContext().catch(() => {});
}

function sendOverlayCommand(command) {
  if (!state.capture?.sessionId) return;
  chrome.runtime
    .sendMessage({
      type: "redline-overlay-command",
      sessionId: state.capture.sessionId,
      command,
    })
    .catch(() => {});
}

async function injectOverlay(capture) {
  const tabId = capture?.tabId || state.overlayTabId;
  if (!tabId) throw new Error("This capture has no tab. Screenshot the tab again.");
  await chrome.scripting.executeScript({
    target: { tabId },
    files: ["src/overlay-host.js"],
  });
  const response = await chrome.tabs.sendMessage(tabId, {
    type: "redline-overlay-open",
    sessionId: capture.sessionId,
  });
  if (!response?.ok) throw new Error("Could not open the annotation overlay.");
  state.overlayTabId = tabId;
  state.overlayActive = true;
  chrome.runtime
    .sendMessage({
      type: "redline-overlay-command",
      sessionId: capture.sessionId,
      command: { type: "request-state" },
    })
    .catch(() => {});
}

async function closeOverlay(capture = state.capture) {
  if (!capture?.tabId) return;
  await chrome.tabs
    .sendMessage(capture.tabId, {
      type: "redline-overlay-close",
      sessionId: capture.sessionId,
    })
    .catch(() => {});
  state.overlayActive = false;
}

function resumeOverlay() {
  if (!state.capture || state.submitted) return;
  injectOverlay(state.capture)
    .then(() => setStatus("Annotating on the page. Controls are here."))
    .catch((error) => setStatus(error.message || String(error), "err"));
}

function overlayCommand(command) {
  if (!state.capture || !state.overlayActive) return;
  sendOverlayCommand(command);
}

function applyOverlayState(message) {
  const next = message.state || {};
  if (Array.isArray(next.annotations)) state.annotations = next.annotations;
  state.selectedId = next.selectedId || null;
  if (next.tool) state.tool = next.tool;
  if (next.color) state.color = next.color;
  if (next.width) state.strokeWidth = Number(next.width) || state.strokeWidth;
  if (next.widthPx) state.width = Number(next.widthPx) || state.width;
  if (next.heightPx) state.height = Number(next.heightPx) || state.height;
  state.undoCount = Number(next.undo) || 0;
  state.redoCount = Number(next.redo) || 0;
  state.readOnly = Boolean(next.readOnly);
  if (next.role) state.role = next.role;
  state.requested = Boolean(next.requested);
  state.submitting = Boolean(next.submitting);
  if (next.status && state.meta) state.meta.status = next.status;
  if (typeof next.note === "string" && document.activeElement !== noteInput) {
    noteInput.value = next.note;
  }
  if (next.title) $("title").textContent = next.title;
  if (state.width && state.height) {
    $("subtitle").textContent = `${state.width}×${state.height}${
      state.readOnly ? ` · ${state.meta?.status || "read-only"}` : ""
    }`;
  }

  if (message.event === "submitted") {
    state.meta = message.meta || state.meta;
    state.overlayActive = false;
    state.submitting = false;
    state.requested = Boolean(state.meta?.agent_requested_at);
    state.submitted =
      state.meta?.status === "submitted" || state.meta?.status === "resolved";
    state.readOnly = state.submitted || state.role === "viewer";
    void closeOverlay();
    chrome.runtime.sendMessage({ type: "clear-active-capture" }).catch(() => {});
    if (state.meta?.status === "open" && state.role === "tester") {
      showSent("Sent to a developer.", true);
      return;
    }
    setStatus("Submitted to RedLine. The agent queue has it.", "ok");
  } else if (message.event === "error") {
    state.submitting = false;
    state.overlayActive = true;
    setStatus(message.error || "Something went wrong.", "err");
  } else if (message.event === "discarded") {
    state.submitting = false;
    void closeOverlay();
    chrome.runtime.sendMessage({ type: "clear-active-capture" }).catch(() => {});
    showEmpty("Task discarded.");
    return;
  } else if (message.event === "paused") {
    state.overlayActive = false;
    state.submitting = false;
    setStatus("Capture paused. Resume annotating to continue.");
  } else if (message.event === "ready" || message.event === "sync") {
    state.overlayActive = true;
    state.submitting = false;
    if (!state.readOnly) setStatus("Annotate on the page. Controls are here.");
  }

  $("overlay-banner").hidden = !state.overlayMode || state.submitted;
  $("overlay-banner-text").textContent = state.readOnly
    ? "Read-only review — open it in RedLine to edit."
    : state.overlayActive
      ? "Annotating on the page — draw on the frozen screenshot."
      : "Capture paused — the annotations are saved.";
  $("resume-overlay").hidden = !state.overlayMode || state.overlayActive || state.submitted;
  $("pause-overlay").hidden = !(state.overlayActive && !state.submitted);
  $("capture-new").hidden = !state.submitted;
  render();
}

function showWorkspace() {
  $("empty").hidden = true;
  $("workspace").hidden = false;
  $("empty-status").textContent = "";
  $("stage").hidden = state.overlayMode;
  updateModeChrome();
}

function mediaWidth() {
  return state.mediaKind === "video" ? state.width : state.bitmap?.width || 0;
}

function mediaHeight() {
  return state.mediaKind === "video" ? state.height : state.bitmap?.height || 0;
}

function formatTime(seconds) {
  const value = Math.max(0, Number(seconds) || 0);
  const minutes = Math.floor(value / 60);
  const rest = value - minutes * 60;
  return `${String(minutes).padStart(2, "0")}:${rest.toFixed(3).padStart(6, "0")}`;
}

function updateModeChrome() {
  const videoMode = state.mediaKind === "video";
  $("video-bar").hidden = !videoMode;
  $("trace-panel").hidden = !(videoMode && state.traceEvents.length);
}

function draftKey() {
  return `draft:${state.capture.sessionId}`;
}

async function saveDraft() {
  if (!state.capture || state.readOnly) return;
  clearTimeout(state.saveTimer);
  state.saveTimer = setTimeout(() => {
    chrome.storage.session
      .set({
        [draftKey()]: {
          annotations: state.annotations,
          note: noteInput.value,
          tool: state.tool,
          color: state.color,
          width: state.strokeWidth,
          updatedAt: Date.now(),
        },
      })
      .catch(() => {});
  }, 250);
}

function render() {
  if (state.overlayMode) {
    renderOverlayControls();
    return;
  }
  if (state.mediaKind === "video") {
    renderVideo();
  } else if (state.bitmap) {
    drawScene(canvas, state.bitmap, state.annotations, state.selectedId, state.preview);
  }
  $("count").textContent = String(state.annotations.length);
  renderList();
  $("undo").disabled = state.readOnly || state.undo.length === 0;
  $("redo").disabled = state.readOnly || state.redo.length === 0;
  $("delete").disabled = state.readOnly || !state.selectedId;
  $("clear").disabled = state.readOnly || state.annotations.length === 0;
  $("submit").disabled = state.readOnly;
  const status = state.meta?.status;
  $("submit").textContent = state.local
    ? "Export review"
    : status === "resolved"
      ? "Resolved"
      : status === "submitted"
        ? "Waiting for agent"
        : state.role === "viewer"
          ? "Read-only"
          : state.role === "tester"
            ? (state.requested ? "Update request" : "Send to developer")
            : "Submit to RedLine";
  $("open-redline").hidden = state.local;
  $("discard").disabled = state.submitted;
  noteInput.disabled = state.readOnly;
  for (const button of document.querySelectorAll(".tool")) {
    button.classList.toggle("active", button.dataset.tool === state.tool);
    button.disabled = state.readOnly;
  }
  $("color").disabled = state.readOnly;
  $("width").disabled = state.readOnly;
  $("ghosts").disabled = false;
  $("timeline").disabled = false;
}

function renderOverlayControls() {
  $("stage").hidden = true;
  $("video-bar").hidden = true;
  $("trace-panel").hidden = true;
  $("count").textContent = String(state.annotations.length);
  renderList();
  $("undo").disabled = state.readOnly || state.undoCount === 0;
  $("redo").disabled = state.readOnly || state.redoCount === 0;
  $("delete").disabled = state.readOnly || !state.selectedId;
  $("clear").disabled = state.readOnly || state.annotations.length === 0;
  const paused = !state.overlayActive && !state.submitted;
  $("submit").disabled = state.readOnly || state.submitting || paused;
  const status = state.meta?.status;
  $("submit").textContent = state.submitting
    ? "Submitting…"
    : paused
      ? "Resume to submit"
      : status === "resolved"
        ? "Resolved"
        : status === "submitted"
          ? "Waiting for agent"
          : state.role === "viewer"
            ? "Read-only"
            : state.role === "tester"
              ? (state.requested ? "Update request" : "Send to developer")
              : "Submit to RedLine";
  $("open-redline").hidden = false;
  $("discard").disabled = state.submitted || state.submitting || state.role === "viewer";
  $("pause-overlay").hidden = !state.overlayActive || state.submitted;
  $("capture-new").hidden = !state.submitted;
  $("resume-overlay").hidden = state.overlayActive || state.submitted;
  $("overlay-banner").hidden = state.submitted;
  noteInput.disabled = state.readOnly || state.submitting;
  for (const button of document.querySelectorAll(".tool")) {
    button.classList.toggle("active", button.dataset.tool === state.tool);
    button.disabled = state.readOnly || state.submitting;
  }
  $("color").disabled = state.readOnly || state.submitting;
  $("width").disabled = state.readOnly || state.submitting;
  if (/^#[0-9a-f]{6}$/i.test(state.color)) $("color").value = state.color;
  if ([...$("width").options].some((option) => Number(option.value) === state.strokeWidth)) {
    $("width").value = String(state.strokeWidth);
  }
}

function renderMarkers() {
  const ann = $("ann-markers");
  const trace = $("trace-markers");
  ann.innerHTML = "";
  trace.innerHTML = "";
  if (state.mediaKind !== "video" || !state.duration) return;
  for (const annotation of state.annotations) {
    const at = Number(annotation.at) || 0;
    const marker = document.createElement("span");
    marker.style.left = `${Math.max(0, Math.min(100, (at / state.duration) * 100))}%`;
    marker.style.background = annotation.color || "#e11d48";
    ann.append(marker);
  }
  for (const event of state.traceEvents) {
    const seconds = Math.max(0, (Number(event.t) || 0) / 1000);
    const marker = document.createElement("span");
    marker.style.left = `${Math.max(0, Math.min(100, (seconds / state.duration) * 100))}%`;
    const kind = String(event.kind || "");
    if (kind === "console" && event.level === "error") marker.className = "console-error";
    else if (kind === "exception") marker.className = "console-error";
    else if (kind === "network" && (event.failed || Number(event.status) >= 400)) {
      marker.className = "network-error";
    }
    trace.append(marker);
  }
}

function renderVideo() {
  if (!state.video) return;
  const width = state.video.videoWidth || state.width;
  const height = state.video.videoHeight || state.height;
  if (width && height && (canvas.width !== width || canvas.height !== height)) {
    canvas.width = width;
    canvas.height = height;
  }
  state.width = width;
  state.height = height;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  if (state.video.readyState >= 2) ctx.drawImage(state.video, 0, 0, canvas.width, canvas.height);
  const at = state.video.currentTime;
  state.annotations.forEach((annotation, index) => {
    const visible = annotationVisibleAt(annotation, at);
    if (!visible && !state.ghosts) return;
    ctx.save();
    if (!visible) ctx.globalAlpha = 0.15;
    drawAnnotation(ctx, annotation, index + 1, annotation.id === state.selectedId && visible);
    ctx.restore();
  });
  if (state.preview) drawAnnotation(ctx, state.preview, state.annotations.length + 1, false);
  $("video-time").textContent = `${formatTime(at)} / ${formatTime(state.duration)}`;
  $("timeline").max = String(Math.max(0.001, state.duration));
  $("timeline").value = String(at);
  $("play").textContent = state.video.paused ? "▶" : "❚❚";
  renderMarkers();
}

function renderTrace() {
  const list = $("trace-list");
  list.innerHTML = "";
  if (!state.traceEvents.length) return;
  const filter = state.traceFilter;
  const events = state.traceEvents.filter((event) => {
    const kind = String(event.kind || "");
    if (filter === "all") return true;
    if (filter === "errors") {
      return (
        kind === "exception" ||
        (kind === "console" && event.level === "error") ||
        (kind === "network" && (event.failed || Number(event.status) >= 400))
      );
    }
    if (filter === "console") return kind === "console" || kind === "exception";
    if (filter === "network") return kind === "network";
    if (filter === "actions") return kind === "action";
    if (filter === "markers") return kind === "marker";
    return true;
  });
  for (const event of events.slice(0, 200)) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "trace-row";
    const kind = String(event.kind || "");
    if (kind === "exception" || (kind === "console" && event.level === "error")) {
      row.classList.add("error");
    }
    if (kind === "network" && (event.failed || Number(event.status) >= 400)) {
      row.classList.add("error");
    }
    const t = document.createElement("span");
    t.className = "t";
    t.textContent = formatTime((Number(event.t) || 0) / 1000);
    const k = document.createElement("span");
    k.className = "k";
    k.textContent = kind;
    const message = document.createElement("span");
    message.className = "m";
    if (kind === "network") {
      message.textContent = `${event.method || "GET"} ${event.url || ""}${
        event.failed ? " (failed)" : event.status ? ` → ${event.status}` : ""
      }`;
    } else if (kind === "action") {
      message.textContent = `${event.action || "action"} ${event.label || event.selector || ""}`;
    } else {
      message.textContent = String(event.text || event.label || event.url || "");
    }
    row.append(t, k, message);
    row.addEventListener("click", () => {
      seekVideo((Number(event.t) || 0) / 1000);
    });
    list.append(row);
  }
  if (events.length > 200) {
    const more = document.createElement("div");
    more.className = "trace-row";
    more.textContent = `… ${events.length - 200} more events in trace.json`;
    list.append(more);
  }
}

function typeLabel(annotation) {
  if (annotation.type === "rect") return "Rectangle";
  if (annotation.type === "arrow") return "Arrow";
  if (annotation.type === "pen") return "Pen";
  if (annotation.type === "text") return annotation.text || "Note";
  return annotation.type;
}

function renderList() {
  const list = $("list");
  list.innerHTML = "";
  state.annotations.forEach((annotation, index) => {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "ann-row";
    if (annotation.id === state.selectedId) row.classList.add("selected");
    const number = document.createElement("span");
    number.className = "ann-num";
    number.textContent = String(index + 1);
    const text = document.createElement("span");
    text.className = "ann-text";
    text.textContent =
      state.mediaKind === "video" && annotation.at != null
        ? `${formatTime(annotation.at)} · ${typeLabel(annotation)}`
        : typeLabel(annotation);
    row.append(number, text);
    row.addEventListener("click", () => {
      if (state.overlayMode) {
        overlayCommand({ type: "select", id: annotation.id });
        return;
      }
      state.selectedId = annotation.id;
      if (state.mediaKind === "video" && annotation.at != null) seekVideo(annotation.at);
      else render();
    });
    list.append(row);
  });
}

function snapshot() {
  state.undo.push(JSON.stringify(state.annotations));
  if (state.undo.length > 50) state.undo.shift();
  state.redo = [];
}

function undo() {
  if (state.overlayMode) {
    overlayCommand({ type: "undo" });
    return;
  }
  if (state.readOnly || !state.undo.length) return;
  state.redo.push(JSON.stringify(state.annotations));
  state.annotations = JSON.parse(state.undo.pop());
  state.selectedId = null;
  render();
  saveDraft();
}

function redo() {
  if (state.overlayMode) {
    overlayCommand({ type: "redo" });
    return;
  }
  if (state.readOnly || !state.redo.length) return;
  state.undo.push(JSON.stringify(state.annotations));
  state.annotations = JSON.parse(state.redo.pop());
  state.selectedId = null;
  render();
  saveDraft();
}

function removeSelected() {
  if (state.overlayMode) {
    overlayCommand({ type: "delete" });
    return;
  }
  if (state.readOnly || !state.selectedId) return;
  snapshot();
  state.annotations = state.annotations.filter((annotation) => annotation.id !== state.selectedId);
  state.selectedId = null;
  render();
  saveDraft();
}

function clearAll() {
  if (state.overlayMode) {
    if (state.readOnly || !state.annotations.length) return;
    if (!confirm("Remove all annotations?")) return;
    overlayCommand({ type: "clear" });
    return;
  }
  if (state.readOnly || !state.annotations.length) return;
  if (!confirm("Remove all annotations?")) return;
  snapshot();
  state.annotations = [];
  state.selectedId = null;
  render();
  saveDraft();
}

function setTool(tool) {
  if (!TOOLS.includes(tool) || state.readOnly) return;
  if (state.overlayMode) {
    state.tool = tool;
    overlayCommand({ type: "set-tool", tool });
    renderOverlayControls();
    return;
  }
  state.tool = tool;
  canvas.style.cursor = tool === "select" ? "default" : "crosshair";
  render();
}

function pointFromEvent(event) {
  const rect = canvas.getBoundingClientRect();
  return {
    x: ((event.clientX - rect.left) * canvas.width) / rect.width,
    y: ((event.clientY - rect.top) * canvas.height) / rect.height,
  };
}

function seekVideo(seconds) {
  if (!state.video) return;
  state.video.pause();
  const duration = state.duration || state.video.duration || 0;
  state.video.currentTime = Math.max(0, Math.min(duration || seconds, seconds));
  render();
}

function togglePlay() {
  if (!state.video) return;
  if (state.video.paused) state.video.play().catch(() => {});
  else state.video.pause();
}

function frameStep(delta) {
  if (!state.video) return;
  seekVideo(state.video.currentTime + delta);
}

let frameLoop = null;
function startFrameLoop() {
  if (frameLoop) cancelAnimationFrame(frameLoop);
  const tick = () => {
    if (state.mediaKind !== "video" || state.video?.paused) {
      frameLoop = null;
      return;
    }
    renderVideo();
    frameLoop = requestAnimationFrame(tick);
  };
  frameLoop = requestAnimationFrame(tick);
}

video.addEventListener("loadedmetadata", () => {
  const metadataDuration = Number(state.meta?.media?.duration_ms) / 1000;
  state.duration =
    Number.isFinite(video.duration) && video.duration > 0
      ? video.duration
      : Number.isFinite(metadataDuration) && metadataDuration > 0
        ? metadataDuration
        : 0;
  state.width = video.videoWidth || state.width;
  state.height = video.videoHeight || state.height;
  $("timeline").max = String(Math.max(0.001, state.duration));
  render();
});
video.addEventListener("timeupdate", () => {
  if (state.mediaKind === "video") renderVideo();
});
video.addEventListener("play", () => {
  $("play").textContent = "❚❚";
  startFrameLoop();
});
video.addEventListener("pause", () => {
  $("play").textContent = "▶";
  if (frameLoop) cancelAnimationFrame(frameLoop);
  frameLoop = null;
  renderVideo();
});
video.addEventListener("ended", () => {
  $("play").textContent = "▶";
  renderVideo();
});

$("play").addEventListener("click", togglePlay);
$("prev-frame").addEventListener("click", () => frameStep(-1 / 30));
$("next-frame").addEventListener("click", () => frameStep(1 / 30));
$("timeline").addEventListener("input", (event) => {
  if (!state.video) return;
  state.video.pause();
  const value = Number(event.target.value);
  state.video.currentTime = value;
  renderVideo();
});
$("ghosts").addEventListener("change", (event) => {
  state.ghosts = event.target.checked;
  render();
});
$("trace-filter").addEventListener("change", (event) => {
  state.traceFilter = event.target.value;
  renderTrace();
});

function openTextEditor(point) {
  state.textPoint = point;
  const canvasRect = canvas.getBoundingClientRect();
  const stageRect = $("stage").getBoundingClientRect();
  const left = canvasRect.left - stageRect.left + (point.x * canvasRect.width) / canvas.width;
  const top = canvasRect.top - stageRect.top + (point.y * canvasRect.height) / canvas.height;
  textEditor.style.left = `${Math.max(4, left)}px`;
  textEditor.style.top = `${Math.max(4, top - 12)}px`;
  textEditor.value = "";
  textEditor.hidden = false;
  textEditor.focus();
}

let textCommitInFlight = false;
function commitTextEditor() {
  if (textEditor.hidden || textCommitInFlight) return;
  textCommitInFlight = true;
  const text = textEditor.value.trim();
  if (text && state.textPoint) {
    snapshot();
    const annotation = createAnnotation("text", state.textPoint, {
      color: state.color,
      width: state.strokeWidth,
    });
    annotation.text = text;
    if (state.mediaKind === "video" && state.video) annotation.at = state.video.currentTime;
    state.annotations.push(annotation);
    state.selectedId = annotation.id;
  }
  textEditor.hidden = true;
  state.textPoint = null;
  render();
  saveDraft();
  textCommitInFlight = false;
}

function cancelTextEditor() {
  textEditor.hidden = true;
  state.textPoint = null;
}

canvas.addEventListener("pointerdown", (event) => {
  if (state.readOnly || !mediaWidth()) return;
  event.preventDefault();
  const point = pointFromEvent(event);
  if (state.mediaKind === "video" && state.video) state.video.pause();
  if (state.tool === "text") {
    openTextEditor(point);
    return;
  }
  canvas.setPointerCapture(event.pointerId);
  if (state.tool === "select") {
    const hit = hitTest(state.annotations, point);
    state.selectedId = hit?.id || null;
    if (hit) {
      snapshot();
      state.drag = { id: hit.id, last: point };
    }
    render();
    return;
  }
  snapshot();
  state.drawing = createAnnotation(state.tool, point, {
    color: state.color,
    width: state.strokeWidth,
  });
  if (state.mediaKind === "video" && state.video) {
    state.drawing.at = state.video.currentTime;
  }
  state.preview = state.drawing;
  render();
});

canvas.addEventListener("pointermove", (event) => {
  if (!state.drawing && !state.drag) return;
  const point = pointFromEvent(event);
  if (state.drag) {
    const annotation = state.annotations.find((item) => item.id === state.drag.id);
    if (annotation) {
      moveAnnotation(annotation, point.x - state.drag.last.x, point.y - state.drag.last.y);
      state.drag.last = point;
      render();
    }
  } else if (state.drawing) {
    if (state.drawing.type === "pen") {
      state.drawing.points.push(point);
    } else {
      state.drawing.end = point;
    }
    state.preview = state.drawing;
    render();
  }
});

function finishDrawing() {
  if (state.drag) {
    state.drag = null;
    saveDraft();
    return;
  }
  if (!state.drawing) return;
  const annotation = finishAnnotation(state.drawing, state.drawing.end, state.drawing.points);
  const bounds = annotation.start
    ? Math.abs(annotation.end.x - annotation.start.x) + Math.abs(annotation.end.y - annotation.start.y)
    : 0;
  const valid =
    annotation.type === "pen"
      ? (annotation.points || []).length > 1
      : annotation.type === "text" || bounds > 4;
  if (valid) {
    state.annotations.push(annotation);
    state.selectedId = annotation.id;
    saveDraft();
  }
  state.drawing = null;
  state.preview = null;
  render();
}

canvas.addEventListener("pointerup", () => finishDrawing());
canvas.addEventListener("pointercancel", () => finishDrawing());

textEditor.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    event.preventDefault();
    cancelTextEditor();
  } else if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    commitTextEditor();
  }
});
textEditor.addEventListener("blur", commitTextEditor);

document.addEventListener("keydown", (event) => {
  const target = event.target;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) return;
  if (event.ctrlKey || event.metaKey) {
    if (event.key.toLowerCase() === "z") {
      event.preventDefault();
      if (event.shiftKey) redo();
      else undo();
    } else if (event.key.toLowerCase() === "y") {
      event.preventDefault();
      redo();
    }
    return;
  }
  const map = { v: "select", r: "rect", a: "arrow", p: "pen", t: "text" };
  if (map[event.key.toLowerCase()]) {
    event.preventDefault();
    setTool(map[event.key.toLowerCase()]);
  } else if (event.key === "Delete" || event.key === "Backspace") {
    event.preventDefault();
    removeSelected();
  }
});

for (const button of document.querySelectorAll(".tool")) {
  button.addEventListener("click", () => setTool(button.dataset.tool));
}
$("undo").addEventListener("click", undo);
$("redo").addEventListener("click", redo);
$("delete").addEventListener("click", removeSelected);
$("clear").addEventListener("click", clearAll);
$("color").addEventListener("input", (event) => {
  state.color = event.target.value;
  if (state.overlayMode) overlayCommand({ type: "set-color", color: state.color });
});
$("width").addEventListener("change", (event) => {
  state.strokeWidth = Number(event.target.value) || 3;
  if (state.overlayMode) {
    overlayCommand({ type: "set-width", width: state.strokeWidth });
  }
});
noteInput.addEventListener("input", () => {
  if (state.overlayMode) {
    if (state.overlayActive) {
      overlayCommand({ type: "set-note", note: noteInput.value });
    } else {
      saveDraft();
    }
    return;
  }
  saveDraft();
});

async function loadCapture(capture) {
  try {
    setBusy(true);
    setStatus("Loading capture…");
    const config = await loadConfig();
    if (!isConfigured(config)) throw new Error("Connect RedLine in the extension options first");
    if (capture.serverUrl && capture.serverUrl !== config.serverUrl) {
      throw new Error(`This capture belongs to ${capture.serverUrl}; reconnect that server.`);
    }
    const api = new RedLineApi(config);
    const payload = await api.session(capture.sessionId);
    const meta = payload.meta;

    if (state.video) state.video.pause();
    if (state.videoUrl) URL.revokeObjectURL(state.videoUrl);
    state.videoUrl = null;
    state.video = null;
    state.trace = null;
    state.traceEvents = [];
    state.bitmap = null;
    state.mediaKind = meta.media?.kind === "video" ? "video" : "image";
    state.overlayMode = false;
    state.overlayActive = false;
    state.local = false;
    state.localData = null;
    state.config = config;
    state.capture = capture;
    state.api = api;
    state.meta = meta;
    state.annotations = Array.isArray(payload.annotations?.annotations)
      ? payload.annotations.annotations
      : [];
    state.selectedId = null;
    state.undo = [];
    state.redo = [];
    state.role = meta.my_role || "owner";
    state.requested = Boolean(meta.agent_requested_at);
    state.readOnly = meta.status !== "open" || state.role === "viewer";
    state.submitted = meta.status === "submitted" || meta.status === "resolved";
    state.width = Number(meta.media?.width) || 0;
    state.height = Number(meta.media?.height) || 0;
    noteInput.value = meta.prompt || "";

    if (!state.readOnly && state.mediaKind === "video") {
      const stored = await chrome.storage.session.get(draftKey());
      const draft = stored[draftKey()];
      if (draft && Array.isArray(draft.annotations)) {
        state.annotations = draft.annotations;
        noteInput.value = draft.note || "";
        setStatus("Restored an unsent draft.");
      }
    }

    if (state.mediaKind === "image") {
      // The page is the canvas: the side panel keeps the controls, the overlay owns the drawing
      // and the submit payload.
      state.overlayMode = true;
      state.overlayActive = false;
      state.overlayTabId = capture.tabId || null;
      state.undoCount = 0;
      state.redoCount = 0;
      state.bitmap = null;
      $("title").textContent = meta.title || "Screenshot";
      $("subtitle").textContent = state.readOnly
        ? `Screenshot · ${meta.status}`
        : "Screenshot";
      showWorkspace();
      renderOverlayControls();
      $("overlay-banner").hidden = false;
      $("overlay-banner-text").textContent = "Opening the page overlay…";
      setStatus("Opening the page overlay…");
      try {
        await injectOverlay(capture);
        setStatus(
          state.readOnly
            ? "This review is already submitted; open it in RedLine."
            : "Annotate on the page. Controls are here.",
        );
      } catch (error) {
        setStatus(error.message || String(error), "err");
      }
      return;
    }

    if (state.mediaKind === "video") {
      const blob = await api.videoBlob(capture.sessionId);
      state.videoUrl = URL.createObjectURL(blob);
      video.src = state.videoUrl;
      await new Promise((resolve, reject) => {
        video.addEventListener("loadedmetadata", resolve, { once: true });
        video.addEventListener("error", () => reject(new Error("cannot load video")), {
          once: true,
        });
      });
      state.video = video;
      state.duration =
        Number.isFinite(video.duration) && video.duration > 0
          ? video.duration
          : (Number(meta.media?.duration_ms) || 0) / 1000;
      state.width = video.videoWidth || state.width;
      state.height = video.videoHeight || state.height;
      video.currentTime = 0;
      if (meta.trace) {
        try {
          state.trace = await api.trace(capture.sessionId);
          state.traceEvents = Array.isArray(state.trace.events) ? state.trace.events : [];
        } catch {
          state.traceEvents = [];
        }
      }
      $("trace-filter").value = state.traceFilter;
      $("ghosts").checked = state.ghosts;
      renderTrace();
    } else {
      const blob = await api.imageBlob(capture.sessionId);
      state.bitmap = await createImageBitmap(blob);
      state.width = state.bitmap.width;
      state.height = state.bitmap.height;
    }

    $("title").textContent = meta.title || (state.mediaKind === "video" ? "Browser flow" : "Screenshot");
    $("subtitle").textContent = `${state.width}×${state.height}${
      state.mediaKind === "video" ? ` · ${formatTime(state.duration)}` : ""
    }${state.readOnly ? ` · ${meta.status}` : ""}`;
    showWorkspace();
    setTool("rect");
    render();
    setStatus(state.readOnly ? "This review is already submitted; open it in RedLine." : "");
  } catch (error) {
    showEmpty(error.message || String(error));
  } finally {
    setBusy(false);
    render();
  }
}

async function loadLocalCapture(capture) {
  try {
    setBusy(true);
    setStatus("Loading local capture…");
    const local = await getCapture(capture.id || capture.sessionId);
    if (!local) throw new Error("Local capture not found");
    if (state.video) state.video.pause();
    if (state.videoUrl) URL.revokeObjectURL(state.videoUrl);
    state.videoUrl = null;
    state.video = null;
    state.trace = null;
    state.traceEvents = [];
    state.bitmap = null;
    state.local = true;
    state.overlayMode = false;
    state.overlayActive = false;
    state.localData = local;
    state.role = "owner";
    state.requested = false;
    state.config = null;
    state.api = null;
    state.capture = {
      ...capture,
      id: local.id,
      sessionId: local.id,
      mode: "local",
      kind: local.kind,
    };
    const media =
      local.kind === "video"
        ? {
            kind: "video",
            width: local.width,
            height: local.height,
            duration_ms: Math.round(Number(local.durationMs) || 0),
            mime: "video/webm",
          }
        : null;
    state.meta = {
      id: local.id,
      title: local.title,
      url: local.url,
      note: local.note,
      status: "open",
      media,
    };
    state.mediaKind = local.kind === "video" ? "video" : "image";
    state.annotations = Array.isArray(local.annotations) ? local.annotations : [];
    state.selectedId = null;
    state.undo = [];
    state.redo = [];
    state.readOnly = false;
    state.submitted = false;
    state.width = Number(local.width) || 0;
    state.height = Number(local.height) || 0;
    noteInput.value = local.note || "";

    const stored = await chrome.storage.session.get(draftKey());
    const draft = stored[draftKey()];
    if (draft && Array.isArray(draft.annotations)) {
      state.annotations = draft.annotations;
      noteInput.value = draft.note || "";
      setStatus("Restored an unsent draft.");
    }

    if (state.mediaKind === "video") {
      if (!local.videoBlob) throw new Error("Local video is missing");
      state.videoUrl = URL.createObjectURL(local.videoBlob);
      video.src = state.videoUrl;
      await new Promise((resolve, reject) => {
        video.addEventListener("loadedmetadata", resolve, { once: true });
        video.addEventListener("error", () => reject(new Error("cannot load video")), {
          once: true,
        });
      });
      state.video = video;
      state.duration =
        Number.isFinite(video.duration) && video.duration > 0
          ? video.duration
          : (Number(local.durationMs) || 0) / 1000;
      state.width = video.videoWidth || state.width;
      state.height = video.videoHeight || state.height;
      video.currentTime = 0;
      state.trace = local.trace || null;
      state.traceEvents = Array.isArray(local.trace?.events) ? local.trace.events : [];
      $("trace-filter").value = state.traceFilter;
      $("ghosts").checked = state.ghosts;
      renderTrace();
    } else {
      if (!local.imageBlob) throw new Error("Local image is missing");
      state.bitmap = await createImageBitmap(local.imageBlob);
      state.width = state.bitmap.width;
      state.height = state.bitmap.height;
    }

    $("title").textContent =
      local.title || (state.mediaKind === "video" ? "Browser flow" : "Screenshot");
    $("subtitle").textContent = `${state.width}×${state.height}${
      state.mediaKind === "video" ? ` · ${formatTime(state.duration)}` : ""
    } · local`;
    $("open-capture").hidden = true;
    showWorkspace();
    setTool("rect");
    render();
    if (!draft) setStatus("Local capture — export a folder or ZIP when done.");
  } catch (error) {
    showEmpty(error.message || String(error));
  } finally {
    setBusy(false);
    render();
  }
}

async function loadActiveCapture() {
  const response = await chrome.runtime.sendMessage({ type: "get-active-capture" });
  if (response?.capture?.mode === "local") await loadLocalCapture(response.capture);
  else if (response?.capture) await loadCapture(response.capture);
  else {
    const config = await loadConfig();
    showEmpty(
      isConfigured(config) ? "" : "Connect RedLine for server mode, or save a local capture.",
    );
  }
}

async function captureCurrent() {
  const config = await loadConfig().catch(() => null);
  if (!isConfigured(config)) {
    await openSettings();
    setSettingsStatus("Connect RedLine first, then click Screenshot current tab.", "err");
    return;
  }
  if (state.annotations.length && !state.readOnly) {
    if (!confirm("Discard the unsent annotations and capture the current tab?")) return;
  }
  try {
    setBusy(true);
    setStatus("Capturing…");
    const response = await chrome.runtime.sendMessage({ type: "capture-and-create" });
    if (!response?.ok) throw new Error(response?.error || "Capture failed");
    await loadCapture(response.capture);
  } catch (error) {
    const message = error.message || String(error);
    setStatus(
      /activeTab|all_urls/i.test(message)
        ? "This tab was not opened through the RedLine icon. Click the icon on the page tab, then press Screenshot again."
        : message,
      "err",
    );
  } finally {
    setBusy(false);
  }
}

async function captureOffline() {
  if (state.annotations.length && !state.readOnly) {
    if (!confirm("Discard the unsent annotations and capture the current tab offline?")) return;
  }
  try {
    setBusy(true);
    setStatus("Capturing offline…");
    const response = await chrome.runtime.sendMessage({ type: "capture-local" });
    if (!response?.ok) throw new Error(response?.error || "Capture failed");
    await loadLocalCapture(response.capture);
  } catch (error) {
    const message = error.message || String(error);
    setStatus(
      /activeTab|all_urls/i.test(message)
        ? "This tab was not opened through the RedLine icon. Click the icon on the page tab, then press Screenshot again."
        : message,
      "err",
    );
  } finally {
    setBusy(false);
  }
}

function canvasBlob(canvas) {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("cannot encode PNG"))), "image/png");
  });
}

async function seekVideoTo(seconds) {
  if (!state.video) return;
  const duration = state.duration || state.video.duration || seconds;
  const target = Math.max(0, Math.min(duration || seconds, seconds));
  if (Math.abs(state.video.currentTime - target) < 0.001) return;
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("video seek timed out")), 3000);
    state.video.addEventListener(
      "seeked",
      () => {
        clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
    state.video.currentTime = target;
  });
}

async function captureFrame(at) {
  await seekVideoTo(at);
  const frame = document.createElement("canvas");
  frame.width = mediaWidth();
  frame.height = mediaHeight();
  const frameCtx = frame.getContext("2d");
  frameCtx.drawImage(state.video, 0, 0, frame.width, frame.height);
  drawAnnotations(frameCtx, state.annotations, at);
  return { at, blob: await canvasBlob(frame) };
}

async function contactSheet(frames) {
  if (!frames.length) return null;
  const columns = Math.min(3, frames.length);
  const rows = Math.ceil(frames.length / columns);
  const tileWidth = 360;
  const tileHeight = Math.max(150, Math.round((tileWidth * mediaHeight()) / Math.max(1, mediaWidth())));
  const gap = 14;
  const header = 24;
  const sheet = document.createElement("canvas");
  sheet.width = columns * tileWidth + (columns + 1) * gap;
  sheet.height = rows * (tileHeight + header) + (rows + 1) * gap;
  const sheetCtx = sheet.getContext("2d");
  sheetCtx.fillStyle = "#0f172a";
  sheetCtx.fillRect(0, 0, sheet.width, sheet.height);
  for (let index = 0; index < frames.length; index += 1) {
    const frame = frames[index];
    const bitmap = await createImageBitmap(frame.blob);
    const column = index % columns;
    const row = Math.floor(index / columns);
    const x = gap + column * (tileWidth + gap);
    const y = gap + row * (tileHeight + header + gap);
    sheetCtx.drawImage(bitmap, x, y + header, tileWidth, tileHeight);
    sheetCtx.fillStyle = "#e6edf6";
    sheetCtx.font = "600 13px system-ui, sans-serif";
    sheetCtx.fillText(formatTime(frame.at), x, y + 16);
    bitmap.close?.();
  }
  return canvasBlob(sheet);
}

async function videoPackageFrames() {
  const times = [
    ...new Set(
      state.annotations.map((annotation) =>
        Math.round((Number(annotation.at) || 0) * 1000),
      ),
    ),
  ].map((value) => value / 1000);
  if (times.length > 20) {
    throw new Error("A video export supports at most 20 annotated timestamps");
  }
  setStatus("Capturing annotated frames…");
  state.video.pause();
  const frames = [];
  for (const at of times) frames.push(await captureFrame(at));
  const sheet = await contactSheet(frames);
  return { frames, sheet };
}

async function submitVideo(request = false) {
  const { frames, sheet } = await videoPackageFrames();
  await state.api.submitVideo(
    state.capture.sessionId,
    {
      annotations: state.annotations,
      width: mediaWidth(),
      height: mediaHeight(),
      contactSheet: sheet,
      frames,
      note: noteInput.value.trim(),
    },
    { request },
  );
}

function dataUrlBlob(dataUrl) {
  return fetch(dataUrl).then((response) => response.blob());
}

async function writeDirectoryFile(dir, name, data) {
  const parts = String(name).split("/").filter(Boolean);
  let handle = dir;
  for (const part of parts.slice(0, -1)) {
    handle = await handle.getDirectoryHandle(part, { create: true });
  }
  const fileHandle = await handle.getFileHandle(parts[parts.length - 1], { create: true });
  const writable = await fileHandle.createWritable();
  await writable.write(data);
  await writable.close();
}

function downloadBlob(blob, name) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
}

async function savePackage(files) {
  const picker =
    globalThis.__redlineTestPicker ||
    (typeof globalThis.showDirectoryPicker === "function"
      ? globalThis.showDirectoryPicker.bind(globalThis)
      : null);
  if (picker) {
    try {
      const dir = await picker({ mode: "readwrite", id: "redline-review" });
      for (const file of files) await writeDirectoryFile(dir, file.name, file.data);
      return { kind: "folder" };
    } catch (error) {
      if (error?.name === "AbortError") return { kind: "canceled" };
      // The picker is unavailable in this context; fall back to a ZIP download.
    }
  }
  const zip = await buildZip(files);
  downloadBlob(zip, `redline-review-${state.capture.sessionId}.zip`);
  return { kind: "zip" };
}

async function exportLocal() {
  if (!state.local || !state.localData) throw new Error("No local capture to export");
  const local = state.localData;
  const note = noteInput.value.trim();
  const media =
    state.mediaKind === "video"
      ? {
          kind: "video",
          width: state.width,
          height: state.height,
          duration_ms: Math.round((state.duration || 0) * 1000),
          mime: "video/webm",
        }
      : null;
  let frames = [];
  let contact = null;
  let annotated = null;
  if (state.mediaKind === "video") {
    const captured = await videoPackageFrames();
    frames = captured.frames;
    contact = captured.sheet;
  } else {
    annotated = await dataUrlBlob(composePng(state.bitmap, state.annotations));
  }

  const trace = local.trace || null;
  const markdown = buildReviewMarkdown({
    id: state.capture.sessionId,
    title: state.meta.title,
    url: state.meta.url,
    note,
    media,
    width: state.width,
    height: state.height,
    annotations: state.annotations,
    frames,
    trace,
  });
  const annotationsJson = buildAnnotationsJson({
    id: state.capture.sessionId,
    title: state.meta.title,
    url: state.meta.url,
    note,
    media,
    width: state.width,
    height: state.height,
    annotations: state.annotations,
    frames,
    trace,
  });
  if (media && !local.posterBlob) delete annotationsJson.image;

  const files = [
    { name: "review.md", data: markdown },
    { name: "annotations.json", data: JSON.stringify(annotationsJson, null, 2) },
    { name: "AI-PROMPT.md", data: AI_PROMPT },
  ];
  if (trace) files.push({ name: "trace.json", data: JSON.stringify(trace, null, 2) });
  if (annotated) files.push({ name: "annotated.png", data: annotated });
  if (media) {
    files.push({ name: "video.webm", data: local.videoBlob });
    if (local.posterBlob) files.push({ name: "poster.png", data: local.posterBlob });
    if (contact) files.push({ name: "contact-sheet.png", data: contact });
    for (const frame of frames) {
      files.push({ name: frameFileName(frame.at), data: frame.blob });
    }
  }

  setStatus("Saving review…");
  const result = await savePackage(files);
  if (result.kind === "canceled") {
    setStatus("Export canceled.");
    return;
  }
  const updated = { ...local, annotations: state.annotations, note };
  await putCapture(updated);
  state.localData = updated;
  await chrome.storage.session.remove(draftKey());
  setStatus(
    result.kind === "folder" ? "Exported review folder." : "Downloaded review ZIP.",
    "ok",
  );
  render();
}

function submitOverlay() {
  if (state.readOnly || state.submitting || !state.capture) return;
  if (!state.overlayActive) {
    resumeOverlay();
    return;
  }
  if (!state.annotations.length && !noteInput.value.trim()) {
    if (!confirm("There are no annotations and no note. Send the capture anyway?")) return;
  }
  state.submitting = true;
  setStatus(
    state.role === "tester" ? "Saving for developer review…" : "Submitting…",
  );
  renderOverlayControls();
  sendOverlayCommand({ type: "submit", note: noteInput.value });
}

async function discardOverlay() {
  if (!state.capture || state.submitting || state.submitted) return;
  if (!confirm("Delete this review and its screenshot?")) return;
  if (!state.overlayActive) {
    try {
      state.submitting = true;
      renderOverlayControls();
      await state.api.deleteSession(state.capture.sessionId);
      await chrome.storage.session.remove(draftKey());
      await chrome.runtime.sendMessage({ type: "clear-active-capture" });
      showEmpty("Task discarded.");
    } catch (error) {
      state.submitting = false;
      setStatus(error.message || String(error), "err");
      renderOverlayControls();
    }
    return;
  }
  state.submitting = true;
  setStatus("Discarding…");
  renderOverlayControls();
  sendOverlayCommand({ type: "discard" });
}

function showSent(message, keepCapture = false) {
  $("workspace").hidden = true;
  $("empty").hidden = false;
  $("open-capture").hidden = !keepCapture || !state.capture;
  $("video-bar").hidden = true;
  $("trace-panel").hidden = true;
  $("overlay-banner").hidden = true;
  $("pause-overlay").hidden = true;
  $("capture-new").hidden = true;
  $("resume-overlay").hidden = true;
  $("stage").hidden = false;
  $("title").textContent = "RedLine Capture";
  $("subtitle").textContent = "No capture yet";
  $("empty-status").textContent = message;
  $("empty-status").className = "status ok";
  state.overlayMode = false;
  state.overlayActive = false;
  state.submitting = false;
  state.submitted = false;
  state.readOnly = true;
  state.annotations = [];
  state.selectedId = null;
  state.undoCount = 0;
  state.redoCount = 0;
  setStatus(message, "ok");
  refreshCaptureContext().catch(() => {});
}

async function submit() {
  if (state.overlayMode) return submitOverlay();
  if (state.readOnly || !state.capture) return;
  if (!state.annotations.length && !noteInput.value.trim()) {
    if (
      !confirm(
        state.local
          ? "There are no annotations and no note. Export the capture anyway?"
          : "There are no annotations and no note. Send the capture anyway?",
      )
    ) {
      return;
    }
  }
  try {
    setBusy(true);
    if (state.local) {
      await exportLocal();
      return;
    }
    if (!state.api) throw new Error("Connect RedLine before submitting.");
    const request = state.role === "tester";
    setStatus(request ? "Saving for developer review…" : "Submitting…");
    if (state.mediaKind === "video") {
      await submitVideo(request);
    } else {
      const png = composePng(state.bitmap, state.annotations);
      await state.api.submit(
        state.capture.sessionId,
        {
          annotations: state.annotations,
          width: mediaWidth(),
          height: mediaHeight(),
          png,
          note: noteInput.value.trim(),
        },
        { request },
      );
    }
    await chrome.storage.session.remove(draftKey());
    if (request) {
      // The task stays open so a developer can review and send it; the tester may update it.
      state.requested = true;
      state.readOnly = false;
      state.submitted = false;
      render();
      setStatus("Sent to a developer for review.", "ok");
    } else {
      state.readOnly = true;
      state.submitted = true;
      render();
      setStatus("Submitted to RedLine. The agent queue has it.", "ok");
    }
  } catch (error) {
    setStatus(error.message || String(error), "err");
  } finally {
    setBusy(false);
    render();
  }
}

async function discard() {
  if (state.overlayMode) return discardOverlay();
  if (!state.capture) return;
  const local = state.local;
  if (!confirm(local ? "Delete this local review and its files?" : "Delete this review and its screenshot?")) {
    return;
  }
  try {
    setBusy(true);
    if (local) {
      await deleteCapture(state.capture.sessionId);
    } else {
      if (!state.api) throw new Error("Connect RedLine first.");
      await state.api.deleteSession(state.capture.sessionId);
    }
    await chrome.storage.session.remove(draftKey());
    await chrome.runtime.sendMessage({ type: "clear-active-capture" });
    if (state.video) state.video.pause();
    if (state.videoUrl) URL.revokeObjectURL(state.videoUrl);
    state.capture = null;
    state.meta = null;
    state.bitmap = null;
    state.local = false;
    state.localData = null;
    state.annotations = [];
    state.video = null;
    state.videoUrl = null;
    showEmpty();
  } catch (error) {
    setStatus(error.message || String(error), "err");
  } finally {
    setBusy(false);
  }
}

function openRedLine() {
  if (state.local || !state.capture) return;
  const url =
    state.capture.reviewUrl ||
    `${state.config.serverUrl}/s/${encodeURIComponent(state.capture.sessionId)}?t=${encodeURIComponent(
      state.config.token,
    )}`;
  chrome.tabs.create({ url });
}

$("capture").addEventListener("click", captureCurrent);
$("capture-offline").addEventListener("click", captureOffline);
$("capture-new").addEventListener("click", captureCurrent);
$("resume-overlay").addEventListener("click", resumeOverlay);
$("pause-overlay").addEventListener("click", () => {
  overlayCommand({ type: "close" });
});
$("open-capture").addEventListener("click", openRedLine);
$("open-options").addEventListener("click", openSettings);
$("settings").addEventListener("click", () => {
  if ($("settings-panel").hidden) openSettings();
  else closeSettings();
});
$("settings-close").addEventListener("click", closeSettings);
$("settings-save").addEventListener("click", () => {
  saveSettings().catch((error) => setSettingsStatus(error.message || String(error), "err"));
});
$("settings-test").addEventListener("click", () => {
  testSettings().catch((error) => setSettingsStatus(error.message || String(error), "err"));
});
$("submit").addEventListener("click", submit);
$("discard").addEventListener("click", discard);
$("open-redline").addEventListener("click", openRedLine);

chrome.runtime.onMessage.addListener((message) => {
  if (message?.type === "redline-overlay-state") {
    if (message.sessionId !== state.capture?.sessionId) return;
    applyOverlayState(message);
    return;
  }
  if (message?.type === "capture-created" && message.capture) {
    if (state.capture?.sessionId === message.capture.sessionId) return;
    if (message.capture.mode === "local") loadLocalCapture(message.capture);
    else loadCapture(message.capture);
  }
});

chrome.tabs.onActivated.addListener(() => {
  refreshCaptureContext().catch(() => {});
});
chrome.tabs.onUpdated.addListener((_tabId, changeInfo) => {
  if (changeInfo.status || changeInfo.url) refreshCaptureContext().catch(() => {});
});

loadActiveCapture().catch((error) => showEmpty(error.message || String(error)));
