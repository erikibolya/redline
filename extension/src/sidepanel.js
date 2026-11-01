import { RedLineApi } from "./lib/api.js";
import { isConfigured, loadConfig } from "./lib/config.js";
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
const video = $("video");

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
  saveTimer: null,
};

function setStatus(text, kind = "") {
  statusEl.textContent = text;
  statusEl.className = `status ${kind}`.trim();
}

function setBusy(busy) {
  for (const element of document.querySelectorAll("button, input, select, textarea")) {
    if (element.id === "settings") continue;
    element.disabled = busy || (state.readOnly && !["open-redline", "settings"].includes(element.id));
  }
  if (!busy && state.readOnly) {
    $("open-redline").disabled = false;
    $("settings").disabled = false;
  }
}

function showEmpty(message = "") {
  $("workspace").hidden = true;
  $("empty").hidden = false;
  $("open-capture").hidden = true;
  $("video-bar").hidden = true;
  $("trace-panel").hidden = true;
  $("title").textContent = "RedLine Capture";
  $("subtitle").textContent = "No capture yet";
  if (state.video) state.video.pause();
  if (message) setStatus(message, "err");
}

function showWorkspace() {
  $("empty").hidden = true;
  $("workspace").hidden = false;
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
          updatedAt: Date.now(),
        },
      })
      .catch(() => {});
  }, 250);
}

function render() {
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
  if (state.readOnly || !state.undo.length) return;
  state.redo.push(JSON.stringify(state.annotations));
  state.annotations = JSON.parse(state.undo.pop());
  state.selectedId = null;
  render();
  saveDraft();
}

function redo() {
  if (state.readOnly || !state.redo.length) return;
  state.undo.push(JSON.stringify(state.annotations));
  state.annotations = JSON.parse(state.redo.pop());
  state.selectedId = null;
  render();
  saveDraft();
}

function removeSelected() {
  if (state.readOnly || !state.selectedId) return;
  snapshot();
  state.annotations = state.annotations.filter((annotation) => annotation.id !== state.selectedId);
  state.selectedId = null;
  render();
  saveDraft();
}

function clearAll() {
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
});
$("width").addEventListener("change", (event) => {
  state.strokeWidth = Number(event.target.value) || 3;
});
noteInput.addEventListener("input", saveDraft);

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
    state.readOnly = meta.status !== "open";
    state.submitted = meta.status === "submitted" || meta.status === "resolved";
    state.width = Number(meta.media?.width) || 0;
    state.height = Number(meta.media?.height) || 0;
    noteInput.value = meta.prompt || "";

    if (!state.readOnly) {
      const stored = await chrome.storage.session.get(draftKey());
      const draft = stored[draftKey()];
      if (draft && Array.isArray(draft.annotations)) {
        state.annotations = draft.annotations;
        noteInput.value = draft.note || "";
        setStatus("Restored an unsent draft.");
      }
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

async function loadActiveCapture() {
  const response = await chrome.runtime.sendMessage({ type: "get-active-capture" });
  if (response?.capture) await loadCapture(response.capture);
  else {
    const config = await loadConfig();
    if (!isConfigured(config)) setStatus("Connect RedLine in the extension options first.", "err");
    showEmpty();
  }
}

async function captureCurrent() {
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
    setStatus(error.message || String(error), "err");
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

async function submitVideo() {
  const times = [
    ...new Set(
      state.annotations.map((annotation) =>
        Math.round((Number(annotation.at) || 0) * 1000),
      ),
    ),
  ].map((value) => value / 1000);
  if (times.length > 20) {
    throw new Error("A video submit supports at most 20 annotated timestamps");
  }
  setStatus("Capturing annotated frames…");
  state.video.pause();
  const frames = [];
  for (const at of times) frames.push(await captureFrame(at));
  const sheet = await contactSheet(frames);
  await state.api.submitVideo(state.capture.sessionId, {
    annotations: state.annotations,
    width: mediaWidth(),
    height: mediaHeight(),
    contactSheet: sheet,
    frames,
    note: noteInput.value.trim(),
  });
}

async function submit() {
  if (state.readOnly || !state.api || !state.capture) return;
  if (!state.annotations.length && !noteInput.value.trim()) {
    if (!confirm("There are no annotations and no note. Send the capture anyway?")) return;
  }
  try {
    setBusy(true);
    setStatus("Submitting…");
    if (state.mediaKind === "video") {
      await submitVideo();
    } else {
      const png = composePng(state.bitmap, state.annotations);
      await state.api.submit(state.capture.sessionId, {
        annotations: state.annotations,
        width: mediaWidth(),
        height: mediaHeight(),
        png,
        note: noteInput.value.trim(),
      });
    }
    await chrome.storage.session.remove(draftKey());
    state.readOnly = true;
    state.submitted = true;
    render();
    setStatus("Submitted to RedLine. The agent queue has it.", "ok");
  } catch (error) {
    setStatus(error.message || String(error), "err");
  } finally {
    setBusy(false);
    render();
  }
}

async function discard() {
  if (!state.capture) return;
  if (!confirm("Delete this review and its screenshot?")) return;
  try {
    setBusy(true);
    await state.api.deleteSession(state.capture.sessionId);
    await chrome.storage.session.remove(draftKey());
    await chrome.runtime.sendMessage({ type: "clear-active-capture" });
    state.capture = null;
    state.meta = null;
    state.bitmap = null;
    state.annotations = [];
    showEmpty();
  } catch (error) {
    setStatus(error.message || String(error), "err");
  } finally {
    setBusy(false);
  }
}

function openRedLine() {
  if (!state.capture) return;
  const url =
    state.capture.reviewUrl ||
    `${state.config.serverUrl}/s/${encodeURIComponent(state.capture.sessionId)}?t=${encodeURIComponent(
      state.config.token,
    )}`;
  chrome.tabs.create({ url });
}

$("capture").addEventListener("click", captureCurrent);
$("open-capture").addEventListener("click", openRedLine);
$("open-options").addEventListener("click", () => chrome.runtime.openOptionsPage());
$("settings").addEventListener("click", () => chrome.runtime.openOptionsPage());
$("submit").addEventListener("click", submit);
$("discard").addEventListener("click", discard);
$("open-redline").addEventListener("click", openRedLine);

chrome.runtime.onMessage.addListener((message) => {
  if (message?.type === "capture-created" && message.capture) {
    loadCapture(message.capture);
  }
});

loadActiveCapture().catch((error) => showEmpty(error.message || String(error)));
