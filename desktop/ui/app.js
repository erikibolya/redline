/* redline UI. No framework, no build step: the whole client is this file. */
"use strict";

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

const escapeHtml = (value) =>
  String(value ?? "").replace(/[&<>"']/g, (ch) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[ch]));

const clamp = (value, low, high) => Math.min(high, Math.max(low, value));

/** Stable identity per annotation: lets history diffing tell "moved" from "deleted + added". */
const uid = () =>
  (globalThis.crypto?.randomUUID?.() || `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`);

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

/* ------------------------------------------------------------------ theme */

const Theme = {
  current() {
    return prefs.get("theme", null)
      || (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");
  },
  apply(theme) {
    document.documentElement.dataset.theme = theme;
    prefs.set("theme", theme);
    $$("[data-theme-toggle]").forEach((button) => {
      button.textContent = theme === "light" ? "☾" : "☀";
      button.title = theme === "light" ? "Dark theme" : "Light theme";
    });
  },
  toggle() {
    this.apply(this.current() === "light" ? "dark" : "light");
  },
};

/* ------------------------------------------------------------------ rail */

// "Active" is the default so finished sessions drop out of the way; "Done" is the archive.
const FILTERS = [
  { id: "active", label: "Active", test: (s) => s.status !== "resolved" },
  { id: "open", label: "Open", test: (s) => s.status === "open" },
  { id: "submitted", label: "Submitted", test: (s) => s.status === "submitted" },
  { id: "resolved", label: "Done", test: (s) => s.status === "resolved" },
  { id: "mine", label: "Mine", test: (s) => s.agent === "human" },
];

class Rail {
  constructor({ body, chips, search, purge, selectedId, onSelect, onAction, onPurge }) {
    this.body = body;
    this.chips = chips;
    this.search = search;
    this.purge = purge;
    this.selectedId = selectedId || null;
    this.onSelect = onSelect;
    this.onAction = onAction;
    this.onPurge = onPurge;
    this.sessions = [];
    this.filter = prefs.get("filter", "active");
    if (this.filter === "all") this.filter = "active"; // pre-v0.6 preference
    this.query = "";
    this.wire();
  }

  wire() {
    if (this.purge) {
      this.purge.addEventListener("click", () => {
        const ids = this.sessions.filter((session) => session.status === "resolved").map((s) => s.id);
        if (!ids.length) return;
        if (confirm(`Delete ${ids.length} finished session(s)? Their files go too.`)) {
          this.onPurge?.(ids);
        }
      });
    }
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
    if (this.purge) {
      const done = this.sessions.filter((session) => session.status === "resolved").length;
      this.purge.hidden = done === 0;
      this.purge.textContent = `Delete done (${done})`;
    }
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
        const done = session.status === "resolved";
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
            <span class="row-actions">
              <button class="act" data-act="${done ? "reopen" : "resolve"}"
                      title="${done ? "Reopen" : "Mark done"}">${done ? "↺" : "✓"}</button>
              <button class="act" data-act="delete" title="Delete">✕</button>
            </span>
            ${badge}
          </div>`);
      }
    }
    this.body.innerHTML = parts.join("");

    $$(".session .act", this.body).forEach((button) => {
      button.addEventListener("click", (event) => {
        event.stopPropagation();
        this.onAction?.(button.dataset.act, button.closest(".session").dataset.id);
      });
    });
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

const TOOLS = ["select", "rect", "ellipse", "arrow", "pen"];

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
    this.selected = new Set();
    this.marquee = null;
    this.tool = "select";
    this.color = prefs.get("color", "#e11d48");
    this.textSize = prefs.get("textSize", 18);
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
      purge: $("#purge-done"),
      selectedId: this.sessionId,
      onSelect: (id) => this.openSession(id),
      onAction: (action, id) => this.railAction(action, id),
      onPurge: (ids) => this.purgeSessions(ids),
    });
    this.bindChrome();
    this.bindCanvas();
    this.bindKeyboard();
    this.syncToolButtons();
    $("#color").value = this.color;
    const sizeSelect = $("#text-size");
    if (sizeSelect) sizeSelect.value = String(this.textSize);
    $("#width").value = this.lineWidth;
    this.syncWidthLabel();
    await this.rail.load();
    await this.load(this.sessionId);
  }

  /** Mark done / reopen / delete, straight from the rail. */
  async railAction(action, id) {
    try {
      if (action === "resolve") {
        await api(`/api/sessions/${encodeURIComponent(id)}/resolve`, { method: "POST" });
      } else if (action === "reopen") {
        await api(`/api/sessions/${encodeURIComponent(id)}/reopen`, { method: "POST" });
      } else if (action === "delete") {
        const title = this.rail.sessions.find((s) => s.id === id)?.title || id;
        if (!confirm(`Delete “${title}”? The screenshot and annotations are removed.`)) return;
        await api(`/api/sessions/${encodeURIComponent(id)}`, { method: "DELETE" });
        if (id === this.sessionId) {
          location.href = "/";
          return;
        }
      }
      await this.rail.load();
    } catch (error) {
      alert(`Could not ${action}: ${error.message}`);
    }
  }

  async purgeSessions(ids) {
    try {
      for (const id of ids) {
        await api(`/api/sessions/${encodeURIComponent(id)}`, { method: "DELETE" });
      }
      if (ids.includes(this.sessionId)) {
        location.href = "/";
        return;
      }
      await this.rail.load();
    } catch (error) {
      alert(`Could not delete: ${error.message}`);
    }
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
      this.selected.clear();
      this.renderInspector();
      this.renderHeader();
      this.startThreadPolling();
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
    document.title = `${this.meta.title || this.meta.id} · RedLine`;
    const idLabel = $("#session-id-status");
    if (idLabel) idLabel.textContent = this.meta.id;
  }

  bindChrome() {
    const noteField = $("#note-text");
    if (noteField) {
      noteField.addEventListener("focus", () => {
        this.noteSnapshot = JSON.stringify(this.annotations);
      });
      noteField.addEventListener("input", () => {
        const index = this.primary();
        if (index < 0) return;
        this.annotations[index].note = noteField.value;
        this.renderInspector();
        this.render();
      });
      noteField.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          noteField.blur();
        }
      });
      noteField.addEventListener("blur", () => {
        if (this.noteSnapshot && this.noteSnapshot !== JSON.stringify(this.annotations)) {
          this.history.push(this.noteSnapshot);
          this.future.length = 0;
        }
        this.noteSnapshot = null;
      });
    }
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
    const agentBlock = $("#agent-block");
    if (agentBlock) {
      agentBlock.addEventListener("toggle", () => {
        if (agentBlock.open) this.loadAgentView();
      });
      const historyBlock = $("#history-block");
      if (historyBlock) {
        historyBlock.addEventListener("toggle", () => {
          if (historyBlock.open) this.loadHistory();
        });
      }
      const viewBlock = $("#agent-view-block");
      if (viewBlock) {
        viewBlock.addEventListener("toggle", () => {
          if (viewBlock.open) this.loadAgentView();
        });
      }
    }

    if (prefs.get("railHidden", false)) $("#app").classList.add("rail-hidden");
    if (prefs.get("inspectorHidden", false)) $("#app").classList.add("inspector-hidden");

    $$(".tool").forEach((button) =>
      button.addEventListener("click", () => this.setTool(button.dataset.tool)),
    );
    const sizeSelect = $("#text-size");
    if (sizeSelect) {
      sizeSelect.addEventListener("change", (event) => {
        this.textSize = Number(event.target.value);
        prefs.set("textSize", this.textSize);
        const indices = [...this.selected].filter((i) => this.annotations[i].type === "text");
        if (indices.length) {
          this.mutate(() => {
            for (const index of indices) this.annotations[index].size = this.textSize;
          });
        }
      });
    }

    $$("[data-layer]").forEach((button) =>
      button.addEventListener("click", () => this.reorderSelected(button.dataset.layer)));
    $$("[data-group]").forEach((button) =>
      button.addEventListener("click", () =>
        button.dataset.group === "group" ? this.groupSelected() : this.ungroupSelected()));

    $("#color").addEventListener("input", (event) => {
      this.color = event.target.value;
      prefs.set("color", this.color);
      if (this.selected.size) {
        this.mutate(() => {
          for (const index of this.selected) this.annotations[index].color = this.color;
        });
      }
    });
    $("#width").addEventListener("input", (event) => {
      this.lineWidth = Number(event.target.value);
      prefs.set("width", this.lineWidth);
      this.syncWidthLabel();
      if (this.selected.size) {
        this.mutate(() => {
          for (const index of this.selected) this.annotations[index].width = this.lineWidth;
        });
      }
    });

    $$("[data-align]").forEach((button) =>
      button.addEventListener("click", () => this.alignSelected(button.dataset.align)));
    $$("[data-distribute]").forEach((button) =>
      button.addEventListener("click", () => this.distributeSelected(button.dataset.distribute)));

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

      const map = { v: "select", r: "rect", e: "ellipse", a: "arrow", p: "pen" };
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
        case "Enter": {
          const index = this.primary();
          const target = this.annotations[index];
          if (target && target.type === "text") {
            event.preventDefault();
            this.startText({ x: target.x, y: target.y }, event, index);
          }
          break;
        }
        case "?": {
          event.preventDefault();
          Help.toggle();
          break;
        }
        case "Delete":
        case "Backspace":
          if (this.selected.size) {
            event.preventDefault();
            this.deleteSelected();
          }
          break;
        case "ArrowUp": case "ArrowDown": case "ArrowLeft": case "ArrowRight": {
          if (!this.selected.size) break;
          event.preventDefault();
          const step = event.shiftKey ? 10 : 1;
          const dx = event.key === "ArrowLeft" ? -step : event.key === "ArrowRight" ? step : 0;
          const dy = event.key === "ArrowUp" ? -step : event.key === "ArrowDown" ? step : 0;
          const indices = [...this.selected];
          this.mutate(() => {
            for (const index of indices) this.translateAnnotation(this.annotations[index], dx, dy);
          });
          break;
        }
        case "g":
          event.shiftKey ? this.ungroupSelected() : this.groupSelected();
          break;
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
    if (this.view.fitted && this.image) {
      this.fit();
      return;
    }
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
      this.paint(ctx, annotation, this.selected.has(index));
    });
    this.annotations.forEach((annotation, index) => {
      if (annotation.type !== "text") this.paintBadge(ctx, annotation, index + 1, 12 / this.view.scale, 2 / this.view.scale);
    });
    if (this.draw) this.paint(ctx, this.draw, false);
    ctx.restore();

    if (this.marquee) {
      const toScreen = (point) => ({
        x: point.x * this.view.scale + this.view.x,
        y: point.y * this.view.scale + this.view.y,
      });
      const a = toScreen(this.marquee.start);
      const b = toScreen(this.marquee.end);
      ctx.save();
      ctx.fillStyle = "rgba(47,129,247,.12)";
      ctx.strokeStyle = "#2f81f7";
      ctx.lineWidth = 1;
      ctx.fillRect(Math.min(a.x, b.x), Math.min(a.y, b.y), Math.abs(b.x - a.x), Math.abs(b.y - a.y));
      ctx.strokeRect(Math.min(a.x, b.x), Math.min(a.y, b.y), Math.abs(b.x - a.x), Math.abs(b.y - a.y));
      ctx.restore();
    }
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
    } else if (annotation.type === "ellipse") {
      const { start, end } = annotation;
      ctx.beginPath();
      ctx.ellipse(
        (start.x + end.x) / 2,
        (start.y + end.y) / 2,
        Math.abs(end.x - start.x) / 2,
        Math.abs(end.y - start.y) / 2,
        0, 0, Math.PI * 2,
      );
      ctx.stroke();
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

  /// The number the human sees on the canvas, in the panel and on `annotated.png`.
  paintBadge(ctx, annotation, number, radius, inset) {
    const box = this.bounds(annotation);
    // Sit on the corner/start point instead of inside the shape: the badge must not cover the
    // very thing the human marked.
    const anchor = annotation.start
      || (annotation.points && annotation.points[0])
      || { x: box.x, y: box.y };
    const cx = anchor.x + inset;
    const cy = anchor.y + inset;
    ctx.save();
    ctx.beginPath();
    ctx.arc(cx, cy, radius, 0, Math.PI * 2);
    ctx.fillStyle = annotation.color || "#e11d48";
    ctx.fill();
    ctx.lineWidth = Math.max(1, radius / 8);
    ctx.strokeStyle = "rgba(13,17,23,.55)";
    ctx.stroke();
    ctx.fillStyle = "#fff";
    ctx.font = `700 ${radius * 1.15}px -apple-system, "Segoe UI", system-ui, sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(String(number), cx, cy);
    ctx.restore();
  }

  bounds(annotation) {
    if (annotation.type === "rect" || annotation.type === "ellipse") {
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

  select(index, additive = false) {
    if (index < 0) {
      this.selected.clear();
    } else if (additive) {
      if (this.selected.has(index)) this.selected.delete(index);
      else this.selected.add(index);
    } else {
      this.selected = new Set([index]);
    }
    this.renderInspector();
    this.render();
  }

  primary() {
    return this.selected.size ? Math.min(...this.selected) : -1;
  }

  clearSelection() {
    this.selected.clear();
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
    this.selected.clear();
    this.renderInspector();
    this.render();
  }

  redo() {
    if (!this.future.length) return;
    this.history.push(JSON.stringify(this.annotations));
    this.annotations = JSON.parse(this.future.pop());
    this.selected.clear();
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
    if (!this.selected.size) return;
    const indices = [...this.selected].sort((a, b) => b - a);
    this.mutate(() => {
      for (const index of indices) this.annotations.splice(index, 1);
      this.selected.clear();
    });
  }

  /** Clicking one member of a group selects the whole group. */
  expandGroup(index) {
    const group = this.annotations[index]?.group;
    if (!group) return [index];
    return this.annotations
      .map((annotation, i) => (annotation.group === group ? i : -1))
      .filter((i) => i >= 0);
  }

  selectAt(index, additive) {
    const group = this.expandGroup(index);
    if (additive) {
      const allSelected = group.every((i) => this.selected.has(i));
      for (const i of group) {
        if (allSelected) this.selected.delete(i);
        else this.selected.add(i);
      }
      this.renderInspector();
      this.render();
      return;
    }
    this.selected = new Set(group);
    this.renderInspector();
    this.render();
  }

  /** Z-order: the array order is the paint order. */
  reorderSelected(mode) {
    if (!this.selected.size) return;
    const objects = [...this.selected].map((i) => this.annotations[i]);
    const isSelected = (annotation) => objects.includes(annotation);
    this.mutate(() => {
      let list = this.annotations.slice();
      if (mode === "front") {
        list = [...list.filter((a) => !isSelected(a)), ...list.filter(isSelected)];
      } else if (mode === "back") {
        list = [...list.filter(isSelected), ...list.filter((a) => !isSelected(a))];
      } else if (mode === "forward") {
        for (let i = list.length - 2; i >= 0; i -= 1) {
          if (isSelected(list[i]) && !isSelected(list[i + 1])) {
            [list[i], list[i + 1]] = [list[i + 1], list[i]];
          }
        }
      } else if (mode === "backward") {
        for (let i = 1; i < list.length; i += 1) {
          if (isSelected(list[i]) && !isSelected(list[i - 1])) {
            [list[i], list[i - 1]] = [list[i - 1], list[i]];
          }
        }
      }
      this.annotations = list;
      this.selected = new Set(
        list.map((annotation, index) => (isSelected(annotation) ? index : -1)).filter((i) => i >= 0),
      );
    });
  }

  groupSelected() {
    if (this.selected.size < 2) return;
    const id = uid();
    this.mutate(() => {
      for (const index of this.selected) this.annotations[index].group = id;
    });
  }

  ungroupSelected() {
    if (![...this.selected].some((i) => this.annotations[i].group)) return;
    this.mutate(() => {
      for (const index of this.selected) delete this.annotations[index].group;
    });
  }

  /** Align the selection's bounding boxes; needs two or more annotations. */
  alignSelected(kind) {
    const indices = [...this.selected];
    if (indices.length < 2) return;
    const boxes = indices.map((index) => ({ index, box: this.bounds(this.annotations[index]) }));
    const left = Math.min(...boxes.map((b) => b.box.x));
    const right = Math.max(...boxes.map((b) => b.box.x + b.box.w));
    const top = Math.min(...boxes.map((b) => b.box.y));
    const bottom = Math.max(...boxes.map((b) => b.box.y + b.box.h));
    this.mutate(() => {
      for (const { index, box } of boxes) {
        let dx = 0;
        let dy = 0;
        if (kind === "left") dx = left - box.x;
        if (kind === "right") dx = right - (box.x + box.w);
        if (kind === "hcenter") dx = (left + right) / 2 - (box.x + box.w / 2);
        if (kind === "top") dy = top - box.y;
        if (kind === "bottom") dy = bottom - (box.y + box.h);
        if (kind === "vcenter") dy = (top + bottom) / 2 - (box.y + box.h / 2);
        if (dx || dy) this.translateAnnotation(this.annotations[index], Math.round(dx), Math.round(dy));
      }
    });
  }

  /** Even gaps between three or more annotations, on one axis. */
  distributeSelected(axis) {
    const indices = [...this.selected];
    if (indices.length < 3) return;
    const items = indices
      .map((index) => ({ index, box: this.bounds(this.annotations[index]) }))
      .sort((a, b) => (axis === "h" ? a.box.x - b.box.x : a.box.y - b.box.y));
    const first = items[0].box;
    const last = items[items.length - 1].box;
    const span = axis === "h" ? last.x + last.w - first.x : last.y + last.h - first.y;
    const occupied = items.reduce((sum, item) => sum + (axis === "h" ? item.box.w : item.box.h), 0);
    const gap = (span - occupied) / (items.length - 1);
    this.mutate(() => {
      let cursor = axis === "h" ? first.x : first.y;
      for (const { index, box } of items) {
        const current = axis === "h" ? box.x : box.y;
        const delta = Math.round(cursor - current);
        if (delta) {
          this.translateAnnotation(this.annotations[index], axis === "h" ? delta : 0, axis === "h" ? 0 : delta);
        }
        cursor += (axis === "h" ? box.w : box.h) + gap;
      }
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

    const pinch = { start: null, scale: 1 };

    stage.addEventListener("pointerdown", (event) => {
      if (!this.image) return;
      // The options bar and the inline text editor live inside the stage. Capturing the pointer
      // for marquee/pan on those would swallow their click events.
      if (event.target.closest(".options, .text-editor")) return;

      this.pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
      if (this.pointers.size === 2) {
        const [a, b] = Array.from(this.pointers.values());
        pinch.start = {
          distance: Math.hypot(a.x - b.x, a.y - b.y),
          scale: this.view.scale,
          cx: (a.x + b.x) / 2,
          cy: (a.y + b.y) / 2,
        };
        this.draw = null;
        this.drag = null;
        this.pan = null;
        return;
      }
      const point = this.toImage(event);

      if (event.button === 1 || event.button === 2 || this.spacePan) {
        this.pan = { x: event.clientX, y: event.clientY, vx: this.view.x, vy: this.view.y };
        stage.classList.add("panning");
        stage.setPointerCapture(event.pointerId);
        return;
      }

      if (this.tool === "select") {
        const hit = this.hitTest(point);
        const additive = event.shiftKey || event.ctrlKey || event.metaKey;
        if (hit >= 0) {
          if (additive) this.selectAt(hit, true);
          else if (!this.selected.has(hit)) this.selectAt(hit, false);
          if (this.selected.has(hit) && event.button === 0) {
            this.drag = {
              origin: point,
              snapshot: JSON.stringify(this.annotations),
              indices: [...this.selected],
            };
            stage.setPointerCapture(event.pointerId);
          }
        } else if (!additive && event.button === 0) {
          // Empty space: rubber-band select (pan lives on space/middle/right drag).
          this.selected.clear();
          this.marquee = { start: point, end: point };
          stage.setPointerCapture(event.pointerId);
        } else {
          this.select(-1);
        }
        this.renderInspector();
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
      if (this.pointers.has(event.pointerId)) {
        this.pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
      }
      if (pinch.start && this.pointers.size === 2) {
        const [a, b] = Array.from(this.pointers.values());
        const distance = Math.hypot(a.x - b.x, a.y - b.y);
        const rect = stage.getBoundingClientRect();
        this.setZoom(
          pinch.start.scale * (distance / Math.max(1, pinch.start.distance)),
          pinch.start.cx - rect.left,
          pinch.start.cy - rect.top,
        );
        return;
      }
      const point = this.toImage(event);
      $("#cursor-pos").textContent = `${Math.round(point.x)}, ${Math.round(point.y)}`;

      if (this.pan) {
        this.view.x = this.pan.vx + (event.clientX - this.pan.x);
        this.view.y = this.pan.vy + (event.clientY - this.pan.y);
        this.render();
        return;
      }
      if (this.marquee) {
        this.marquee.end = point;
        this.render();
        return;
      }
      if (this.drag) {
        const dx = point.x - this.drag.origin.x;
        const dy = point.y - this.drag.origin.y;
        const original = JSON.parse(this.drag.snapshot);
        for (const index of this.drag.indices) {
          const annotation = original[index];
          this.translateAnnotation(annotation, dx, dy);
          this.annotations[index] = annotation;
        }
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
      this.pointers.delete(event.pointerId);
      if (this.pointers.size < 2) pinch.start = null;
      if (this.pan) {
        this.pan = null;
        stage.classList.remove("panning");
      }
      if (this.marquee) {
        const start = this.marquee.start;
        const end = this.marquee.end;
        const box = {
          x: Math.min(start.x, end.x),
          y: Math.min(start.y, end.y),
          w: Math.abs(end.x - start.x),
          h: Math.abs(end.y - start.y),
        };
        this.selected.clear();
        this.annotations.forEach((annotation, index) => {
          const b = this.bounds(annotation);
          if (b.x <= box.x + box.w && b.x + b.w >= box.x && b.y <= box.y + box.h && b.y + b.h >= box.y) {
            this.selected.add(index);
          }
        });
        this.marquee = null;
        this.renderInspector();
        this.render();
      }
      if (this.drag) {
        this.history.push(this.drag.snapshot);
        this.future.length = 0;
        this.drag = null;
        this.renderInspector();
      }
      if (this.draw) {
        const drawing = this.draw;
        const before = this.annotations.length;
        this.draw = null;
        const width = Math.abs(drawing.end.x - drawing.start.x);
        const height = Math.abs(drawing.end.y - drawing.start.y);
        if (drawing.type === "pen" && drawing.points.length > 1) {
          this.mutate(() => {
            this.annotations.push({
              id: uid(),
              type: "pen",
              color: drawing.color,
              width: drawing.width,
              points: drawing.points.map((p) => ({ x: Math.round(p.x), y: Math.round(p.y) })),
            });
          });
        } else if (drawing.type === "rect" && (width > 2 || height > 2)) {
          this.mutate(() => {
            this.annotations.push({
              id: uid(),
              type: "rect",
              color: drawing.color,
              width: drawing.width,
              start: { x: Math.round(drawing.start.x), y: Math.round(drawing.start.y) },
              end: { x: Math.round(drawing.end.x), y: Math.round(drawing.end.y) },
            });
          });
        } else if (drawing.type === "ellipse" && (width > 2 || height > 2)) {
          this.mutate(() => {
            this.annotations.push({
              id: uid(),
              type: "ellipse",
              color: drawing.color,
              width: drawing.width,
              start: { x: Math.round(drawing.start.x), y: Math.round(drawing.start.y) },
              end: { x: Math.round(drawing.end.x), y: Math.round(drawing.end.y) },
            });
          });
        } else if (drawing.type === "arrow" && Math.hypot(width, height) > 4) {
          this.mutate(() => {
            this.annotations.push({
              id: uid(),
              type: "arrow",
              color: drawing.color,
              width: drawing.width,
              start: { x: Math.round(drawing.start.x), y: Math.round(drawing.start.y) },
              end: { x: Math.round(drawing.end.x), y: Math.round(drawing.end.y) },
            });
          });
        }
        this.render();
        if (this.annotations.length > before) {
          this.select(this.annotations.length - 1);
          requestAnimationFrame(() => $("#note-text")?.focus());
        }
      }
      if (stage.hasPointerCapture?.(event.pointerId)) stage.releasePointerCapture(event.pointerId);
    };
    stage.addEventListener("pointerup", finish);
    stage.addEventListener("pointercancel", finish);

    stage.addEventListener("dblclick", (event) => {
      if (this.tool !== "select") return;
      const hit = this.hitTest(this.toImage(event));
      if (hit >= 0 && this.annotations[hit].type === "text") {
        this.select(hit);
        this.startText({ x: this.annotations[hit].x, y: this.annotations[hit].y }, event, hit);
      }
    });
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

  startText(point, event, existingIndex = -1) {
    this.closeTextEditor();
    const rect = this.stage.getBoundingClientRect();
    const existing = existingIndex >= 0 ? this.annotations[existingIndex] : null;
    const anchor = existing ? { x: existing.x, y: existing.y } : point;

    // Place the editor over the text, in screen space, whatever the zoom.
    const screenX = anchor.x * this.view.scale + this.view.x;
    const screenY = anchor.y * this.view.scale + this.view.y;

    const editor = document.createElement("textarea");
    editor.className = "text-editor";
    editor.rows = 1;
    editor.style.left = `${Math.max(6, screenX)}px`;
    editor.style.top = `${Math.max(6, screenY)}px`;
    editor.style.color = existing ? existing.color : this.color;
    editor.value = existing ? existing.text : "";
    this.stage.appendChild(editor);
    this.textEditor = { editor, point };
    // Focus now — pointerdown already called preventDefault, so the browser will not move focus
    // to the body afterwards. Deferring this lost the first keystroke for fast typists.
    editor.focus();
    editor.select();

    // Enter commits and removes the editor, which fires blur — commit must be idempotent or
    // every note is stored twice.
    let closed = false;
    const commit = () => {
      if (closed) return;
      closed = true;
      const text = editor.value.trim();
      this.closeTextEditor();
      if (existing) {
        if (!text) {
          // Clearing the note removes the annotation — that is what people expect.
          const index = this.annotations.indexOf(existing);
          this.mutate(() => {
            this.annotations.splice(index, 1);
            this.selection = -1;
          });
        } else if (text !== existing.text) {
          this.mutate(() => { existing.text = text; });
        }
        return;
      }
      if (text) {
        this.mutate(() => {
          this.annotations.push({
            id: uid(),
            type: "text",
            color: this.color,
            x: Math.round(point.x),
            y: Math.round(point.y),
            size: this.textSize,
            text,
          });
        });
      }
    };
    editor.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        closed = true;
        this.closeTextEditor();
      } else if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        commit();
      }
    });
    editor.addEventListener("blur", commit);
  }

  closeTextEditor() {
    const current = this.textEditor;
    if (!current) return;
    // Clear the reference first: removing a focused node fires `blur`, whose handler calls
    // commit() again — which must find nothing to do.
    this.textEditor = null;
    current.editor.remove();
  }

  // ---------------------------------------------------------- inspector

  renderInspector() {
    const body = $("#ann-list");
    const count = this.annotations.length;
    $("#ann-count").textContent = this.selected.size > 1
      ? `${this.selected.size}/${count}`
      : String(count);
    const multi = this.selected.size >= 2;
    const tools = $("#selection-tools");
    if (tools) tools.classList.toggle("hidden", this.selected.size === 0);
    $$(".align-only").forEach((element) => element.classList.toggle("hidden", !multi));
    $$("[data-distribute]").forEach((element) =>
      element.classList.toggle("disabled", this.selected.size < 3));
    $$('[data-group="group"]').forEach((element) =>
      element.classList.toggle("disabled", !multi));
    $$('[data-group="ungroup"]').forEach((element) =>
      element.classList.toggle("disabled", ![...this.selected].some((i) => this.annotations[i].group)));
    const single = this.selected.size === 1 ? [...this.selected][0] : -1;
    const target = single >= 0 ? this.annotations[single] : null;
    const editor = $("#note-editor");
    if (editor) {
      const editable = Boolean(target) && target.type !== "text";
      editor.classList.toggle("hidden", !editable);
      if (editable) {
        const number = $("#note-num");
        number.textContent = String(single + 1);
        number.style.background = target.color || "#e11d48";
        $("#note-kind").textContent = target.type;
        const field = $("#note-text");
        if (document.activeElement !== field) field.value = target.note || "";
      }
    }
    if (!this.annotations.length) {
      body.innerHTML = `<div class="inspector-empty">Nothing drawn yet.<br>Pick a tool above the canvas.</div>`;
      return;
    }
    body.innerHTML = this.annotations.map((annotation, index) => {
      const label = annotation.type === "text"
        ? escapeHtml(annotation.text || "")
        : annotation.type === "rect" || annotation.type === "ellipse"
          ? `${Math.round(annotation.start.x)}, ${Math.round(annotation.start.y)} → ${Math.round(annotation.end.x)}, ${Math.round(annotation.end.y)}`
          : `${Math.round(annotation.start.x)}, ${Math.round(annotation.start.y)}`;
      const note = String(annotation.note || "").trim();
      const meta = note ? escapeHtml(note) : annotation.type;
      return `
        <div class="ann-row" data-index="${index}" aria-current="${this.selected.has(index)}">
          <span class="ann-num" style="background:${escapeHtml(annotation.color || "#e11d48")}">${index + 1}</span>
          <span class="label"><b>${label}</b><span>${meta}${annotation.group ? " · group" : ""}</span></span>
          <button class="del" data-delete="${index}" title="Delete">✕</button>
        </div>`;
    }).join("");

    $$(".ann-row", body).forEach((row) => {
      row.addEventListener("click", (event) => {
        if (event.target.dataset.delete !== undefined) return;
        const index = Number(row.dataset.index);
        const additive = event.shiftKey || event.ctrlKey || event.metaKey;
        this.selectAt(index, additive);
        if (!additive) this.revealSelection();
      });
    });
    $$(".del", body).forEach((button) => {
      button.addEventListener("click", (event) => {
        event.stopPropagation();
        const index = Number(button.dataset.delete);
        this.mutate(() => {
          this.annotations.splice(index, 1);
          this.selected.clear();
        });
      });
    });
  }

  revealSelection() {
    const index = this.primary();
    if (index < 0 || !this.image) return;
    const box = this.bounds(this.annotations[index]);
    const rect = this.stage.getBoundingClientRect();
    this.view.x = rect.width / 2 - (box.x + box.w / 2) * this.view.scale;
    this.view.y = rect.height / 2 - (box.y + box.h / 2) * this.view.scale;
    this.render();
  }

  // ---------------------------------------------------------- history

  async loadHistory() {
    const list = $("#history-list");
    if (!list) return;
    list.textContent = "Loading…";
    try {
      const data = await api(`/api/sessions/${encodeURIComponent(this.sessionId)}/history`);
      const rounds = data.submissions || [];
      const counter = $("#history-count");
      if (counter) counter.textContent = String(rounds.length);
      if (!rounds.length) {
        list.innerHTML = `<div class="inspector-empty">No earlier submissions.</div>`;
        return;
      }
      list.innerHTML = rounds.map((round) => `
        <div class="ann-row" style="cursor:default">
          <span class="glyph">↺</span>
          <span class="label">
            <b>${escapeHtml(round.stamp.replace("T", " ").replace(/-([0-9]{2})-([0-9]{2})$/, ":$1:$2"))}</b>
            <span>${plural(round.annotations, "annotation")}${round.png ? " · image" : ""}</span>
          </span>
          <button class="del" style="opacity:1" data-diff="${escapeHtml(round.stamp)}"
                  title="What changed since this round">Δ</button>
          ${round.png ? `<a class="del" style="opacity:1;text-decoration:none" target="_blank"
              href="/sessions/${encodeURIComponent(this.sessionId)}/history/${encodeURIComponent(round.stamp)}.png"
              title="Open that round's screenshot">↗</a>` : ""}
        </div>
        <pre class="pre hidden" id="diff-${escapeHtml(round.stamp)}"></pre>`).join("");

      $$("[data-diff]", list).forEach((button) =>
        button.addEventListener("click", () => this.loadDiff(button.dataset.diff)));
    } catch (error) {
      list.textContent = `Could not load history: ${error.message}`;
    }
  }

  async loadDiff(stamp) {
    const block = document.getElementById(`diff-${stamp}`);
    if (!block) return;
    if (!block.classList.contains("hidden")) {
      block.classList.add("hidden");
      return;
    }
    block.classList.remove("hidden");
    block.textContent = "Comparing…";
    try {
      const data = await api(
        `/api/sessions/${encodeURIComponent(this.sessionId)}/history/${encodeURIComponent(stamp)}/diff`,
      );
      const lines = [];
      for (const change of data.added) lines.push(`+ ${change.summary}`);
      for (const change of data.removed) lines.push(`- ${change.summary}`);
      for (const change of data.changed) lines.push(`~ ${change.summary} (edited)`);
      block.textContent = lines.length
        ? `${data.from.annotations} → ${data.to.annotations} annotations\n\n${lines.join("\n")}`
        : "No changes since this round.";
    } catch (error) {
      block.textContent = `Could not compare: ${error.message}`;
    }
  }

  // ---------------------------------------------------------- agent thread

  /// The agent answers with the MCP `reply` tool; without a poll the human would have to reload
  /// the page to notice, which is exactly the "I submitted and heard nothing" dead end.
  startThreadPolling() {
    clearInterval(this.threadTimer);
    this.loadThread();
    this.threadTimer = setInterval(() => this.loadThread({ quiet: true }), 5000);
  }

  async loadThread({ quiet = false } = {}) {
    const box = $("#agent-thread");
    if (!box) return;
    if (!quiet) box.textContent = "Loading…";
    try {
      const data = await api(`/api/sessions/${encodeURIComponent(this.sessionId)}/thread`);
      const messages = Array.isArray(data.messages) ? data.messages : [];
      const counter = $("#thread-count");
      if (counter) counter.textContent = String(messages.length);
      if (!messages.length) {
        box.innerHTML = `<div class="inspector-empty">No replies yet.</div>`;
        return;
      }
      box.innerHTML = messages.map((entry) => {
        const when = String(entry.at || "").replace("T", " ").slice(0, 16);
        return `<div class="thread-row">
          <div class="thread-at">${escapeHtml(when)}</div>
          <div class="thread-text">${escapeHtml(entry.message || "")}</div>
        </div>`;
      }).join("");
    } catch (error) {
      if (!quiet) box.textContent = `Could not load replies: ${error.message}`;
    }
  }

  // ---------------------------------------------------------- agent view

  async loadAgentView() {
    const block = $("#agent-view");
    if (!block) return;
    block.textContent = "Loading…";
    try {
      const response = await fetch(`/sessions/${encodeURIComponent(this.sessionId)}/review.md`);
      if (response.status === 404) {
        block.textContent = "Not submitted yet — the agent has nothing to read.";
        return;
      }
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      block.textContent = await response.text();
    } catch (error) {
      block.textContent = `Could not load: ${error.message}`;
    }
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
      this.annotations.forEach((annotation, index) => {
        if (annotation.type !== "text") this.paintBadge(ctx, annotation, index + 1, 14, 2);
      });

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
      if ($("#agent-block")?.open) this.loadAgentView();
      if ($("#history-block")?.open) this.loadHistory();
      this.loadThread({ quiet: true });
      // Hand the keyboard back to the canvas: the review is sent, the next key is a shortcut.
      document.activeElement?.blur?.();
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

/* ------------------------------------------------------------------ help */

const Help = {
  toggle(force) {
    const overlay = $("#help");
    if (!overlay) return;
    overlay.hidden = force === undefined ? !overlay.hidden : !force;
  },
  wire() {
    const overlay = $("#help");
    if (!overlay) return;
    $$("[data-help-close]", overlay).forEach((button) =>
      button.addEventListener("click", () => Help.toggle(false)));
    overlay.addEventListener("click", (event) => {
      if (event.target === overlay) Help.toggle(false);
    });
    $$("[data-help-open]").forEach((button) =>
      button.addEventListener("click", () => Help.toggle(true)));
  },
};

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
      const captureUrl = ($("#f-capture")?.value || "").trim();
      if (!picked && !captureUrl) {
        $("#new-error").textContent = "Choose a screenshot, or give a URL to capture.";
        return;
      }
      const submit = $("#new-submit");
      submit.disabled = true;
      if (captureUrl) {
        $("#new-error").textContent = "Capturing… (a few seconds)";
        try {
          const body = await api("/api/capture", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              url: captureUrl,
              title: $("#f-title").value.trim() || captureUrl,
              note: $("#f-note").value.trim(),
              viewport: "1440x900",
              agent: "human",
            }),
          });
          location.href = `/s/${encodeURIComponent(body.meta.id)}`;
        } catch (error) {
          $("#new-error").textContent = error.message;
          submit.disabled = false;
        }
        return;
      }
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
    purge: $("#purge-done"),
    onSelect: (id) => { location.href = `/s/${encodeURIComponent(id)}`; },
    onAction: async (action, id) => {
      try {
        if (action === "resolve") {
          await api(`/api/sessions/${encodeURIComponent(id)}/resolve`, { method: "POST" });
        } else if (action === "reopen") {
          await api(`/api/sessions/${encodeURIComponent(id)}/reopen`, { method: "POST" });
        } else if (action === "delete") {
          if (!confirm("Delete this session? The screenshot and annotations are removed.")) return;
          await api(`/api/sessions/${encodeURIComponent(id)}`, { method: "DELETE" });
        }
        await rail.load();
      } catch (error) {
        alert(`Could not ${action}: ${error.message}`);
      }
    },
    onPurge: async (ids) => {
      for (const id of ids) {
        await api(`/api/sessions/${encodeURIComponent(id)}`, { method: "DELETE" });
      }
      await rail.load();
    },
  });
  await rail.load();
  NewReview.wire();
  $("#new-review").addEventListener("click", () => NewReview.open());
  const count = $("#sessions-count");
  if (count) count.textContent = plural(rail.sessions.length, "session");

  try {
    const config = await api("/api/config");
    const json = JSON.stringify(config.mcp_config, null, 2);
    // This page is a screenshot target (redline can capture itself), so the token is masked on
    // screen. "Copy config" still copies the real JSON.
    const masked = json.replace(/(Bearer\s+)[A-Za-z0-9._~+/=-]{6,}/g, "$1••••••••••••");
    const block = $("#mcp-json");
    if (block) block.textContent = masked;
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
  Theme.apply(Theme.current());
  $$("[data-theme-toggle]").forEach((button) =>
    button.addEventListener("click", () => Theme.toggle()));
  Help.wire();
  addEventListener("keydown", (event) => {
    if (event.key === "?" && document.body.dataset.page !== "editor") {
      const tag = event.target.tagName;
      if (tag !== "INPUT" && tag !== "TEXTAREA") {
        event.preventDefault();
        Help.toggle(true);
      }
    }
    if (event.key === "Escape") {
      const help = $("#help");
      if (help && !help.hidden) { help.hidden = true; return; }
      const overlay = $("#overlay");
      if (overlay && !overlay.hidden) overlay.hidden = true;
    }
  });

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
