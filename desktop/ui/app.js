/* redline UI. No framework, no build step: the whole client is this file. */
"use strict";

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

const escapeHtml = (value) =>
  String(value ?? "").replace(/[&<>"']/g, (ch) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[ch]));

const clamp = (value, low, high) => Math.min(high, Math.max(low, value));

const plural = (count, word) => `${count} ${word}${count === 1 ? "" : "s"}`;

function relativeTime(iso) {
  if (!iso) return "";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "";
  const seconds = Math.round((Date.now() - then) / 1000);
  if (seconds < 60) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days}d ago`;
  return new Date(then).toLocaleDateString();
}

function dayLabel(iso) {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "Older";
  const today = new Date();
  const yesterday = new Date(Date.now() - 86400000);
  const same = (a, b) => a.toDateString() === b.toDateString();
  if (same(date, today)) return "Today";
  if (same(date, yesterday)) return "Yesterday";
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** Local UI state that must survive a reload. */
const prefs = {
  get(key, fallback) {
    try {
      const raw = localStorage.getItem(`redline:${key}`);
      return raw === null ? fallback : JSON.parse(raw);
    } catch {
      return fallback;
    }
  },
  set(key, value) {
    try {
      localStorage.setItem(`redline:${key}`, JSON.stringify(value));
    } catch { /* private mode */ }
  },
};

/** Sessions this browser has looked at, for the "new" dot. */
const seen = {
  all() { return prefs.get("seen", {}); },
  mark(id, stamp) {
    const map = this.all();
    map[id] = stamp || new Date().toISOString();
    prefs.set("seen", map);
  },
  isNew(session) {
    const map = this.all();
    const latest = session.submitted_at || session.created_at;
    return !map[session.id] || (latest && latest > map[session.id]);
  },
};

async function api(path, options = {}) {
  const response = await fetch(path, options);
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = new Error(body.error || `request failed (${response.status})`);
    error.status = response.status;
    error.body = body;
    throw error;
  }
  return body;
}

const unfiltered = (query) => !query;

/* ------------------------------------------------------------------ rail */

const FILTERS = [
  { id: "all", label: "All", test: () => true },
  { id: "open", label: "Open", test: (s) => s.status === "open" },
  { id: "submitted", label: "Submitted", test: (s) => s.status === "submitted" },
  { id: "mine", label: "Mine", test: (s) => s.agent === "human" },
];

class Rail {
  constructor({ body, chips, search, selectedId, onSelect }) {
    this.body = body;
    this.chips = chips;
    this.search = search;
    this.selectedId = selectedId || null;
    this.onSelect = onSelect;
    this.sessions = [];
    this.filter = prefs.get("filter", "all");
    this.query = "";
    this.wire();
  }

  wire() {
    if (this.search) {
      this.search.value = prefs.get("query", "");
      this.query = this.search.value;
      this.search.addEventListener("input", () => {
        this.query = this.search.value;
        prefs.set("query", this.query);
        this.render();
      });
    }
  }

  async load() {
    const { sessions } = await api("/api/sessions");
    this.sessions = sessions || [];
    this.render();
  }

  setSelected(id) {
    this.selectedId = id;
    if (id) seen.mark(id);
    this.render();
  }

  visible() {
    const active = FILTERS.find((f) => f.id === this.filter) || FILTERS[0];
    const query = this.query.trim().toLowerCase();
    return this.sessions.filter((session) => {
      if (!active.test(session)) return false;
      if (unfiltered(query)) return true;
      return [session.title, session.note, session.id, session.url]
        .filter(Boolean)
        .some((field) => field.toLowerCase().includes(query));
    });
  }

  renderChips() {
    if (!this.chips) return;
    const counts = Object.fromEntries(
      FILTERS.map((filter) => [filter.id, this.sessions.filter(filter.test).length]),
    );
    this.chips.innerHTML = FILTERS.map(
      (filter) => `
        <button class="chip" type="button" data-filter="${filter.id}"
                aria-pressed="${this.filter === filter.id}">
          ${filter.label}<b>${counts[filter.id]}</b>
        </button>`,
    ).join("");
    $$(".chip", this.chips).forEach((chip) =>
      chip.addEventListener("click", () => {
        this.filter = chip.dataset.filter;
        prefs.set("filter", this.filter);
        this.render();
      }),
    );
  }

  render() {
    this.renderChips();
    const sessions = this.visible();
    if (!sessions.length) {
      this.body.innerHTML = `
        <div class="rail-empty">
          ${this.sessions.length ? "Nothing matches that filter." : "No sessions yet."}
        </div>`;
      return;
    }

    const groups = new Map();
    for (const session of sessions) {
      const label = dayLabel(session.created_at);
      if (!groups.has(label)) groups.set(label, []);
      groups.get(label).push(session);
    }

    const showGroups = sessions.length > 12;
    const parts = [];
    for (const [label, items] of groups) {
      if (showGroups) parts.push(`<div class="rail-group">${escapeHtml(label)}</div>`);
      for (const session of items) {
        const annotations = session.annotation_count || 0;
        const fresh = seen.isNew(session);
        const badge = fresh
          ? `<span class="badge new" title="new since you last looked">new</span>`
          : annotations
            ? `<span class="badge" title="${plural(annotations, "annotation")}">${annotations}</span>`
            : "";
        parts.push(`
          <div class="session ${escapeHtml(session.status)}"
               role="button" tabindex="0"
               data-id="${escapeHtml(session.id)}"
               aria-current="${session.id === this.selectedId}">
            <span class="dot"></span>
            <div class="body">
              <div class="name">${escapeHtml(session.title || session.id)}</div>
              <div class="sub">${escapeHtml(relativeTime(session.submitted_at || session.created_at))}${
                session.note ? " · " + escapeHtml(session.note) : ""
              }</div>
            </div>
            ${badge}
          </div>`);
      }
    }
    this.body.innerHTML = parts.join("");

    $$(".session", this.body).forEach((row) => {
      const activate = () => this.onSelect(row.dataset.id, row);
      row.addEventListener("click", activate);
      row.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          activate();
        }
      });
    });
  }
}

/* ------------------------------------------------------------------ editor */

const TOOLS = ["select", "rect", "arrow", "pen", "text"];

class Editor {
  constructor(root) {
    this.root = root;
    this.stage = $("#stage", root);
    this.canvas = $("#canvas", root);
    this.ctx = this.canvas.getContext("2d");
    this.sessionId = root.dataset.sessionId;
    this.meta = null;
    this.image = null;
    this.annotations = [];
    this.history = [];
    this.future = [];
    this.selection = -1;
    this.tool = "select";
    this.color = prefs.get("color", "#e11d48");
    this.lineWidth = prefs.get("width", 3);
    this.view = { scale: 1, x: 0, y: 0, fitted: false };
    this.pointers = new Map();
    this.drag = null;
    this.draw = null;
    this.pan = null;
  }

  async init() {
    this.rail = new Rail({
      body: $("#rail-body"),
      chips: $("#chips"),
      search: $("#search"),
      selectedId: this.sessionId,
      onSelect: (id) => this.openSession(id),
    });
    this.bindChrome();
    this.bindCanvas();
    this.bindKeyboard();
    this.syncToolButtons();
    $("#color").value = this.color;
    $("#width").value = this.lineWidth;
    this.syncWidthLabel();
    await this.rail.load();
    await this.load(this.sessionId);
  }

  async openSession(id) {
    if (!id || id === this.sessionId) return;
    history.pushState({ id }, "", `/s/${encodeURIComponent(id)}?`);
    this.sessionId = id;
    this.root.dataset.sessionId = id;
    this.rail.setSelected(id);
    await this.load(id);
  }

  async load(id) {
    $("#stage").classList.remove("hidden");
    try {
      const data = await api(`/api/sessions/${encodeURIComponent(id)}`);
      this.meta = data.meta;
      this.annotations = (data.annotations && data.annotations.annotations) || [];
      this.history = [];
      this.future = [];
      this.selection = -1;
      this.renderInspector();
      this.renderHeader();
      await this.loadImage(this.meta.image);
      this.fit();
      this.render();
    } catch (error) {
      this.showStageMessage(
        error.status === 401 ? "Not signed in" : "Could not load this session",
        error.status === 401
          ? "Open the session list and sign in with your access token."
          : error.message,
      );
    }
  }

  loadImage(name) {
    return new Promise((resolve) => {
      const image = new Image();
      image.onload = () => {
        this.image = image;
        resolve();
      };
      image.onerror = () => {
        this.image = null;
        this.showStageMessage("Screenshot missing", `Could not load ${name}.`);
        resolve();
      };
      image.src = `/sessions/${encodeURIComponent(this.sessionId)}/image`;
    });
  }

  showStageMessage(title, text) {
    let box = $("#stage-empty");
    if (!box) {
      box = document.createElement("div");
      box.id = "stage-empty";
      box.className = "empty-stage";
      this.stage.appendChild(box);
    }
    box.innerHTML = `<div><h2>${escapeHtml(title)}</h2><p>${escapeHtml(text)}</p></div>`;
  }

  renderHeader() {
    $("#session-title").textContent = this.meta.title || this.meta.id;
    const bits = [this.meta.id, this.meta.image_width && this.meta.image_height
      ? `${this.meta.image_width}×${this.meta.image_height}` : null,
      this.meta.url || null].filter(Boolean);
    $("#session-sub").textContent = bits.join(" · ");
    const status = $("#session-status");
    status.textContent = this.meta.status;
    status.className = `pill ${this.meta.status}`;
    const submitted = this.meta.status !== "open";
    $("#submit").textContent = submitted ? "Submit again" : "Submit review";
    $("#agent-note").textContent = this.meta.note || "(no message — just the annotations)";
    $("#agent-url").textContent = this.meta.url || "";
    document.title = `${this.meta.title || this.meta.id} · redline`;
    const idLabel = $("#session-id-status");
    if (idLabel) idLabel.textContent = this.meta.id;
  }

  bindChrome() {
    $("#submit").addEventListener("click", () => this.submit());
    $("#toggle-rail").addEventListener("click", () => {
      const app = $("#app");
      app.classList.toggle("rail-hidden");
      prefs.set("railHidden", app.classList.contains("rail-hidden"));
      this.resize();
    });
    $("#toggle-inspector").addEventListener("click", () => {
      const app = $("#app");
      app.classList.toggle("inspector-hidden");
      prefs.set("inspectorHidden", app.classList.contains("inspector-hidden"));
      this.resize();
    });
    if (prefs.get("railHidden", false)) $("#app").classList.add("rail-hidden");
    if (prefs.get("inspectorHidden", false)) $("#app").classList.add("inspector-hidden");

    $$(".tool").forEach((button) =>
      button.addEventListener("click", () => this.setTool(button.dataset.tool)),
    );
    $("#color").addEventListener("input", (event) => {
      this.color = event.target.value;
      prefs.set("color", this.color);
      if (this.selection >= 0) {
        this.mutate(() => { this.annotations[this.selection].color = this.color; });
      }
    });
    $("#width").addEventListener("input", (event) => {
      this.lineWidth = Number(event.target.value);
      prefs.set("width", this.lineWidth);
      this.syncWidthLabel();
      if (this.selection >= 0) {
        this.mutate(() => { this.annotations[this.selection].width = this.lineWidth; });
      }
    });

    $("#zoom-in").addEventListener("click", () => this.zoomAt(1.25));
    $("#zoom-out").addEventListener("click", () => this.zoomAt(0.8));
    $("#zoom-fit").addEventListener("click", () => this.fit());
    $("#zoom-100").addEventListener("click", () => this.setZoom(1));
    $("#new-review").addEventListener("click", () => NewReview.open());
    addEventListener("resize", () => this.resize());
    addEventListener("popstate", () => {
      const id = location.pathname.split("/").filter(Boolean).pop();
      if (id && id !== this.sessionId) {
        this.sessionId = id;
        this.rail.setSelected(id);
        this.load(id);
      }
    });
  }

  syncWidthLabel() {
    $("#width-value").textContent = `${this.lineWidth}px`;
  }

  bindKeyboard() {
    const typing = (event) => {
      const tag = event.target.tagName;
      return tag === "INPUT" || tag === "TEXTAREA" || event.target.isContentEditable;
    };
    addEventListener("keydown", (event) => {
      const meta = event.ctrlKey || event.metaKey;
      if (meta && event.key.toLowerCase() === "z") {
        event.preventDefault();
        event.shiftKey ? this.redo() : this.undo();
        return;
      }
      if (meta && event.key === "Enter") {
        event.preventDefault();
        this.submit();
        return;
      }
      if (typing(event)) return;

      const map = { v: "select", r: "rect", a: "arrow", p: "pen", t: "text" };
      if (map[event.key.toLowerCase()] && !meta) {
        this.setTool(map[event.key.toLowerCase()]);
        return;
      }
      switch (event.key) {
        case "Escape":
          this.draw = null;
          this.select(-1);
          this.render();
          break;
        case "Delete":
        case "Backspace":
          if (this.selection >= 0) {
            event.preventDefault();
            this.deleteSelected();
          }
          break;
        case "ArrowUp": case "ArrowDown": case "ArrowLeft": case "ArrowRight": {
          if (this.selection < 0) break;
          event.preventDefault();
          const step = event.shiftKey ? 10 : 1;
          const dx = event.key === "ArrowLeft" ? -step : event.key === "ArrowRight" ? step : 0;
          const dy = event.key === "ArrowUp" ? -step : event.key === "ArrowDown" ? step : 0;
          this.mutate(() => this.translateAnnotation(this.annotations[this.selection], dx, dy));
          break;
        }
        case "+": case "=": this.zoomAt(1.25); break;
        case "-": this.zoomAt(0.8); break;
        case "0": this.fit(); break;
        case "1": this.setZoom(1); break;
        case "[": $("#toggle-rail").click(); break;
        case "]": $("#toggle-inspector").click(); break;
        default: break;
      }
    });
  }

  setTool(tool) {
    if (!TOOLS.includes(tool)) return;
    this.tool = tool;
    this.syncToolButtons();
    this.stage.className = `stage tool-${tool}`;
    if (tool !== "select") this.select(-1);
  }

  syncToolButtons() {
    $$(".tool").forEach((button) =>
      button.setAttribute("aria-pressed", String(button.dataset.tool === this.tool)),
    );
  }

  // ---------------------------------------------------------- canvas geometry

  resize() {
    const dpr = window.devicePixelRatio || 1;
    const rect = this.stage.getBoundingClientRect();
    this.canvas.width = Math.max(1, Math.round(rect.width * dpr));
    this.canvas.height = Math.max(1, Math.round(rect.height * dpr));
    this.dpr = dpr;
    this.render();
  }

  fit() {
    if (!this.image) return;
    const rect = this.stage.getBoundingClientRect();
    const scale = Math.min(rect.width / this.image.naturalWidth, rect.height / this.image.naturalHeight) * 0.94;
    this.view.scale = clamp(scale, 0.05, 8);
    this.view.x = (rect.width - this.image.naturalWidth * this.view.scale) / 2;
    this.view.y = (rect.height - this.image.naturalHeight * this.view.scale) / 2;
    this.view.fitted = true;
    this.updateZoomLabel();
    this.render();
  }

  setZoom(scale, cx, cy) {
    const rect = this.stage.getBoundingClientRect();
    const point = { x: cx ?? rect.width / 2, y: cy ?? rect.height / 2 };
    const before = {
      x: (point.x - this.view.x) / this.view.scale,
      y: (point.y - this.view.y) / this.view.scale,
    };
    this.view.scale = clamp(scale, 0.05, 8);
    this.view.x = point.x - before.x * this.view.scale;
    this.view.y = point.y - before.y * this.view.scale;
    this.updateZoomLabel();
    this.render();
  }

  zoomAt(factor, cx, cy) {
    this.setZoom(this.view.scale * factor, cx, cy);
  }

  updateZoomLabel() {
    const label = `${Math.round(this.view.scale * 100)}%`;
    $("#zoom-label").textContent = label;
    const mirror = $("#zoom-label-status");
    if (mirror) mirror.textContent = label;
  }

  toImage(event) {
    const rect = this.canvas.getBoundingClientRect();
    return {
      x: (event.clientX - rect.left - this.view.x) / this.view.scale,
      y: (event.clientY - rect.top - this.view.y) / this.view.scale,
    };
  }

  // ---------------------------------------------------------- rendering

  render() {
    if (!this.dpr) this.resize();
    const { ctx, canvas } = this;
    const dpr = this.dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, canvas.width / dpr, canvas.height / dpr);
    if (!this.image) return;

    ctx.save();
    ctx.translate(this.view.x, this.view.y);
    ctx.scale(this.view.scale, this.view.scale);
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(this.image, 0, 0);

    this.annotations.forEach((annotation, index) => {
      this.paint(ctx, annotation, index === this.selection);
    });
    if (this.draw) this.paint(ctx, this.draw, false);
    ctx.restore();
  }

  paint(ctx, annotation, selected) {
    const colour = annotation.color || "#e11d48";
    const width = annotation.width || 3;
    ctx.save();
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = colour;
    ctx.fillStyle = colour;
    ctx.lineWidth = width;

    if (annotation.type === "rect") {
      const { start, end } = annotation;
      const x = Math.min(start.x, end.x);
      const y = Math.min(start.y, end.y);
      const w = Math.abs(end.x - start.x);
      const h = Math.abs(end.y - start.y);
      ctx.strokeRect(x, y, w, h);
    } else if (annotation.type === "arrow") {
      const { start, end } = annotation;
      ctx.beginPath();
      ctx.moveTo(start.x, start.y);
      ctx.lineTo(end.x, end.y);
      ctx.stroke();
      const angle = Math.atan2(end.y - start.y, end.x - start.x);
      const head = Math.max(10, width * 3.5);
      ctx.beginPath();
      ctx.moveTo(end.x, end.y);
      ctx.lineTo(end.x - head * Math.cos(angle - Math.PI / 7), end.y - head * Math.sin(angle - Math.PI / 7));
      ctx.lineTo(end.x - head * Math.cos(angle + Math.PI / 7), end.y - head * Math.sin(angle + Math.PI / 7));
      ctx.closePath();
      ctx.fill();
    } else if (annotation.type === "pen") {
      const points = annotation.points || [];
      if (points.length > 1) {
        ctx.beginPath();
        ctx.moveTo(points[0].x, points[0].y);
        for (let i = 1; i < points.length; i += 1) ctx.lineTo(points[i].x, points[i].y);
        ctx.stroke();
      }
    } else if (annotation.type === "text") {
      ctx.font = `600 ${annotation.size || 18}px -apple-system, "Segoe UI", system-ui, sans-serif`;
      ctx.textBaseline = "top";
      const lines = String(annotation.text || "").split("\n");
      lines.forEach((line, index) => {
        const y = annotation.y + index * (annotation.size || 18) * 1.25;
        ctx.lineWidth = 4;
        ctx.strokeStyle = "rgba(13,17,23,.85)";
        ctx.strokeText(line, annotation.x, y);
        ctx.fillStyle = colour;
        ctx.fillText(line, annotation.x, y);
      });
    }

    if (selected) {
      const box = this.bounds(annotation);
      ctx.restore();
      ctx.save();
      ctx.strokeStyle = "#2f81f7";
      ctx.lineWidth = 1;
      ctx.setLineDash([5, 4]);
      ctx.strokeRect(box.x - 6, box.y - 6, box.w + 12, box.h + 12);
      ctx.setLineDash([]);
      ctx.fillStyle = "#2f81f7";
      for (const [hx, hy] of [[box.x, box.y], [box.x + box.w, box.y], [box.x, box.y + box.h], [box.x + box.w, box.y + box.h]]) {
        ctx.fillRect(hx - 3, hy - 3, 6, 6);
      }
    }
    ctx.restore();
  }

  bounds(annotation) {
    if (annotation.type === "rect") {
      const x = Math.min(annotation.start.x, annotation.end.x);
      const y = Math.min(annotation.start.y, annotation.end.y);
      return { x, y, w: Math.abs(annotation.end.x - annotation.start.x), h: Math.abs(annotation.end.y - annotation.start.y) };
    }
    if (annotation.type === "arrow") {
      const x = Math.min(annotation.start.x, annotation.end.x);
      const y = Math.min(annotation.start.y, annotation.end.y);
      return { x, y, w: Math.abs(annotation.end.x - annotation.start.x), h: Math.abs(annotation.end.y - annotation.start.y) };
    }
    if (annotation.type === "pen") {
      const points = annotation.points || [{ x: 0, y: 0 }];
      const xs = points.map((p) => p.x);
      const ys = points.map((p) => p.y);
      return { x: Math.min(...xs), y: Math.min(...ys), w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) };
    }
    const size = annotation.size || 18;
    const width = Math.max(20, String(annotation.text || "").length * size * 0.55);
    return { x: annotation.x, y: annotation.y, w: width, h: size * 1.3 };
  }

  // ---------------------------------------------------------- hit testing

  hitTest(point) {
    const tolerance = 7 / this.view.scale;
    for (let index = this.annotations.length - 1; index >= 0; index -= 1) {
      const annotation = this.annotations[index];
      const box = this.bounds(annotation);
      if (annotation.type === "pen") {
        const points = annotation.points || [];
        for (let i = 1; i < points.length; i += 1) {
          if (distanceToSegment(point, points[i - 1], points[i]) <= tolerance) return index;
        }
      } else if (annotation.type === "arrow") {
        if (distanceToSegment(point, annotation.start, annotation.end) <= tolerance + 4) return index;
      } else if (point.x >= box.x - tolerance && point.x <= box.x + box.w + tolerance
        && point.y >= box.y - tolerance && point.y <= box.y + box.h + tolerance) {
        return index;
      }
    }
    return -1;
  }

  select(index) {
    this.selection = index;
    this.renderInspector();
    this.render();
  }

  // ---------------------------------------------------------- mutations

  snapshot() {
    this.history.push(JSON.stringify(this.annotations));
    if (this.history.length > 100) this.history.shift();
    this.future.length = 0;
  }

  mutate(fn) {
    this.snapshot();
    fn();
    this.renderInspector();
    this.render();
  }

  undo() {
    if (!this.history.length) return;
    this.future.push(JSON.stringify(this.annotations));
    this.annotations = JSON.parse(this.history.pop());
    this.selection = -1;
    this.renderInspector();
    this.render();
  }

  redo() {
    if (!this.future.length) return;
    this.history.push(JSON.stringify(this.annotations));
    this.annotations = JSON.parse(this.future.pop());
    this.selection = -1;
    this.renderInspector();
    this.render();
  }

  translateAnnotation(annotation, dx, dy) {
    const shift = (point) => { point.x += dx; point.y += dy; };
    if (annotation.type === "pen") annotation.points.forEach(shift);
    else if (annotation.type === "text") { annotation.x += dx; annotation.y += dy; }
    else { shift(annotation.start); shift(annotation.end); }
  }

  deleteSelected() {
    if (this.selection < 0) return;
    this.mutate(() => {
      this.annotations.splice(this.selection, 1);
      this.selection = -1;
    });
  }

  // ---------------------------------------------------------- pointer

  bindCanvas() {
    const stage = this.stage;

    stage.addEventListener("wheel", (event) => {
      event.preventDefault();
      const rect = stage.getBoundingClientRect();
      const cx = event.clientX - rect.left;
      const cy = event.clientY - rect.top;
      if (event.ctrlKey || event.metaKey) {
        this.zoomAt(event.deltaY < 0 ? 1.1 : 1 / 1.1, cx, cy);
      } else if (event.shiftKey) {
        this.view.x -= event.deltaY;
        this.render();
      } else {
        this.view.y -= event.deltaY;
        this.render();
      }
    }, { passive: false });

    stage.addEventListener("pointerdown", (event) => {
      if (!this.image) return;
      const point = this.toImage(event);

      if (event.button === 1 || event.button === 2 || (this.tool === "select" && event.spaceKey)) {
        this.pan = { x: event.clientX, y: event.clientY, vx: this.view.x, vy: this.view.y };
        stage.classList.add("panning");
        stage.setPointerCapture(event.pointerId);
        return;
      }

      if (this.tool === "select") {
        const hit = this.hitTest(point);
        this.select(hit);
        if (hit >= 0 && event.button === 0) {
          this.drag = { index: hit, origin: point, snapshot: JSON.stringify(this.annotations) };
          stage.setPointerCapture(event.pointerId);
        } else if (hit < 0 && event.button === 0) {
          this.pan = { x: event.clientX, y: event.clientY, vx: this.view.x, vy: this.view.y };
          stage.classList.add("panning");
          stage.setPointerCapture(event.pointerId);
        }
        return;
      }

      if (this.tool === "text") {
        this.startText(point, event);
        return;
      }

      this.draw = {
        type: this.tool,
        color: this.color,
        width: this.lineWidth,
        start: point,
        end: point,
        points: [point],
      };
      stage.setPointerCapture(event.pointerId);
      this.render();
    });

    stage.addEventListener("pointermove", (event) => {
      const point = this.toImage(event);
      $("#cursor-pos").textContent = `${Math.round(point.x)}, ${Math.round(point.y)}`;

      if (this.pan) {
        this.view.x = this.pan.vx + (event.clientX - this.pan.x);
        this.view.y = this.pan.vy + (event.clientY - this.pan.y);
        this.render();
        return;
      }
      if (this.drag) {
        const dx = point.x - this.drag.origin.x;
        const dy = point.y - this.drag.origin.y;
        const annotation = JSON.parse(this.drag.snapshot)[this.drag.index];
        this.translateAnnotation(annotation, dx, dy);
        this.annotations[this.drag.index] = annotation;
        this.render();
        return;
      }
      if (this.draw) {
        if (this.draw.type === "pen") this.draw.points.push(point);
        this.draw.end = point;
        this.render();
      }
    });

    const finish = (event) => {
      if (this.pan) {
        this.pan = null;
        stage.classList.remove("panning");
      }
      if (this.drag) {
        this.history.push(this.drag.snapshot);
        this.future.length = 0;
        this.drag = null;
        this.renderInspector();
      }
      if (this.draw) {
        const drawing = this.draw;
        this.draw = null;
        const width = Math.abs(drawing.end.x - drawing.start.x);
        const height = Math.abs(drawing.end.y - drawing.start.y);
        if (drawing.type === "pen" && drawing.points.length > 1) {
          this.mutate(() => {
            this.annotations.push({
              type: "pen",
              color: drawing.color,
              width: drawing.width,
              points: drawing.points.map((p) => ({ x: Math.round(p.x), y: Math.round(p.y) })),
            });
          });
        } else if (drawing.type === "rect" && (width > 2 || height > 2)) {
          this.mutate(() => {
            this.annotations.push({
              type: "rect",
              color: drawing.color,
              width: drawing.width,
              start: { x: Math.round(drawing.start.x), y: Math.round(drawing.start.y) },
              end: { x: Math.round(drawing.end.x), y: Math.round(drawing.end.y) },
            });
          });
        } else if (drawing.type === "arrow" && Math.hypot(width, height) > 4) {
          this.mutate(() => {
            this.annotations.push({
              type: "arrow",
              color: drawing.color,
              width: drawing.width,
              start: { x: Math.round(drawing.start.x), y: Math.round(drawing.start.y) },
              end: { x: Math.round(drawing.end.x), y: Math.round(drawing.end.y) },
            });
          });
        }
        this.render();
      }
      if (stage.hasPointerCapture?.(event.pointerId)) stage.releasePointerCapture(event.pointerId);
    };
    stage.addEventListener("pointerup", finish);
    stage.addEventListener("pointercancel", finish);
    stage.addEventListener("contextmenu", (event) => event.preventDefault());

    // space to pan
    addEventListener("keydown", (event) => {
      if (event.code === "Space" && !event.repeat && event.target === document.body) {
        event.preventDefault();
        this.spacePan = true;
      }
    });
    addEventListener("keyup", (event) => {
      if (event.code === "Space") this.spacePan = false;
    });
  }

  startText(point, event) {
    this.closeTextEditor();
    const rect = this.stage.getBoundingClientRect();
    const editor = document.createElement("textarea");
    editor.className = "text-editor";
    editor.rows = 1;
    editor.style.left = `${event.clientX - rect.left}px`;
    editor.style.top = `${event.clientY - rect.top}px`;
    this.stage.appendChild(editor);
    editor.focus();
    this.textEditor = { editor, point };

    const commit = () => {
      const text = editor.value.trim();
      this.closeTextEditor();
      if (text) {
        this.mutate(() => {
          this.annotations.push({
            type: "text",
            color: this.color,
            x: Math.round(point.x),
            y: Math.round(point.y),
            size: 18,
            text,
          });
        });
      }
    };
    editor.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        this.closeTextEditor();
      } else if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        commit();
      }
    });
    editor.addEventListener("blur", commit);
  }

  closeTextEditor() {
    if (this.textEditor) {
      this.textEditor.editor.remove();
      this.textEditor = null;
    }
  }

  // ---------------------------------------------------------- inspector

  renderInspector() {
    const body = $("#ann-list");
    $("#ann-count").textContent = String(this.annotations.length);
    if (!this.annotations.length) {
      body.innerHTML = `<div class="inspector-empty">Nothing drawn yet.<br>Pick a tool above the canvas.</div>`;
      return;
    }
    body.innerHTML = this.annotations.map((annotation, index) => {
      const glyph = { rect: "▭", arrow: "↗", pen: "✎", text: "T" }[annotation.type] || "?";
      const label = annotation.type === "text"
        ? escapeHtml(annotation.text || "")
        : annotation.type === "rect"
          ? `${Math.round(annotation.start.x)}, ${Math.round(annotation.start.y)} → ${Math.round(annotation.end.x)}, ${Math.round(annotation.end.y)}`
          : `${Math.round(annotation.start.x)}, ${Math.round(annotation.start.y)}`;
      return `
        <div class="ann-row" data-index="${index}" aria-current="${index === this.selection}">
          <span class="glyph">${glyph}</span>
          <span class="label"><b>${index + 1}. ${label}</b><span>${annotation.type}</span></span>
          <button class="del" data-delete="${index}" title="Delete">✕</button>
        </div>`;
    }).join("");

    $$(".ann-row", body).forEach((row) => {
      row.addEventListener("click", (event) => {
        if (event.target.dataset.delete !== undefined) return;
        const index = Number(row.dataset.index);
        this.select(index);
        this.revealSelection();
      });
    });
    $$(".del", body).forEach((button) => {
      button.addEventListener("click", (event) => {
        event.stopPropagation();
        const index = Number(button.dataset.delete);
        this.mutate(() => {
          this.annotations.splice(index, 1);
          this.selection = -1;
        });
      });
    });
  }

  revealSelection() {
    if (this.selection < 0 || !this.image) return;
    const box = this.bounds(this.annotations[this.selection]);
    const rect = this.stage.getBoundingClientRect();
    this.view.x = rect.width / 2 - (box.x + box.w / 2) * this.view.scale;
    this.view.y = rect.height / 2 - (box.y + box.h / 2) * this.view.scale;
    this.render();
  }

  // ---------------------------------------------------------- submit

  async submit() {
    if (!this.image) return;
    const button = $("#submit");
    const label = button.textContent;
    button.disabled = true;
    button.textContent = "Submitting…";
    try {
      const exportCanvas = document.createElement("canvas");
      exportCanvas.width = this.image.naturalWidth;
      exportCanvas.height = this.image.naturalHeight;
      const ctx = exportCanvas.getContext("2d");
      ctx.drawImage(this.image, 0, 0);
      for (const annotation of this.annotations) this.paint(ctx, annotation, false);

      const payload = {
        annotations: this.annotations,
        width: this.image.naturalWidth,
        height: this.image.naturalHeight,
        png: exportCanvas.toDataURL("image/png"),
      };
      await api(`/api/sessions/${encodeURIComponent(this.sessionId)}/annotations`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload),
      });
      this.meta.status = "submitted";
      this.meta.submitted_at = new Date().toISOString();
      this.renderHeader();
      this.renderInspector();
      await this.rail.load();
      button.textContent = "Submitted ✓";
      setTimeout(() => { button.textContent = "Submit again"; button.disabled = false; }, 1400);
    } catch (error) {
      button.textContent = label;
      button.disabled = false;
      alert(`Submit failed: ${error.message}`);
    }
  }
}

/* ------------------------------------------------------------------ helpers */

function distanceToSegment(point, a, b) {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const lengthSq = dx * dx + dy * dy;
  if (!lengthSq) return Math.hypot(point.x - a.x, point.y - a.y);
  let t = ((point.x - a.x) * dx + (point.y - a.y) * dy) / lengthSq;
  t = clamp(t, 0, 1);
  return Math.hypot(point.x - (a.x + t * dx), point.y - (a.y + t * dy));
}

/* ------------------------------------------------------------------ new review */

const NewReview = {
  open() {
    const overlay = $("#overlay");
    if (!overlay) { location.href = "/?new=1"; return; }
    overlay.hidden = false;
    const error = $("#new-error");
    if (error) error.textContent = "";
    $("#f-title").focus();
  },
  close() { const overlay = $("#overlay"); if (overlay) overlay.hidden = true; },
  wire() {
    const overlay = $("#overlay");
    if (!overlay) return;
    const drop = $("#drop");
    const file = $("#file");
    let picked = null;
    const take = (candidate) => {
      if (!candidate || !candidate.type.startsWith("image/")) {
        $("#new-error").textContent = "That is not an image.";
        return;
      }
      picked = candidate;
      drop.textContent = `${candidate.name} — ${Math.round(candidate.size / 1024)} KB`;
      const title = $("#f-title");
      if (!title.value) title.value = candidate.name.replace(/\.[^.]+$/, "");
    };
    drop.addEventListener("click", () => file.click());
    file.addEventListener("change", () => take(file.files[0]));
    ["dragover", "dragleave", "drop"].forEach((type) =>
      drop.addEventListener(type, (event) => {
        event.preventDefault();
        drop.classList.toggle("over", type === "dragover");
        if (type === "drop") take(event.dataTransfer.files[0]);
      }),
    );
    addEventListener("dragover", (event) => event.preventDefault());
    addEventListener("drop", (event) => {
      if (!event.dataTransfer.files.length) return;
      event.preventDefault();
      if (overlay.hidden) NewReview.open();
      take(event.dataTransfer.files[0]);
    });
    $("#new-cancel").addEventListener("click", () => NewReview.close());
    $("#new-form").addEventListener("submit", async (event) => {
      event.preventDefault();
      if (!picked) { $("#new-error").textContent = "Choose a screenshot first."; return; }
      const submit = $("#new-submit");
      submit.disabled = true;
      $("#new-error").textContent = "Uploading…";
      try {
        const dataUrl = await new Promise((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(reader.result);
          reader.onerror = () => reject(reader.error);
          reader.readAsDataURL(picked);
        });
        const body = await api("/api/sessions", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            title: $("#f-title").value.trim() || picked.name,
            url: $("#f-url").value.trim(),
            note: $("#f-note").value.trim(),
            image_base64: dataUrl,
            agent: "human",
          }),
        });
        location.href = `/s/${encodeURIComponent(body.meta.id)}`;
      } catch (error) {
        $("#new-error").textContent = error.message;
        submit.disabled = false;
      }
    });
    if (new URLSearchParams(location.search).has("new")) NewReview.open();
  },
};

/* ------------------------------------------------------------------ list page */

async function bootList() {
  const rail = new Rail({
    body: $("#rail-body"),
    chips: $("#chips"),
    search: $("#search"),
    onSelect: (id) => { location.href = `/s/${encodeURIComponent(id)}`; },
  });
  await rail.load();
  NewReview.wire();
  $("#new-review").addEventListener("click", () => NewReview.open());
  const count = $("#sessions-count");
  if (count) count.textContent = plural(rail.sessions.length, "session");

  try {
    const config = await api("/api/config");
    const json = JSON.stringify(config.mcp_config, null, 2);
    const block = $("#mcp-json");
    if (block) block.textContent = json;
    const copy = $("#copy-mcp");
    if (copy) {
      copy.addEventListener("click", async () => {
        try {
          await navigator.clipboard.writeText(json);
          $("#copy-status").textContent = "copied";
          setTimeout(() => { $("#copy-status").textContent = ""; }, 3000);
        } catch {
          $("#copy-status").textContent = "select the text above and copy";
        }
      });
    }
  } catch { /* not signed in yet */ }
}

/* ------------------------------------------------------------------ boot */

document.addEventListener("DOMContentLoaded", () => {
  const page = document.body.dataset.page;
  if (page === "editor") {
    const id = decodeURIComponent(location.pathname.split("/").filter(Boolean).pop() || "");
    $("#app").dataset.sessionId = id;
    const editor = new Editor($("#app"));
    window.redline = editor;
    editor.init();
  } else {
    bootList();
  }
});
