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
const ctx = canvas.getContext("2d");
const textEditor = $("text-editor");
const noteInput = $("note");
const statusEl = $("status");

const state = {
  config: null,
  capture: null,
  api: null,
  meta: null,
  bitmap: null,
  annotations: [],
  selectedId: null,
  tool: "select",
  color: "#e11d48",
  width: 3,
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
  $("title").textContent = "RedLine Capture";
  $("subtitle").textContent = "No capture yet";
  if (message) setStatus(message, "err");
}

function showWorkspace() {
  $("empty").hidden = true;
  $("workspace").hidden = false;
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
  if (!state.bitmap) return;
  drawScene(canvas, state.bitmap, state.annotations, state.selectedId, state.preview);
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
    text.textContent = typeLabel(annotation);
    row.append(number, text);
    row.addEventListener("click", () => {
      state.selectedId = annotation.id;
      render();
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
      width: state.width,
    });
    annotation.text = text;
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
  if (state.readOnly || !state.bitmap) return;
  event.preventDefault();
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
    return;
  }
  snapshot();
  state.drawing = createAnnotation(state.tool, point, {
    color: state.color,
    width: state.width,
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
  state.width = Number(event.target.value) || 3;
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
    if (meta.media?.kind === "video") {
      state.config = config;
      state.capture = capture;
      state.api = api;
      state.meta = meta;
      state.bitmap = null;
      state.annotations = [];
      showEmpty();
      $("open-capture").hidden = false;
      setStatus("Flow recorded and submitted. Open it in RedLine to annotate; the video annotator lands in Phase 4.", "ok");
      return;
    }
    const blob = await api.imageBlob(capture.sessionId);
    const bitmap = await createImageBitmap(blob);
    state.config = config;
    state.capture = capture;
    state.api = api;
    state.meta = meta;
    state.bitmap = bitmap;
    state.annotations = Array.isArray(payload.annotations?.annotations)
      ? payload.annotations.annotations
      : [];
    state.selectedId = null;
    state.undo = [];
    state.redo = [];
    state.readOnly = meta.status !== "open";
    state.submitted = meta.status === "submitted" || meta.status === "resolved";
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

    $("title").textContent = meta.title || "Screenshot";
    $("subtitle").textContent = `${bitmap.width}×${bitmap.height}${
      state.readOnly ? ` · ${meta.status}` : ""
    }`;
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

async function submit() {
  if (state.readOnly || !state.api || !state.bitmap) return;
  if (!state.annotations.length && !noteInput.value.trim()) {
    if (!confirm("There are no annotations and no note. Send the screenshot anyway?")) return;
  }
  try {
    setBusy(true);
    setStatus("Submitting…");
    const png = composePng(state.bitmap, state.annotations);
    await state.api.submit(state.capture.sessionId, {
      annotations: state.annotations,
      width: state.bitmap.width,
      height: state.bitmap.height,
      png,
      note: noteInput.value.trim(),
    });
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
