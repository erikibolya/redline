import { RedLineApi } from "./lib/api.js";
import { isConfigured, loadConfig } from "./lib/config.js";
import {
  composePng,
  createAnnotation,
  drawScene,
  finishAnnotation,
  hitTest,
  moveAnnotation,
  TOOLS,
} from "./lib/annotations.js";

const $ = (id) => document.getElementById(id);
const canvas = $("canvas");
const textEditor = $("text-editor");
const statusEl = $("status");
const closeButton = $("close");

const hash = new URLSearchParams(location.hash.replace(/^#/, ""));
const state = {
  sessionId: hash.get("session") || "",
  config: null,
  api: null,
  capture: null,
  meta: null,
  bitmap: null,
  width: 0,
  height: 0,
  annotations: [],
  selectedId: null,
  tool: "rect",
  color: "#e11d48",
  strokeWidth: 3,
  undo: [],
  redo: [],
  drawing: null,
  preview: null,
  drag: null,
  textPoint: null,
  readOnly: false,
  submitting: false,
  role: "owner",
  requested: false,
  note: "",
  saveTimer: null,
};

function setStatus(text, kind = "") {
  statusEl.textContent = text;
  statusEl.className = `status ${kind}`.trim();
}

function draftKey() {
  return `draft:${state.sessionId}`;
}

function summary() {
  return {
    sessionId: state.sessionId,
    annotations: state.annotations.map((annotation) => ({
      id: annotation.id,
      type: annotation.type,
      text: annotation.text || "",
      at: annotation.at ?? null,
      color: annotation.color,
      width: annotation.width,
    })),
    selectedId: state.selectedId,
    tool: state.tool,
    color: state.color,
    width: state.strokeWidth,
    undo: state.undo.length,
    redo: state.redo.length,
    count: state.annotations.length,
    widthPx: state.width,
    heightPx: state.height,
    note: state.note,
    readOnly: state.readOnly,
    role: state.role,
    requested: state.requested,
    status: state.meta?.status || "open",
    title: state.meta?.title || "",
    submitting: state.submitting,
  };
}

function publish(event = "state", extra = {}) {
  chrome.runtime
    .sendMessage({
      type: "redline-overlay-state",
      sessionId: state.sessionId,
      event,
      state: summary(),
      ...extra,
    })
    .catch(() => {});
}

async function saveDraftNow() {
  clearTimeout(state.saveTimer);
  state.saveTimer = null;
  if (state.readOnly) return;
  await chrome.storage.session
    .set({
      [draftKey()]: {
        annotations: state.annotations,
        note: state.note,
        tool: state.tool,
        color: state.color,
        width: state.strokeWidth,
        updatedAt: Date.now(),
      },
    })
    .catch(() => {});
}

function saveDraft() {
  if (state.readOnly) return;
  clearTimeout(state.saveTimer);
  state.saveTimer = setTimeout(() => {
    void saveDraftNow();
  }, 250);
}

function render() {
  if (state.bitmap) {
    drawScene(canvas, state.bitmap, state.annotations, state.selectedId, state.preview);
  }
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
  publish();
}

function redo() {
  if (state.readOnly || !state.redo.length) return;
  state.undo.push(JSON.stringify(state.annotations));
  state.annotations = JSON.parse(state.redo.pop());
  state.selectedId = null;
  render();
  saveDraft();
  publish();
}

function removeSelected() {
  if (state.readOnly || !state.selectedId) return;
  snapshot();
  state.annotations = state.annotations.filter((annotation) => annotation.id !== state.selectedId);
  state.selectedId = null;
  render();
  saveDraft();
  publish();
}

function clearAll() {
  if (state.readOnly || !state.annotations.length) return;
  snapshot();
  state.annotations = [];
  state.selectedId = null;
  render();
  saveDraft();
  publish();
}

function setTool(tool) {
  if (!TOOLS.includes(tool) || state.readOnly) return;
  state.tool = tool;
  canvas.style.cursor = tool === "select" ? "default" : "crosshair";
  document.body.dataset.tool = state.tool;
  publish();
}

function setColor(color) {
  if (state.readOnly || !/^#[0-9a-f]{6}$/i.test(String(color || ""))) return;
  state.color = color;
  publish();
}

function setWidth(width) {
  const value = Number(width);
  if (state.readOnly || !Number.isFinite(value) || value < 1 || value > 40) return;
  state.strokeWidth = value;
  publish();
}

function selectAnnotation(id) {
  state.selectedId = id || null;
  render();
  publish();
}

function pointFromEvent(event) {
  const rect = canvas.getBoundingClientRect();
  return {
    x: ((event.clientX - rect.left) * canvas.width) / rect.width,
    y: ((event.clientY - rect.top) * canvas.height) / rect.height,
  };
}

function openTextEditor(point) {
  state.textPoint = point;
  const rect = canvas.getBoundingClientRect();
  const left = rect.left + (point.x * rect.width) / canvas.width;
  const top = rect.top + (point.y * rect.height) / canvas.height;
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
    state.annotations.push(annotation);
    state.selectedId = annotation.id;
  }
  textEditor.hidden = true;
  state.textPoint = null;
  render();
  saveDraft();
  publish();
  textCommitInFlight = false;
}

function cancelTextEditor() {
  textEditor.hidden = true;
  state.textPoint = null;
}

canvas.addEventListener("pointerdown", (event) => {
  if (state.readOnly || !state.bitmap) return;
  event.preventDefault();
  canvas.focus();
  const point = pointFromEvent(event);
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
    publish();
    return;
  }
  snapshot();
  state.drawing = createAnnotation(state.tool, point, {
    color: state.color,
    width: state.strokeWidth,
  });
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
    publish();
    return;
  }
  if (!state.drawing) return;
  const annotation = finishAnnotation(state.drawing, state.drawing.end, state.drawing.points);
  const bounds = annotation.start
    ? Math.abs(annotation.end.x - annotation.start.x)
      + Math.abs(annotation.end.y - annotation.start.y)
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
  publish();
}

canvas.addEventListener("pointerup", finishDrawing);
canvas.addEventListener("pointercancel", finishDrawing);

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
  } else if (event.key === "Escape") {
    event.preventDefault();
    void closePaused();
  }
});

closeButton.addEventListener("click", () => { void closePaused(); });

async function command(command) {
  if (!command || typeof command !== "object") return;
  switch (command.type) {
    case "set-tool":
      setTool(command.tool);
      break;
    case "set-color":
      setColor(command.color);
      break;
    case "set-width":
      setWidth(command.width);
      break;
    case "undo":
      undo();
      break;
    case "redo":
      redo();
      break;
    case "delete":
      removeSelected();
      break;
    case "clear":
      clearAll();
      break;
    case "select":
      selectAnnotation(command.id);
      break;
    case "set-note":
      state.note = String(command.note || "");
      saveDraft();
      break;
    case "request-state":
      publish("sync");
      break;
    case "submit":
      await submit(command.note);
      break;
    case "discard":
      await discard();
      break;
    case "close":
      await closePaused();
      break;
    default:
      break;
  }
}

chrome.runtime.onMessage.addListener((message) => {
  if (message?.type === "redline-overlay-escape") {
    if (message.sessionId === state.sessionId) closePaused();
    return;
  }
  if (message?.type !== "redline-overlay-command") return;
  if (message.sessionId !== state.sessionId) return;
  command(message.command);
});

async function submit(note) {
  if (state.readOnly || state.submitting) return;
  state.note = String(note ?? state.note ?? "");
  state.submitting = true;
  setStatus("Submitting…");
  publish("submitting");
  try {
    const png = composePng(state.bitmap, state.annotations);
    const request = state.role === "tester";
    const result = await state.api.submit(
      state.sessionId,
      {
        annotations: state.annotations,
        width: state.width,
        height: state.height,
        png,
        note: state.note,
      },
      { request },
    );
    state.meta = result.meta;
    state.requested = Boolean(result.meta.agent_requested_at);
    state.readOnly = result.meta.status !== "open" || state.role === "viewer";
    state.submitting = false;
    await chrome.storage.session.remove(draftKey());
    setStatus(request ? "Sent to a developer." : "Submitted to RedLine.", "ok");
    publish("submitted", { meta: result.meta });
    closeOverlay();
  } catch (error) {
    state.submitting = false;
    const message = error.message || String(error);
    setStatus(message, "err");
    publish("error", { error: message });
  }
}

async function discard() {
  if (state.submitting) return;
  try {
    await state.api.deleteSession(state.sessionId);
    await chrome.storage.session.remove(draftKey());
    publish("discarded");
    closeOverlay();
  } catch (error) {
    const message = error.message || String(error);
    setStatus(message, "err");
    publish("error", { error: message });
  }
}

async function closePaused() {
  if (state.submitting) return;
  await saveDraftNow();
  publish("paused");
  closeOverlay();
}

function closeOverlay() {
  chrome.runtime
    .sendMessage({ type: "redline-overlay-request-close", sessionId: state.sessionId })
    .catch(() => {});
}

async function load() {
  try {
    if (!state.sessionId) {
      const stored = await chrome.storage.session.get("activeCapture");
      state.sessionId = stored.activeCapture?.sessionId || "";
    }
    if (!state.sessionId) throw new Error("No active capture.");
    const config = await loadConfig();
    if (!isConfigured(config)) {
      throw new Error("Connect RedLine in the extension options first.");
    }
    state.config = config;
    state.api = new RedLineApi(config);
    const payload = await state.api.session(state.sessionId);
    state.meta = payload.meta;
    if (state.meta.media?.kind === "video") {
      throw new Error("Video reviews open in the RedLine editor; the overlay is for screenshots.");
    }
    state.role = state.meta.my_role || "owner";
    state.requested = Boolean(state.meta.agent_requested_at);
    state.readOnly = state.meta.status !== "open" || state.role === "viewer";
    state.note = state.meta.prompt || "";
    state.annotations = Array.isArray(payload.annotations?.annotations)
      ? payload.annotations.annotations
      : [];

    if (!state.readOnly) {
      const stored = await chrome.storage.session.get(draftKey());
      const draft = stored[draftKey()];
      if (draft && Array.isArray(draft.annotations)) {
        state.annotations = draft.annotations;
        state.note = draft.note || state.note;
        if (TOOLS.includes(draft.tool)) state.tool = draft.tool;
        if (/^#[0-9a-f]{6}$/i.test(draft.color || "")) state.color = draft.color;
        if (Number.isFinite(Number(draft.width)) && Number(draft.width) > 0) {
          state.strokeWidth = Number(draft.width);
        }
        setStatus("Restored an unsent draft.");
      }
    }

    const blob = await state.api.imageBlob(state.sessionId);
    state.bitmap = await createImageBitmap(blob);
    state.width = state.bitmap.width;
    state.height = state.bitmap.height;
    canvas.style.cursor = state.tool === "select" ? "default" : "crosshair";
    document.body.dataset.tool = state.tool;
    render();
    canvas.focus();
    if (state.readOnly) {
      setStatus(`This task is ${state.meta.status}; it is read-only here.`, "ok");
    }
    publish("ready");
  } catch (error) {
    const message = error.message || String(error);
    setStatus(message, "err");
    publish("error", { error: message });
  }
}

window.addEventListener("pagehide", () => { void saveDraftNow(); });
window.addEventListener("beforeunload", () => { void saveDraftNow(); });

load();
