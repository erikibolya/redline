/* redline UI. No framework, no build step: the whole client is this file. */
"use strict";

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

const escapeHtml = (value) =>
  String(value ?? "").replace(/[&<>"']/g, (ch) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[ch]));

const clamp = (value, low, high) => Math.min(high, Math.max(low, value));

/**
 * Drag a divider with the pointer (or the arrow keys when it has focus). `size` reads the
 * current dimension, `apply` writes the new one, `min`/`max` are re-read on every move so the
 * limits can follow the window. `sign` is -1 for edges that grow when dragged left or up.
 */
function bindSplitter(handle, { axis = "x", sign = 1, min, max, size, apply, step = 8 }) {
  if (!handle) return;
  const position = (event) => (axis === "x" ? event.clientX : event.clientY);
  let origin = 0;
  let from = 0;
  let dragging = false;

  const clampTo = (raw) => Math.round(clamp(raw, min(), max()));
  const move = (px) => apply(clampTo(px));

  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    dragging = true;
    from = position(event);
    origin = size();
    handle.setPointerCapture(event.pointerId);
    handle.classList.add("dragging");
    document.body.classList.add(axis === "x" ? "resizing-x" : "resizing-y");
  });
  handle.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    move(origin + sign * (position(event) - from));
  });
  const finish = (event) => {
    if (!dragging) return;
    dragging = false;
    handle.classList.remove("dragging");
    document.body.classList.remove("resizing-x", "resizing-y");
    if (handle.hasPointerCapture(event.pointerId)) handle.releasePointerCapture(event.pointerId);
  };
  handle.addEventListener("pointerup", finish);
  handle.addEventListener("pointercancel", finish);

  const keys = axis === "x"
    ? { ArrowLeft: -1, ArrowRight: 1 }
    : { ArrowUp: -1, ArrowDown: 1 };
  handle.addEventListener("keydown", (event) => {
    const direction = keys[event.key];
    if (!direction) return;
    // Keep the editor's arrow-key nudge from firing as well.
    event.preventDefault();
    event.stopPropagation();
    move(size() + sign * direction * (event.shiftKey ? step * 5 : step));
  });
}

/** Stable identity per annotation: lets history diffing tell "moved" from "deleted + added". */
const uid = () =>
  (globalThis.crypto?.randomUUID?.() || `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`);

const plural = (count, word) => `${count} ${word}${count === 1 ? "" : "s"}`;

/** An agent is working only while the round is submitted. A late narration line used to leave
 *  `agent_state=working` on a review that had already been handed back, so the spinner ran
 *  forever and the human could not tell a live agent from a dead one. */
const agentWorking = (meta) => meta?.status === "submitted" && meta.agent_state === "working";


/** "edit, shell and run" → "Edit, Shell and Run", the way a person lists tools. */
function listNames(names) {
  const titled = names.map((name) => name.charAt(0).toUpperCase() + name.slice(1));
  if (titled.length <= 1) return titled.join("");
  return `${titled.slice(0, -1).join(", ")} and ${titled[titled.length - 1]}`;
}

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

/** Wall-clock time in the viewer's timezone: the API sends UTC, people read their own clock. */
function clockTime(at) {
  if (!at) return "";
  const date = at instanceof Date ? at : new Date(at);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
}

/// MediaRecorder and some streamed WebMs report `Infinity` as duration until the browser seeks
/// past the buffered end. Asking for a huge time makes Chromium compute the real value.
function resolvedDuration(video) {
  if (Number.isFinite(video.duration) && video.duration > 0) {
    return Promise.resolve(video.duration);
  }
  return new Promise((resolve) => {
    let settled = false;
    const finish = () => {
      if (settled) return;
      settled = true;
      video.removeEventListener("durationchange", finish);
      resolve(Number.isFinite(video.duration) ? video.duration : 0);
    };
    video.addEventListener("durationchange", finish);
    try {
      video.currentTime = 1e101;
    } catch {
      finish();
    }
    setTimeout(finish, 1500);
  });
}

/// `mm:ss.mmm` (or `hh:mm:ss.mmm`) for video timecodes.
function timecode(seconds) {
  const total = Math.max(0, Math.round((Number(seconds) || 0) * 1000));
  const ms = total % 1000;
  const totalSeconds = Math.floor(total / 1000);
  const s = totalSeconds % 60;
  const m = Math.floor(totalSeconds / 60) % 60;
  const h = Math.floor(totalSeconds / 3600);
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  const mmm = String(ms).padStart(3, "0");
  return h > 0 ? `${String(h).padStart(2, "0")}:${mm}:${ss}.${mmm}` : `${mm}:${ss}.${mmm}`;
}

/** History stamps are the UTC submission time with every ':' and '+' flattened to '-' by the store. */
function stampTime(stamp) {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2})-(\d{2})-(\d{2})/.exec(String(stamp ?? ""));
  if (!match) return String(stamp ?? "");
  const date = new Date(Date.UTC(
    Number(match[1]), Number(match[2]) - 1, Number(match[3]),
    Number(match[4]), Number(match[5]), Number(match[6]),
  ));
  return `${date.toLocaleDateString()} ${clockTime(date)}`;
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

/** The project the human last worked in, so a new review starts there instead of at the placeholder. */
const lastProject = {
  get() { return prefs.get("project", ""); },
  remember(slug) {
    const value = (slug || "").trim();
    if (value && value !== "__new__") prefs.set("project", value);
  },
};

/**
 * The session rail's divider, shared by the dashboard and the editor. The width lives in
 * `redline:railWidth` and in the `--rail` CSS variable, so moving between `/` and `/s/<id>`
 * never jumps. `max` is a callback because the editor must leave room for the inspector.
 */
function bindRailResize({ max, onResize = () => {} } = {}) {
  const app = $("#app");
  const handle = $("#rail-resizer");
  if (!app || !handle) return null;
  const width = () => parseFloat(app.style.getPropertyValue("--rail")) || 272;
  const limit = max || (() => Math.max(200, Math.min(560, innerWidth - 360)));
  const set = (px, persist = true) => {
    app.style.setProperty("--rail", `${px}px`);
    if (persist) prefs.set("railWidth", px);
    onResize();
  };
  // Restoring (and clamping) is not a resize: keep the preferred size for a wider window.
  set(Math.min(prefs.get("railWidth", 272), limit()), false);
  bindSplitter(handle, { min: () => 200, max: limit, size: width, apply: set });
  addEventListener("resize", () => set(Math.min(width(), limit()), false));
  return { width, max: limit, set };
}

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

/* ---------------------------------------------------------- feature confirmation */

/**
 * A native `confirm()` cannot explain what "the feature is done" means to an agent, and it
 * looks different on every platform. This is a small `<dialog>` built on first use, shared by
 * the editor and the sessions page. It returns the chosen action value, or null when dismissed.
 */
function askFeature({ title, body, actions }) {
  return new Promise((resolve) => {
    let dialog = $("#feature-dialog");
    if (!dialog) {
      document.body.insertAdjacentHTML("beforeend", `
        <dialog class="feature-dialog" id="feature-dialog"
                aria-labelledby="feature-dialog-title" aria-describedby="feature-dialog-body">
          <form method="dialog" class="feature-card dialog-frame">
            <div class="dialog-head">
              <div><h2 id="feature-dialog-title"></h2></div>
              <button type="submit" value="keep" class="btn icon dialog-close" aria-label="Close">
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>
              </button>
            </div>
            <div class="dialog-body stack">
              <p id="feature-dialog-body"></p>
            </div>
            <div class="dialog-foot feature-actions" id="feature-actions"></div>
          </form>
        </dialog>`);
      dialog = $("#feature-dialog");
    }
    // A second action while the dialog is open is a double click; the first decision wins.
    if (dialog.open) {
      resolve(null);
      return;
    }
    $("#feature-dialog-title").textContent = title;
    $("#feature-dialog-body").textContent = body;
    const container = $("#feature-actions");
    container.innerHTML = "";
    for (const action of actions) {
      const button = document.createElement("button");
      button.type = "submit";
      button.value = action.value;
      button.dataset.action = action.value;
      button.className = `btn${action.kind ? ` ${action.kind}` : ""}`;
      button.textContent = action.label;
      container.appendChild(button);
    }
    dialog.returnValue = "keep";
    dialog.onclose = () => {
      const choice = dialog.returnValue;
      resolve(choice && choice !== "keep" ? choice : null);
    };
    dialog.showModal();
  });
}

/**
 * Ask for the feature branch a review runs on. Returns the new name, or `null` when the human
 * cancelled, left it unchanged, or the round is submitted (the agent already has a worktree for
 * the old name — renaming now would describe work happening somewhere else).
 */
function askBranch(current, submitted = false) {
  return new Promise((resolve) => {
    let dialog = $("#branch-dialog");
    if (!dialog) {
      document.body.insertAdjacentHTML("beforeend", `
        <dialog class="feature-dialog" id="branch-dialog" aria-labelledby="branch-dialog-title">
          <form method="dialog" class="feature-card dialog-frame">
            <div class="dialog-head">
              <div><h2 id="branch-dialog-title">Feature branch</h2></div>
              <button type="submit" value="keep" class="btn icon dialog-close" aria-label="Close">
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>
              </button>
            </div>
            <div class="dialog-body stack">
              <p id="branch-dialog-body"></p>
              <input id="branch-input" type="text" spellcheck="false" autocomplete="off"
                     aria-label="Feature branch" />
            </div>
            <div class="dialog-foot feature-actions" id="branch-actions"></div>
          </form>
        </dialog>`);
      dialog = $("#branch-dialog");
    }
    if (dialog.open) {
      resolve(null);
      return;
    }
    const input = $("#branch-input");
    $("#branch-dialog-body").textContent = submitted
      ? "This review is with the agent, and its branch is already checked out. Stop it first, then give the work its feature branch."
      : "The agent checks this branch out, works in it and pushes it. It must be a feature branch, not the session id.";
    input.value = current || "";
    input.hidden = submitted;
    const actions = $("#branch-actions");
    actions.innerHTML = "";
    const buttons = submitted
      ? [{ value: "keep", label: "Close" }]
      : [
          { value: "keep", label: "Cancel" },
          { value: "save", label: "Save branch", kind: "primary" },
        ];
    for (const action of buttons) {
      const button = document.createElement("button");
      button.type = "submit";
      button.value = action.value;
      button.className = `btn${action.kind ? ` ${action.kind}` : ""}`;
      button.textContent = action.label;
      actions.appendChild(button);
    }
    dialog.returnValue = "keep";
    dialog.onclose = () => {
      const value = input.value.trim();
      resolve(dialog.returnValue === "save" && value && value !== current ? value : null);
    };
    dialog.showModal();
    if (!submitted) {
      input.focus();
      input.select();
    }
  });
}

/**
 * Confirm, cancel, reopen or delete a feature. Returns the server response, `{deleted: true}`,
 * or `null` when the human dismissed the dialog.
 */
async function featureAction(action, id, title, state = "pending") {
  const path = `/api/sessions/${encodeURIComponent(id)}`;
  if (action === "confirm") {
    const choice = await askFeature({
      title: "Confirm this feature?",
      body: `“${title}” will be marked as confirmed and closed. Your agent may now open a pull request and merge it into the integration branch.`,
      actions: [
        { value: "keep", label: "Keep working" },
        { value: "confirm", label: "Confirm feature", kind: "primary" },
      ],
    });
    if (choice !== "confirm") return null;
    return api(`${path}/confirm-feature`, { method: "POST" });
  }
  if (action === "menu") {
    const decided = state !== "pending";
    const choice = await askFeature({
      title: "What should happen to this feature?",
      body: decided
        ? `“${title}” is ${state}. Reopen returns it to the queue; Delete files removes the screenshot and annotations permanently.`
        : `“${title}” has no decision yet. Cancel feature closes it without merging and keeps the screenshot and annotations; Delete files removes them permanently.`,
      actions: decided
        ? [
            { value: "keep", label: "Keep working" },
            { value: "reopen", label: "Reopen" },
            { value: "delete", label: "Delete files…", kind: "danger" },
          ]
        : [
            { value: "keep", label: "Keep working" },
            { value: "cancel-feature", label: "Cancel feature" },
            { value: "delete", label: "Delete files…", kind: "danger" },
          ],
    });
    if (choice === "reopen") return api(`${path}/reopen`, { method: "POST" });
    if (choice === "cancel-feature") return api(`${path}/cancel-feature`, { method: "POST" });
    if (choice === "delete") {
      await api(path, { method: "DELETE" });
      return { deleted: true };
    }
    return null;
  }
  if (action === "reopen") {
    return api(`${path}/reopen`, { method: "POST" });
  }
  return null;
}

const featureState = (session) => session.feature_state
  || (session.feature_confirmed_at ? "confirmed"
    : session.feature_cancelled_at ? "cancelled" : "pending");

/* ---------------------------------------------------------------- agent settings */

/**
 * Global and per-project run limits. The server is authoritative; this is only the editor for
 * `/api/agent/settings`. A branch always has one owner; waiting for the human frees a run slot.
 */
const AgentSettings = {
  dialog: null,

  ensure() {
    if (this.dialog) return this.dialog;
    document.body.insertAdjacentHTML("beforeend", `
      <dialog class="settings-dialog" id="agent-settings-dialog"
              aria-labelledby="agent-settings-title">
        <form class="settings-form dialog-frame" id="agent-settings-form">
          <div class="dialog-head settings-head">
            <div>
              <h2 id="agent-settings-title">Agent settings</h2>
              <p>How many reviews may run at the same time. One feature branch always has one owner;
                waiting for the human does not take a run slot.</p>
            </div>
            <button type="button" class="btn icon dialog-close" id="agent-settings-close" aria-label="Close">
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>
            </button>
          </div>
          <div class="dialog-body settings-body">
            <div class="settings-row">
              <label for="agent-max-parallel">Max concurrent runs</label>
              <input type="number" id="agent-max-parallel" min="1" max="32" required />
            </div>
            <div class="settings-row">
              <label for="agent-project-max-parallel">Default per-project limit</label>
              <input type="number" id="agent-project-max-parallel" min="1" max="32" required />
            </div>
            <div class="settings-row">
              <label for="agent-lease-seconds">Claim lease (seconds)</label>
              <input type="number" id="agent-lease-seconds" min="30" max="3600" required />
            </div>
            <fieldset class="settings-projects">
              <legend>Per project</legend>
              <div id="agent-project-list"></div>
            </fieldset>
            <div class="settings-status" id="agent-settings-status"></div>
            <p class="settings-error" id="agent-settings-error" hidden></p>
          </div>
          <div class="dialog-foot settings-actions">
            <button type="button" class="btn" id="agent-settings-cancel">Cancel</button>
            <button type="submit" class="btn primary" id="agent-settings-save">Save</button>
          </div>
        </form>
      </dialog>`);
    this.dialog = $("#agent-settings-dialog");
    $("#agent-settings-close").addEventListener("click", () => this.dialog.close());
    $("#agent-settings-cancel").addEventListener("click", () => this.dialog.close());
    $("#agent-settings-form").addEventListener("submit", (event) => {
      event.preventDefault();
      this.save();
    });
    return this.dialog;
  },

  async open() {
    const dialog = this.ensure();
    if (dialog.open) return;
    const error = $("#agent-settings-error");
    error.hidden = true;
    try {
      const data = await api("/api/agent/settings");
      $("#agent-max-parallel").value = String(data.max_parallel);
      $("#agent-project-max-parallel").value = String(data.project_max_parallel);
      $("#agent-lease-seconds").value = String(data.lease_seconds);
      const projects = data.projects || [];
      $("#agent-project-list").innerHTML = projects.length
        ? projects.map((project) => `
          <label class="settings-project">
            <span class="name">${escapeHtml(project.name)}
              <span class="repo">${escapeHtml(project.repo || "")}</span></span>
            <input type="number" min="1" max="32" data-project="${escapeHtml(project.slug)}"
                   value="${project.max_parallel ?? ""}"
                   placeholder="Default (${project.effective_max_parallel})" />
          </label>`).join("")
        : `<div class="settings-empty">No repository-backed projects yet.</div>`;
      const claims = data.claims || [];
      $("#agent-settings-status").innerHTML =
        `<b>${data.running}/${data.max_parallel}</b> running · ${data.waiting} waiting`
        + (claims.length
          ? claims.map((claim) => `
            <div class="settings-claim" title="${escapeHtml(claim.session_id)}">
              <span>${escapeHtml(claim.session_id)}</span>
              <span>${escapeHtml(claim.work_key)}</span>
              <span>${escapeHtml(claim.state)} · ${escapeHtml(claim.agent_id)}</span>
            </div>`).join("")
          : `<div class="settings-empty">No active claims.</div>`);
    } catch (requestError) {
      error.textContent = requestError.message;
      error.hidden = false;
    }
    dialog.showModal();
    $("#agent-max-parallel").focus();
  },

  async save() {
    const error = $("#agent-settings-error");
    error.hidden = true;
    const projects = {};
    $$("#agent-project-list input[data-project]").forEach((input) => {
      projects[input.dataset.project] = input.value.trim() === "" ? null : Number(input.value);
    });
    const body = {
      max_parallel: Number($("#agent-max-parallel").value),
      project_max_parallel: Number($("#agent-project-max-parallel").value),
      lease_seconds: Number($("#agent-lease-seconds").value),
      projects,
    };
    const save = $("#agent-settings-save");
    save.disabled = true;
    try {
      await api("/api/agent/settings", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
      this.dialog.close();
    } catch (requestError) {
      error.textContent = requestError.message;
      error.hidden = false;
    } finally {
      save.disabled = false;
    }
  },
};

const unfiltered = (query) => !query;

/* ---------------------------------------------------------------- agent presence */

/**
 * "Is anybody listening?" One server endpoint feeds the header chip and the rail warning. The
 * status is deliberately non-blocking: reviews can always be created and submitted, they just
 * wait until a bridge connects.
 */
const AgentStatus = {
  timer: null,
  data: null,

  start() {
    this.refresh();
    clearInterval(this.timer);
    this.timer = setInterval(() => this.refresh(), 10000);
  },

  async refresh() {
    try {
      this.render(await api("/api/agent/status"));
    } catch {
      this.render(null);
    }
  },

  render(data) {
    this.data = data;
    const agents = data?.agents || [];
    const online = data ? (data.online || 0) : 0;
    const label = !data
      ? "Agent status unavailable"
      : online
        ? `${online} agent${online === 1 ? "" : "s"} connected`
        : "No agent connected";
    const title = !data
      ? "Could not read the agent status from this server"
      : online
        ? `Connected: ${agents.filter((agent) => agent.online).map((agent) => agent.agent_id).join(", ")}`
        : "No agent is polling this server — submitted reviews will wait";
    $$("#agent-status").forEach((chip) => {
      chip.classList.toggle("online", Boolean(data && online));
      chip.classList.toggle("offline", Boolean(data && !online));
      chip.classList.toggle("unknown", !data);
      chip.title = title;
      chip.setAttribute("aria-label", title);
      const text = chip.querySelector(".agent-label");
      if (text) text.textContent = label;
    });
    $$("#agent-warning").forEach((warning) => {
      warning.hidden = !data || online > 0;
    });
    // The dashboard's invitation to connect is only for a server with no bridge; the dialog can
    // stay open while the bridge appears, so refresh its status too.
    $$("#agent-cta").forEach((callout) => {
      callout.hidden = !data || online > 0;
    });
    if (ConnectAgent.dialog?.open) ConnectAgent.renderStatus();
  },
};

/* ---------------------------------------------------------------- connect agent */

/**
 * The MCP config used to live permanently on the dashboard, even after the agent was already
 * connected. It is now a dialog: the header chip opens it anywhere, and the dashboard only shows
 * an invitation while no bridge is online.
 */
const ConnectAgent = {
  dialog: null,
  config: null,
  error: null,

  ensure() {
    if (this.dialog) return this.dialog;
    document.body.insertAdjacentHTML("beforeend", `
      <dialog class="feature-dialog connect-dialog" id="connect-dialog"
              aria-labelledby="connect-title">
        <div class="feature-card dialog-frame">
          <div class="dialog-head">
            <div>
              <h2 id="connect-title">Connect an agent</h2>
              <p class="connect-status" id="connect-status">Checking…</p>
            </div>
            <button type="button" class="btn icon dialog-close" id="connect-close" aria-label="Close">
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>
            </button>
          </div>
          <div class="dialog-body stack">
            <p class="hint">Paste this into your agent's MCP settings. The token is masked here —
              <b>Copy config</b> puts the real one on your clipboard.</p>
            <pre class="pre" id="connect-json">Loading…</pre>
          </div>
          <div class="dialog-foot">
            <span class="copy-status" id="connect-copy-status"></span>
            <button type="button" class="btn accent" id="connect-copy">Copy config</button>
          </div>
        </div>
      </dialog>`);
    this.dialog = $("#connect-dialog");
    $("#connect-close").addEventListener("click", () => this.dialog.close());
    $("#connect-copy").addEventListener("click", () => this.copy());
    return this.dialog;
  },

  async open() {
    const dialog = this.ensure();
    if (dialog.open) return;
    this.renderStatus();
    dialog.showModal();
    await this.load();
  },

  async load() {
    if (this.config) {
      this.renderConfig();
      return;
    }
    try {
      const config = await api("/api/config");
      this.config = JSON.stringify(config.mcp_config, null, 2);
      this.error = null;
    } catch (requestError) {
      this.error = requestError.message;
    }
    this.renderConfig();
  },

  renderConfig() {
    const block = $("#connect-json");
    if (!block) return;
    if (!this.config) {
      block.textContent = this.error ? `Could not load the config: ${this.error}` : "Could not load the config.";
      return;
    }
    // The token is real in the clipboard, masked on screen: this dialog is a screenshot target too.
    block.textContent = this.config.replace(
      /(Bearer\s+)[A-Za-z0-9._~+/=-]{6,}/g,
      "$1••••••••••••",
    );
  },

  renderStatus() {
    const status = $("#connect-status");
    if (!status) return;
    const data = AgentStatus.data;
    const online = data?.online || 0;
    status.classList.toggle("online", online > 0);
    status.classList.toggle("offline", Boolean(data && !online));
    status.textContent = !data
      ? "Checking the agent status…"
      : online
        ? `${online} agent${online === 1 ? "" : "s"} connected — this config is already in use.`
        : "No agent is connected yet. Paste this config into your agent to start.";
  },

  async copy() {
    const note = $("#connect-copy-status");
    if (!this.config) await this.load();
    if (!this.config) {
      if (note) note.textContent = "config unavailable";
      return;
    }
    try {
      await navigator.clipboard.writeText(this.config);
      if (note) {
        note.textContent = "copied";
        setTimeout(() => { note.textContent = ""; }, 3000);
      }
    } catch {
      if (note) note.textContent = "select the text above and copy";
    }
  },
};

/* ------------------------------------------------------------------ theme */

/// The toggle shows the theme it switches *to*, as an inline SVG: a glyph swap is not a promise
/// that the sun means "light now" on someone else's emoji font.
const THEME_ICONS = {
  light: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z"/></svg>',
  dark: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.2 5.2l1.4 1.4M17.4 17.4l1.4 1.4M18.8 5.2l-1.4 1.4M6.6 17.4l-1.4 1.4"/></svg>',
};

const Theme = {
  current() {
    return prefs.get("theme", null)
      || (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");
  },
  apply(theme) {
    document.documentElement.dataset.theme = theme;
    prefs.set("theme", theme);
    $$("[data-theme-toggle]").forEach((button) => {
      // Dark theme offers the sun, light theme offers the moon.
      button.innerHTML = THEME_ICONS[theme === "light" ? "light" : "dark"];
      button.title = theme === "light" ? "Switch to dark theme" : "Switch to light theme";
      button.setAttribute("aria-label", button.title);
    });
  },
  toggle() {
    this.apply(this.current() === "light" ? "dark" : "light");
  },
};

/* ------------------------------------------------------------------ rail */

/// Which sessions an agent is running right now; the rail polls for this and redraws only when
/// the set changes, so a live indicator does not rebuild the list every few seconds.
const workingIds = (sessions) => (sessions || [])
  .filter(agentWorking)
  .map((session) => session.id)
  .sort()
  .join("\n");

// "Active" is the default so finished sessions drop out of the way; "All" is the archive view
// and "Done" is the resolved-only view.
const FILTERS = [
  { id: "all", label: "All", test: () => true },
  { id: "active", label: "Active", test: (s) => s.status !== "resolved" },
  { id: "open", label: "Open", test: (s) => s.status === "open" },
  { id: "submitted", label: "Submitted", test: (s) => s.status === "submitted" },
  { id: "resolved", label: "Done", test: (s) => s.status === "resolved" },
];

class Rail {
  constructor({ body, chips, search, purge, selectedId, onSelect, onAction, onPurge, onBulk, onViewChange }) {
    this.body = body;
    this.chips = chips;
    this.search = search;
    this.purge = purge;
    this.selectedId = selectedId || null;
    this.onSelect = onSelect;
    this.onAction = onAction;
    this.onPurge = onPurge;
    this.onBulk = onBulk;
    this.onViewChange = onViewChange;
    this.bulkMode = false;
    this.selection = new Set();
    this.sessions = [];
    this.projects = [];
    this.collapsed = new Set(prefs.get("collapsedProjects", []));
    this.filter = prefs.get("filter", "active");
    // Drop preferences for removed filters (the old "mine") instead of rendering an unpressed
    // chip row with no matching view.
    if (!FILTERS.some((filter) => filter.id === this.filter)) {
      this.filter = "active";
      prefs.set("filter", this.filter);
    }
    this.query = "";
    this.agentTimer = null;
    this.wire();
  }

  wire() {
    if (this.purge) {
      this.purge.addEventListener("click", () => {
        const ids = this.sessions
          .filter((session) => featureState(session) === "cancelled")
          .map((session) => session.id);
        if (!ids.length) return;
        if (confirm(`Delete ${ids.length} cancelled session(s)? Their files go too.`)) {
          this.onPurge?.(ids);
        }
      });
    }
    $("#bulk-mode")?.addEventListener("click", () => this.toggleBulk());
    $("#bulk-close")?.addEventListener("click", () => this.exitBulk());
    $("#bulk-confirm")?.addEventListener("click", () => this.applyBulk("confirm"));
    $("#bulk-delete")?.addEventListener("click", () => this.applyBulk("delete"));
    addEventListener("keydown", (event) => {
      if (event.key === "Escape" && this.bulkMode) this.exitBulk();
    });
    if (this.search) {
      this.search.value = prefs.get("query", "");
      this.query = this.search.value;
      this.search.addEventListener("input", () => {
        this.query = this.search.value;
        prefs.set("query", this.query);
        this.render();
        this.viewChanged();
      });
    }
  }

  /// The visible list changed by a human action (filter or search), not by an initial load.
  /// The editor uses it to leave a review that the chosen view no longer contains.
  viewChanged() {
    this.onViewChange?.();
  }

  toggleBulk(force) {
    this.bulkMode = force === undefined ? !this.bulkMode : force;
    this.selection.clear();
    const mode = $("#bulk-mode");
    if (mode) mode.setAttribute("aria-pressed", String(this.bulkMode));
    this.render();
  }

  exitBulk() {
    this.toggleBulk(false);
  }

  applyBulk(action) {
    if (!this.selection.size) return;
    this.onBulk?.(action, [...this.selection]);
  }

  async load() {
    const [sessions, projects] = await Promise.all([
      api("/api/sessions").catch(() => ({ sessions: [] })),
      api("/api/projects").catch(() => ({ projects: [] })),
    ]);
    this.sessions = sessions.sessions || [];
    this.projects = projects.projects || [];
    this.render();
    this.watchAgentState();
  }

  /// The rail is the one screen that must say "an agent is busy here" without opening the review,
  /// so it re-reads the session states even while the human stays on another page.
  watchAgentState() {
    clearInterval(this.agentTimer);
    this.agentTimer = setInterval(() => this.refreshAgentState(), 8000);
  }

  async refreshAgentState() {
    let sessions;
    try {
      sessions = (await api("/api/sessions")).sessions || [];
    } catch {
      return;
    }
    const before = workingIds(this.sessions);
    this.sessions = sessions;
    if (workingIds(sessions) !== before) this.render();
  }

  setSelected(id) {
    this.selectedId = id;
    if (id) seen.mark(id);
    this.render();
  }

  visible() {
    return this.sessions.filter((session) => this.matches(session));
  }

  /// Does one session belong to the current filter and search? Kept separate from `visible` so
  /// the editor can test its live `meta` instead of a rail row that may be a poll behind.
  matches(session) {
    const active = FILTERS.find((f) => f.id === this.filter) || FILTERS[0];
    if (!active.test(session)) return false;
    const query = this.query.trim().toLowerCase();
    if (unfiltered(query)) return true;
    return [session.title, session.note, session.id, session.url]
      .filter(Boolean)
      .some((field) => field.toLowerCase().includes(query));
  }

  renderChips() {
    if (!this.chips) return;
    const counts = Object.fromEntries(
      FILTERS.map((filter) => [filter.id, this.sessions.filter(filter.test).length]),
    );
    if (this.purge) {
      const cancelled = this.sessions
        .filter((session) => featureState(session) === "cancelled").length;
      this.purge.hidden = cancelled === 0;
      this.purge.textContent = `Delete cancelled (${cancelled})`;
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
        this.viewChanged();
      }),
    );
  }

  render() {
    this.renderChips();
    const bar = $("#rail-bulk");
    if (bar) bar.hidden = !this.bulkMode;
    const bulkLabel = $("#bulk-label");
    if (bulkLabel) bulkLabel.textContent = `${this.selection.size} selected`;
    const sessions = this.visible();
    if (!sessions.length) {
      this.body.innerHTML = `
        <div class="rail-empty">
          ${this.sessions.length ? "Nothing matches that filter." : "No sessions yet."}
        </div>`;
      return;
    }

    // Group by project: the rail is a list of conversations, not of requests.
    const byProject = new Map();
    for (const session of sessions) {
      const slug = session.project || "inbox";
      if (!byProject.has(slug)) byProject.set(slug, []);
      byProject.get(slug).push(session);
    }
    const known = new Map((this.projects || []).map((project) => [project.slug, project]));
    const order = [...byProject.keys()].sort((a, b) => {
      if (a === "inbox") return -1;
      if (b === "inbox") return 1;
      const left = known.get(a)?.name || a;
      const right = known.get(b)?.name || b;
      return left.localeCompare(right);
    });

    const parts = [];
    for (const slug of order) {
      const items = byProject.get(slug);
      const project = known.get(slug);
      const label = project?.name || (slug === "inbox" ? "Inbox" : slug);
      const collapsed = this.collapsed.has(slug);
      const working = items.filter(agentWorking).length;
      parts.push(`
        <div class="rail-project" data-project="${escapeHtml(slug)}" role="button" tabindex="0"
             aria-expanded="${!collapsed}" title="${collapsed ? "Expand" : "Collapse"} ${escapeHtml(label)}">
          <span class="caret">${collapsed ? "▸" : "▾"}</span>
          <span class="pname">${escapeHtml(label)}</span>
          ${working ? `<span class="pworking" title="The agent is working on ${plural(working, "review")}">
            <span class="spin" aria-hidden="true"></span>${working > 1 ? `<b>${working}</b>` : ""}working
          </span>` : ""}
          <span class="pcount">${items.length}</span>
        </div>`);
      if (collapsed) continue;

      const groups = new Map();
      for (const session of items) {
        const day = dayLabel(session.created_at);
        if (!groups.has(day)) groups.set(day, []);
        groups.get(day).push(session);
      }
      const showDays = items.length > 12;
      for (const [day, dayItems] of groups) {
        if (showDays) parts.push(`<div class="rail-group">${escapeHtml(day)}</div>`);
        for (const session of dayItems) {
        const annotations = session.annotation_count || 0;
        const fresh = seen.isNew(session);
        const working = agentWorking(session);
        const feature = featureState(session);
        const decided = feature !== "pending";
        // A submitted round belongs to the agent, so its badge says who has the ball: a round
        // nobody has picked up yet used to read "new" (unread by the human), which is a lie
        // about the state. Only a review that is back in the human's hands can be "new".
        const queued = session.status === "submitted";
        // An agent currently running outranks "new" and the annotation count: it is the one
        // state that changes a few seconds later without the human touching anything. A feature
        // decision is the human's answer to the session, so it outranks the unread marker.
        const badge = working
          ? `<span class="badge work" title="The agent is working on this review">
            <span class="spin" aria-hidden="true"></span>working</span>`
          : feature === "confirmed"
            ? `<span class="badge confirmed" title="Feature confirmed by the human">confirmed</span>`
            : feature === "cancelled"
              ? `<span class="badge cancelled" title="Feature cancelled by the human">cancelled</span>`
              : queued && session.agent_state === "replied"
                ? `<span class="badge replied" title="The agent answered; the run is not finished yet">replied</span>`
                : queued
                  ? `<span class="badge waiting" title="Waiting for an agent — nobody has picked this round up yet">waiting for agent</span>`
                  : fresh
                  ? `<span class="badge new" title="new since you last looked">new</span>`
                  : annotations
                    ? `<span class="badge" title="${plural(annotations, "annotation")}">${annotations}</span>`
                    : "";
        parts.push(`
          <div class="session ${escapeHtml(session.status)}${working ? " working" : ""}${
            decided ? ` feature-${feature}` : ""
          }${this.bulkMode ? " bulk" : ""}"
               role="button" tabindex="0"
               data-id="${escapeHtml(session.id)}"
               aria-label="${escapeHtml(session.title || session.id)}, ${escapeHtml(session.status)}${
                 decided ? `, feature ${feature}` : ""
               }${session.branch ? `, branch ${escapeHtml(session.branch)}` : ""}${
                 queued && !working ? `, ${session.agent_state === "replied" ? "replied" : "waiting for agent"}` : ""
               }"
               aria-current="${session.id === this.selectedId}"
               ${this.bulkMode ? `aria-pressed="${this.selection.has(session.id)}"` : ""}>
            ${this.bulkMode
              ? `<span class="bulk-check" aria-hidden="true">${this.selection.has(session.id) ? "✓" : ""}</span>`
              : `<span class="dot"></span>`}
            <div class="body">
              <div class="name">${escapeHtml(session.title || session.id)}</div>
              <div class="sub">${escapeHtml(relativeTime(session.submitted_at || session.created_at))}${
                session.branch
                  ? ` · <span class="branch" title="${escapeHtml(session.branch)}">${escapeHtml(session.branch)}</span>`
                  : ""
              }${session.note ? " · " + escapeHtml(session.note) : ""}</div>
            </div>
            <span class="row-actions">
              ${decided
                ? `<button class="act" data-act="reopen" title="Reopen feature">↺</button>`
                : `<button class="act" data-act="confirm" title="Confirm feature done">✓</button>`}
              <button class="act" data-act="menu"
                      title="${decided ? "Feature actions (reopen or delete)" : "Feature actions (cancel or delete)"}">✕</button>
            </span>
            ${badge}
          </div>`);
        }
      }
    }
    this.body.innerHTML = parts.join("");

    $$(".session .act", this.body).forEach((button) => {
      button.addEventListener("click", (event) => {
        event.stopPropagation();
        this.onAction?.(button.dataset.act, button.closest(".session").dataset.id);
      });
    });
    $$(".rail-project", this.body).forEach((header) => {
      const toggle = () => {
        const slug = header.dataset.project;
        if (this.collapsed.has(slug)) this.collapsed.delete(slug);
        else this.collapsed.add(slug);
        prefs.set("collapsedProjects", [...this.collapsed]);
        this.render();
      };
      header.addEventListener("click", toggle);
      header.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          toggle();
        }
      });
    });
    $$(".session", this.body).forEach((row) => {
      const activate = () => {
        if (this.bulkMode) {
          const id = row.dataset.id;
          if (this.selection.has(id)) this.selection.delete(id);
          else this.selection.add(id);
          this.render();
          return;
        }
        this.onSelect(row.dataset.id, row);
      };
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
    this.video = null;
    this.videoBar = $("#video-bar");
    this.videoWindow = { start: 0, end: 0 };
    this.ghosts = true;
    this.videoFrame = null;
    this.annotations = [];
    this.history = [];
    this.future = [];
    this.drafts = new Map();
    this.locked = false;
    this.serverAnnotations = "[]";
    this.viewing = null;
    this.live = null;
    this.historyEntries = [];
    this.draftNotice = null;
    // The conversation poll fills these; until it lands, `renderHeader` must not trip over
    // an undefined list (a fresh load of a submitted session renders the header first).
    this.events = [];
    this.thread = [];
    this.selected = new Set();
    this.marquee = null;
    this.tool = "select";
    this.color = prefs.get("color", "#e11d48");
    this.textSize = prefs.get("textSize", 18);
    this.lineWidth = prefs.get("width", 3);
    this.view = { scale: 1, x: 0, y: 0, fitted: false };
    this.pointers = new Map();
    this.pointerInStage = false;
    this.drag = null;
    this.draw = null;
    this.pan = null;
  }

  async init() {
    this.rail = new Rail({
      body: $("#rail-body"),
      chips: $("#chips"),
      search: $("#search"),
      purge: $("#purge-cancelled"),
      selectedId: this.sessionId,
      onSelect: (id) => this.openSession(id),
      onAction: (action, id) => this.railAction(action, id),
      onPurge: (ids) => this.purgeSessions(ids),
      onBulk: async (action, ids) => {
        if (action === "confirm") {
          const choice = await askFeature({
            title: `Confirm ${ids.length} feature${ids.length === 1 ? "" : "s"}?`,
            body: "Each selected session will be marked as confirmed and closed. Your agent may open a pull request and merge it into the integration branch.",
            actions: [
              { value: "keep", label: "Keep working" },
              { value: "confirm", label: "Confirm features", kind: "primary" },
            ],
          });
          if (choice !== "confirm") return;
        }
        if (action === "delete"
          && !confirm(`Delete ${ids.length} session(s)? Their files go too.`)) return;
        try {
          for (const id of ids) {
            if (action === "delete") {
              await api(`/api/sessions/${encodeURIComponent(id)}`, { method: "DELETE" });
            } else {
              await api(`/api/sessions/${encodeURIComponent(id)}/confirm-feature`, { method: "POST" });
            }
          }
        } catch (error) {
          alert(`Could not ${action}: ${error.message}`);
        }
        this.rail.exitBulk();
        // Confirming (or deleting) the review that is open finishes the work on screen; there is
        // no review left to look at, so go back to the dashboard instead of an empty canvas.
        if (ids.includes(this.sessionId) && (action === "delete" || action === "confirm")) {
          location.href = "/";
          return;
        }
        await this.rail.load();
        if (ids.includes(this.sessionId)) {
          try {
            const data = await api(`/api/sessions/${encodeURIComponent(this.sessionId)}`);
            this.meta = data.meta;
            this.renderHeader();
          } catch {
            // The rail already reloaded; the header catches up on the next poll.
          }
        }
      },
      onViewChange: () => this.leaveIfFiltered(),
    });
    this.bindChrome();
    this.bindPanels();
    this.bindCanvas();
    this.bindVideoBar();
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

  /** Confirm, cancel, reopen or delete a feature, straight from the rail. */
  async railAction(action, id) {
    try {
      const session = this.rail.sessions.find((s) => s.id === id);
      const title = session?.title || id;
      const result = await featureAction(action, id, title, featureState(session || {}));
      if (!result) return;
      const finished = result.deleted
        || Boolean(result.meta?.feature_confirmed_at)
        || Boolean(result.meta?.feature_cancelled_at);
      if (id === this.sessionId && finished) {
        // The decision closed the review on screen; the dashboard is the next useful place.
        location.href = "/";
        return;
      }
      if (result.deleted) {
        await this.rail.load();
        return;
      }
      if (id === this.sessionId && result.meta) {
        this.meta = result.meta;
        this.renderHeader();
      }
      await this.rail.load();
    } catch (error) {
      alert(`Could not ${action}: ${error.message}`);
    }
  }

  /// A filter or search may hide the review that is open. Keeping it on the canvas would leave a
  /// selected review that the chosen view says is gone, so return to the dashboard instead. The
  /// editor's `meta` is fresher than the rail's poll, so test the merged record; a session the
  /// rail has never loaded is simply new, not filtered out.
  leaveIfFiltered() {
    if (!this.sessionId || !this.rail) return;
    const entry = this.rail.sessions.find((session) => session.id === this.sessionId);
    if (!entry) return;
    const session = { ...entry, ...(this.meta || {}), id: this.sessionId };
    if (this.rail.matches(session)) return;
    location.href = "/";
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
    this.saveDraft();
    history.pushState({ id }, "", `/s/${encodeURIComponent(id)}?`);
    this.sessionId = id;
    this.root.dataset.sessionId = id;
    this.rail.setSelected(id);
    await this.load(id);
  }

  async load(id) {
    $("#stage").classList.remove("hidden");
    this.viewing = null;
    this.live = null;
    this.updateViewingBar();
    try {
      const data = await api(`/api/sessions/${encodeURIComponent(id)}`);
      this.meta = data.meta;
      lastProject.remember(this.meta.project);
      const server = (data.annotations && data.annotations.annotations) || [];
      this.serverAnnotations = JSON.stringify(server);
      const draft = this.drafts.get(id);
      this.annotations = draft ? draft.annotations : server;
      const composer = $("#composer-text");
      if (composer) composer.value = draft ? draft.prompt || "" : "";
      this.history = [];
      this.future = [];
      this.seenCount = 0;
      this.convCount = 0;
      this.setNewBadge(0);
      this.selected.clear();
      this.renderInspector();
      this.renderHeader();
      this.loadHistory();
      this.startConversationPolling();
      await this.loadMedia(this.meta);
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
    if (this.video) this.video.pause();
    this.video = null;
    if (this.videoBar) this.videoBar.hidden = true;
    this.stage.classList.remove("video-mode");
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
      // The URL is stable while the file behind it changes with every round, so the browser
      // would happily show the previous screenshot forever without a cache-buster.
      image.src = `/sessions/${encodeURIComponent(this.sessionId)}/image?v=${encodeURIComponent(name)}`;
    });
  }

  get isVideo() {
    return this.meta?.media?.kind === "video" && Boolean(this.video);
  }

  mediaWidth() {
    return this.video?.videoWidth || this.image?.naturalWidth || 0;
  }

  mediaHeight() {
    return this.video?.videoHeight || this.image?.naturalHeight || 0;
  }

  /// Load the session media: an image stays an Image, a video becomes an HTMLVideoElement drawn
  /// into the same canvas. Video playback is muted in v1.
  loadMedia(meta) {
    if (meta?.media?.kind !== "video") return this.loadImage(meta?.image || "");
    if (this.video) this.video.pause();
    return new Promise((resolve) => {
      const video = document.createElement("video");
      video.muted = true;
      video.playsInline = true;
      video.preload = "auto";
      video.onloadedmetadata = async () => {
        // A MediaRecorder WebM only learns its duration after a seek; without this the whole
        // timeline would collapse to a zero-length window.
        const duration = await resolvedDuration(video);
        video.currentTime = 0;
        this.image = video;
        this.video = video;
        this.videoWindow = { start: 0, end: duration };
        this.bindVideo(video);
        resolve();
      };
      video.onerror = () => {
        this.image = null;
        this.video = null;
        this.showStageMessage("Video missing", "Could not play this video (unsupported codec?).");
        resolve();
      };
      video.src = `/sessions/${encodeURIComponent(this.sessionId)}/media?v=${encodeURIComponent(meta.image)}`;
    });
  }

  bindVideo(video) {
    const draw = () => { if (this.video === video) this.render(); };
    video.addEventListener("seeked", draw);
    video.addEventListener("timeupdate", () => {
      this.syncVideoBar();
      draw();
    });
    video.addEventListener("play", () => { this.syncVideoBar(); this.frameLoop(); });
    video.addEventListener("pause", () => this.syncVideoBar());
    video.addEventListener("loadedmetadata", () => {
      this.videoWindow = { start: 0, end: video.duration || 0 };
      this.syncVideoBar();
    });
    this.syncVideoBar();
  }

  frameLoop() {
    if (!this.video || this.video.paused) return;
    this.render();
    if (this.video.requestVideoFrameCallback) {
      this.videoFrame = this.video.requestVideoFrameCallback(() => this.frameLoop());
    } else {
      this.videoFrame = requestAnimationFrame(() => this.frameLoop());
    }
  }

  syncVideoBar() {
    const bar = this.videoBar;
    if (!bar) return;
    const video = this.video;
    bar.hidden = !video;
    this.stage.classList.toggle("video-mode", Boolean(video));
    if (!video) return;
    const duration = Number.isFinite(video.duration) ? video.duration : 0;
    const window = this.videoWindow;
    const span = Math.max(0.001, window.end - window.start);
    const seek = $("#video-seek");
    if (seek) {
      seek.min = String(window.start);
      seek.max = String(window.end);
      seek.step = "0.001";
      seek.value = String(Math.min(window.end, Math.max(window.start, video.currentTime)));
    }
    const time = $("#video-time");
    if (time) time.textContent = `${timecode(video.currentTime)} / ${timecode(duration)}`;
    const play = $("#video-play");
    if (play) {
      play.setAttribute("aria-label", video.paused ? "Play video" : "Pause video");
      play.textContent = video.paused ? "▶" : "❚❚";
    }
    const speed = $("#video-speed");
    if (speed) speed.value = String(video.playbackRate || 1);
    const overview = $("#timeline-window");
    if (overview && duration > 0) {
      overview.style.left = `${(window.start / duration) * 100}%`;
      overview.style.width = `${(span / duration) * 100}%`;
    }
    this.renderTimelineMarks();
    this.renderTimelineTicks();
  }

  /// A ruler under the detail slider. The step follows the zoom window, so a 1 s window shows
  /// tenths and a 10 min window shows minutes.
  renderTimelineTicks() {
    const ticks = $("#timeline-ticks");
    if (!ticks || !this.video) return;
    const { start, end } = this.videoWindow;
    const span = end - start;
    if (!(span > 0.01)) {
      ticks.innerHTML = "";
      return;
    }
    const steps = [0.1, 0.2, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800];
    const step = steps.find((candidate) => span / candidate <= 5) || 3600;
    const first = Math.ceil((start - 1e-9) / step) * step;
    const labels = [];
    for (let at = first; at <= end + 1e-6; at += step) {
      const left = ((at - start) / span) * 100;
      const short = step < 1
        ? timecode(at).replace(/^0?(\d+):(\d{2})\.\d(\d{2})$/, (_, m, s, tenth) => `${m}:${s}.${tenth}`)
        : timecode(at).replace(/\.\d+$/, "");
      // Keep the edge labels inside the bar: half a label hanging over the end reads clipped.
      const place = left < 2
        ? "left:0;transform:none"
        : left > 98
          ? "left:100%;transform:translateX(-100%)"
          : `left:${left}%;transform:translateX(-50%)`;
      labels.push(`<span class="timeline-tick" style="${place}">${escapeHtml(short)}</span>`);
    }
    ticks.innerHTML = labels.join("");
  }

  renderTimelineMarks() {
    const layer = $("#timeline-marks");
    if (!layer) return;
    const window = this.videoWindow;
    const span = Math.max(0.001, window.end - window.start);
    layer.innerHTML = this.annotations
      .map((annotation, index) => {
        if (typeof annotation.at !== "number") return "";
        if (annotation.at < window.start || annotation.at > window.end) return "";
        const at = annotation.at;
        const left = ((at - window.start) / span) * 100;
        const active = Math.abs((this.video?.currentTime || 0) - at) < 0.2;
        const hasRange = typeof annotation.until === "number" && annotation.until > at;
        const range = hasRange
          ? `<span class="timeline-range" style="left:${left}%;width:${
            (Math.min(window.end, annotation.until) - at) / span * 100
          }%;background:${escapeHtml(annotation.color || "#e11d48")}"></span>`
          : "";
        return `${range}<button class="timeline-mark${active ? " active" : ""}" data-mark="${index}"
          style="left:${left}%;background:${escapeHtml(annotation.color || "#e11d48")}"
          title="${escapeHtml(timecode(at))}${hasRange ? `–${escapeHtml(timecode(annotation.until))}` : ""} · annotation ${index + 1}"
          aria-label="Seek to annotation ${index + 1} at ${escapeHtml(timecode(at))}"></button>`;
      })
      .join("");

    // The overview always shows every annotation, even when the zoomed window has moved away
    // from it: the strip is the map of the whole clip.
    const overview = $("#timeline-overview-marks");
    if (!overview) return;
    const duration = Number.isFinite(this.video?.duration) ? this.video.duration : 0;
    overview.innerHTML = duration > 0
      ? this.annotations.map((annotation) => {
        if (typeof annotation.at !== "number") return "";
        const color = escapeHtml(annotation.color || "#e11d48");
        const left = (annotation.at / duration) * 100;
        const hasRange = typeof annotation.until === "number" && annotation.until > annotation.at;
        const band = hasRange
          ? `<span class="timeline-overview-band" style="left:${left}%;width:${
            (Math.min(duration, annotation.until) - annotation.at) / duration * 100
          }%;background:${color}"></span>`
          : "";
        return `${band}<span class="timeline-overview-mark" style="left:${left}%;background:${color}"></span>`;
      }).join("")
      : "";
  }

  seekTo(seconds, selectIndex = -1) {
    if (!this.video) return;
    this.video.currentTime = Math.min(this.video.duration || 0, Math.max(0, seconds));
    if (selectIndex >= 0) this.select(selectIndex);
    this.syncVideoBar();
    this.render();
  }

  zoomTimeline(factor, center) {
    if (!this.video) return;
    const duration = this.video.duration || 0;
    const window = this.videoWindow;
    const span = Math.max(0.1, window.end - window.start);
    const next = Math.min(duration, Math.max(0.25, span * factor));
    const anchor = center ?? this.video.currentTime ?? window.start;
    let start = anchor - ((anchor - window.start) / span) * next;
    start = Math.max(0, Math.min(duration - next, start));
    this.videoWindow = { start, end: start + next };
    this.syncVideoBar();
    this.render();
  }

  bindVideoBar() {
    const play = $("#video-play");
    play?.addEventListener("click", () => {
      if (!this.video) return;
      if (this.video.paused) this.video.play().catch(() => {});
      else this.video.pause();
    });
    const step = (direction) => {
      if (!this.video) return;
      const fps = Number(this.meta?.media?.fps) || 30;
      this.seekTo(this.video.currentTime + direction / fps);
    };
    $("#video-step-back")?.addEventListener("click", () => step(-1));
    $("#video-step-forward")?.addEventListener("click", () => step(1));
    $("#video-speed")?.addEventListener("change", (event) => {
      if (this.video) this.video.playbackRate = Number(event.target.value) || 1;
    });
    const ghosts = $("#video-ghosts");
    ghosts?.addEventListener("click", () => {
      this.ghosts = !this.ghosts;
      ghosts.setAttribute("aria-pressed", String(this.ghosts));
      this.render();
    });
    $("#timeline-zoom-in")?.addEventListener("click", () => this.zoomTimeline(1 / 1.6));
    $("#timeline-zoom-out")?.addEventListener("click", () => this.zoomTimeline(1.6));
    $("#timeline-fit")?.addEventListener("click", () => {
      if (!this.video) return;
      this.videoWindow = { start: 0, end: this.video.duration || 0 };
      this.syncVideoBar();
      this.render();
    });
    $("#video-seek")?.addEventListener("input", (event) => {
      if (this.video) this.seekTo(Number(event.target.value));
    });
    $("#video-timeline")?.addEventListener("wheel", (event) => {
      if (!this.video) return;
      event.preventDefault();
      event.stopPropagation();
      const rect = event.currentTarget.getBoundingClientRect();
      const at = this.videoWindow.start
        + ((event.clientX - rect.left) / Math.max(1, rect.width))
          * (this.videoWindow.end - this.videoWindow.start);
      this.zoomTimeline(event.deltaY < 0 ? 1 / 1.2 : 1.2, at);
    }, { passive: false });
    $("#timeline-overview")?.addEventListener("click", (event) => {
      if (!this.video || !this.video.duration) return;
      const rect = event.currentTarget.getBoundingClientRect();
      this.seekTo(((event.clientX - rect.left) / Math.max(1, rect.width)) * this.video.duration);
    });
    $("#timeline-marks")?.addEventListener("click", (event) => {
      const mark = event.target.closest("[data-mark]");
      if (!mark) return;
      event.stopPropagation();
      const index = Number(mark.dataset.mark);
      const at = this.annotations[index]?.at;
      if (typeof at === "number") this.seekTo(at, index);
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
    const branch = $("#branch-edit");
    if (branch) {
      branch.hidden = !this.meta.branch;
      branch.textContent = this.meta.branch || "";
    }
    const waiting = this.meta.status === "submitted";
    const button = $("#submit");
    if (waiting) {
      button.innerHTML = '<span class="stop-square" aria-hidden="true"></span>STOP';
    } else {
      button.textContent = this.meta.status === "open" ? "Submit review" : "Submit again";
    }
    button.classList.toggle("stop", waiting);
    const composer = $("#composer-text");
    if (composer) {
      composer.disabled = waiting;
      composer.placeholder = waiting
        ? "Waiting for the agent — press STOP to edit again"
        : "What should change? Sent with the annotations…";
    }
    this.updateLock();
    const stageStatus = $("#stage-status");
    if (stageStatus) {
      const submitted = this.meta.status === "submitted";
      // A historical round is not the round the agent is working on: the live toast must not
      // hover over an archived screenshot.
      stageStatus.hidden = !submitted || Boolean(this.viewing);
      if (submitted && !this.viewing) {
        const last = (this.events || [])[this.events.length - 1];
        stageStatus.textContent = agentWorking(this.meta) && last
          ? `Agent is working — ${last.message}`
          : this.meta.agent_state === "replied"
            // The answer can land before `resolve`; "waiting for the agent" would contradict
            // the reply sitting in the conversation.
            ? "The agent replied — read it in the conversation"
            : "Waiting for the agent…";
      }
    }
    this.applyTitle();
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
        if (this.readOnly) return;
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
    const convLog = $("#conv-log");
    if (convLog) {
      convLog.addEventListener("scroll", () => {
        if (convLog.scrollHeight - convLog.scrollTop - convLog.clientHeight < 40) {
          this.seenCount = this.convCount;
          this.setNewBadge(0);
        }
      });
      $("#conv-new")?.addEventListener("click", () => this.seenConversation());
    }
    $("#viewing-bar")?.addEventListener("click", () => this.showCurrent());
    const syncPanels = () => {
      const app = $("#app");
      const rail = $("#toggle-rail");
      const inspector = $("#toggle-inspector");
      const railHidden = app.classList.contains("rail-hidden");
      const inspectorHidden = app.classList.contains("inspector-hidden");
      if (rail) {
        rail.setAttribute("aria-pressed", String(!railHidden));
        rail.title = `${railHidden ? "Show" : "Hide"} sessions ( [ )`;
      }
      if (inspector) {
        inspector.setAttribute("aria-pressed", String(!inspectorHidden));
        inspector.title = `${inspectorHidden ? "Show" : "Hide"} inspector ( ] )`;
      }
    };
    $("#toggle-rail").addEventListener("click", () => {
      const app = $("#app");
      app.classList.toggle("rail-hidden");
      prefs.set("railHidden", app.classList.contains("rail-hidden"));
      syncPanels();
      this.resize();
    });
    $("#toggle-inspector").addEventListener("click", () => {
      const app = $("#app");
      app.classList.toggle("inspector-hidden");
      prefs.set("inspectorHidden", app.classList.contains("inspector-hidden"));
      syncPanels();
      this.resize();
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

    if (prefs.get("railHidden", false)) $("#app").classList.add("rail-hidden");
    if (prefs.get("inspectorHidden", false)) $("#app").classList.add("inspector-hidden");
    syncPanels();

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

    $("#zoom-in").addEventListener("click", () => { if (!this.locked) this.zoomAt(1.25); });
    $("#zoom-out").addEventListener("click", () => { if (!this.locked) this.zoomAt(0.8); });
    $("#zoom-fit").addEventListener("click", () => { if (!this.locked) this.fit(); });
    $("#zoom-100").addEventListener("click", () => { if (!this.locked) this.setZoom(1); });
    NewReview.wire();
    $("#new-review").addEventListener("click", () => NewReview.open());
    $("#branch-edit")?.addEventListener("click", async () => {
      const current = this.meta.branch || "";
      const branch = await askBranch(current, this.meta.status === "submitted");
      if (!branch) return;
      try {
        const body = await api(`/api/sessions/${encodeURIComponent(this.sessionId)}/branch`, {
          method: "PUT",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ branch }),
        });
        this.meta = body.meta;
        this.renderHeader();
        this.rail.load();
      } catch (error) {
        alert(`Could not set the branch: ${error.message}`);
      }
    });
    addEventListener("resize", () => this.resize());
    addEventListener("popstate", () => {
      const id = location.pathname.split("/").filter(Boolean).pop();
      if (id && id !== this.sessionId) {
        this.saveDraft();
        this.sessionId = id;
        this.rail.setSelected(id);
        this.load(id);
      }
    });
  }

  syncWidthLabel() {
    $("#width-value").textContent = `${this.lineWidth}px`;
  }

  /**
   * The three dividers: the rail's right edge, the inspector's left edge and the top of the
   * conversation. Sizes land in CSS variables on #app, so the grid follows the drag, and are
   * remembered per browser. The stage is re-measured on every move or the canvas keeps its
   * old pixel size while zooming around a stale centre.
   */
  bindPanels() {
    const app = $("#app");
    const inspector = $(".inspector");
    const conversation = $("#conversation");
    const width = (name, fallback) => parseFloat(app.style.getPropertyValue(name)) || fallback;
    const inspectorWidth = () => width("--inspector", 304);

    // The rail is shared with the dashboard; the editor only adds the inspector limit.
    const rail = bindRailResize({
      max: () => Math.max(160, Math.min(560, innerWidth - inspectorWidth() - 360)),
      onResize: () => this.resize(),
    });
    const railWidth = () => rail?.width() ?? 272;

    const setInspector = (px, persist = true) => {
      app.style.setProperty("--inspector", `${px}px`);
      if (persist) prefs.set("inspectorWidth", px);
      this.resize();
    };
    const setConversation = (px, persist = true) => {
      app.style.setProperty("--conv", `${px}px`);
      if (persist) prefs.set("convHeight", px);
    };

    // A column may not eat the stage: leave room for a readable canvas on either side.
    const inspectorMax = () => Math.max(200, Math.min(680, innerWidth - railWidth() - 360));
    // Whatever the conversation takes, the annotation list and the disclosures keep some room.
    const conversationMax = () => Math.max(180, inspector.clientHeight - 170);

    // Restoring (and clamping) is not a resize: keep the preferred size for a wider window.
    setInspector(Math.min(prefs.get("inspectorWidth", 304), inspectorMax()), false);
    setConversation(Math.min(prefs.get("convHeight", 300), conversationMax()), false);

    bindSplitter($("#inspector-resizer"), {
      sign: -1,
      min: () => 240,
      max: inspectorMax,
      size: inspectorWidth,
      apply: setInspector,
    });
    bindSplitter($("#conv-resizer"), {
      axis: "y",
      sign: -1,
      min: () => 140,
      max: conversationMax,
      size: () => conversation.getBoundingClientRect().height,
      apply: setConversation,
    });

    addEventListener("resize", () => {
      setInspector(Math.min(inspectorWidth(), inspectorMax()), false);
      setConversation(Math.min(prefs.get("convHeight", 300), conversationMax()), false);
    });
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

      // Video transport, matching the usual editor keys. Drawing tools keep their letters.
      if (this.video && !meta) {
        const key = event.key.toLowerCase();
        if (key === "k") {
          event.preventDefault();
          if (this.video.paused) this.video.play().catch(() => {});
          else this.video.pause();
          return;
        }
        if (key === "j" || key === "l") {
          event.preventDefault();
          this.seekTo(this.video.currentTime + (key === "j" ? -1 : 1));
          return;
        }
        if (key === "," || key === ".") {
          event.preventDefault();
          const fps = Number(this.meta?.media?.fps) || 30;
          this.seekTo(this.video.currentTime + (key === "," ? -1 : 1) / fps);
          return;
        }
      }

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
        case "+": case "=": if (!this.locked) this.zoomAt(1.25); break;
        case "-": if (!this.locked) this.zoomAt(0.8); break;
        case "0": if (!this.locked) this.fit(); break;
        case "1": if (!this.locked) this.setZoom(1); break;
        case "[": $("#toggle-rail").click(); break;
        case "]": $("#toggle-inspector").click(); break;
        default: break;
      }
    });
  }

  setTool(tool) {
    if (this.viewing || this.locked) return;
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
    const width = this.mediaWidth();
    const height = this.mediaHeight();
    if (!width || !height) return;
    const rect = this.stage.getBoundingClientRect();
    const bar = this.videoBar && !this.videoBar.hidden ? this.videoBar.offsetHeight + 8 : 0;
    const available = Math.max(80, rect.height - bar);
    const scale = Math.min(rect.width / width, available / height) * 0.94;
    this.view.scale = clamp(scale, 0.05, 8);
    this.view.x = (rect.width - width * this.view.scale) / 2;
    this.view.y = (available - height * this.view.scale) / 2;
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

  /// Annotation visibility at the current video time: the current moment is full, other moments
  /// are faint ghosts that fade in as their time approaches. Images always render at full.
  annotationOpacity(annotation, index) {
    if (!this.isVideo || !this.video) return 1;
    if (this.selected.has(index)) return 1;
    if (typeof annotation.at !== "number") return 1;
    const current = this.video.currentTime || 0;
    const at = annotation.at;
    const until = typeof annotation.until === "number" ? Math.max(at, annotation.until) : null;
    const distance = until !== null && current >= at && current <= until
      ? 0
      : until !== null
        ? (current < at ? at - current : current - until)
        : Math.abs(current - at);
    const opacity = Math.max(0.12, 1 - distance / 0.35);
    if (!this.ghosts && opacity < 0.99) return 0;
    return opacity;
  }

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
      const opacity = this.annotationOpacity(annotation, index);
      if (!opacity) return;
      ctx.globalAlpha = opacity;
      this.paint(ctx, annotation, this.selected.has(index));
      ctx.globalAlpha = 1;
    });
    this.annotations.forEach((annotation, index) => {
      if (annotation.type === "text") return;
      const opacity = this.annotationOpacity(annotation, index);
      if (!opacity) return;
      ctx.globalAlpha = opacity;
      this.paintBadge(ctx, annotation, index + 1, 12 / this.view.scale, 2 / this.view.scale);
      ctx.globalAlpha = 1;
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

  /// Every annotation change goes through `mutate`, so the lock lives here as well as on the
  /// pointer and tool guards: a submitted round is the agent's to read, not the human's to edit.
  /// Keyboard Delete/arrow keys and the inspector's align/layer/group buttons used to slip past
  /// the canvas guard and change annotations anyway.
  get readOnly() {
    return Boolean(this.locked || this.viewing);
  }

  mutate(fn) {
    if (this.readOnly) return;
    this.snapshot();
    fn();
    this.renderInspector();
    this.render();
  }

  undo() {
    if (this.readOnly || !this.history.length) return;
    this.future.push(JSON.stringify(this.annotations));
    this.annotations = JSON.parse(this.history.pop());
    this.selected.clear();
    this.renderInspector();
    this.render();
  }

  redo() {
    if (this.readOnly || !this.future.length) return;
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
      if (this.locked) return;
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
      if (!this.image || this.locked) return;
      // The options bar, the inline text editor and the "back to current" pill live inside the
      // stage. Capturing the pointer for marquee/pan on those would swallow their click events.
      if (event.target.closest(".options, .text-editor, .viewing-bar, .video-bar")) return;

      // Drawing on a moving video would capture a different frame than the one the human saw;
      // any canvas interaction pauses first. The play button resumes.
      if (this.isVideo && this.video && !this.video.paused) this.video.pause();

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
        stage.classList.remove("panning");
        return;
      }
      const point = this.toImage(event);

      // A historical round (and any other read-only view) is a viewer: there is nothing to
      // select or draw, so the main button drags the picture. Without this, `viewing` returned
      // from pointerdown before any pan branch and the history screenshot could not be moved.
      if (event.button === 1 || event.button === 2 || this.spacePan || this.readOnly) {
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
        const at = this.isVideo && this.video
          ? { at: Number(this.video.currentTime.toFixed(3)) }
          : {};
        if (drawing.type === "pen" && drawing.points.length > 1) {
          this.mutate(() => {
            this.annotations.push({
              id: uid(),
              type: "pen",
              color: drawing.color,
              width: drawing.width,
              points: drawing.points.map((p) => ({ x: Math.round(p.x), y: Math.round(p.y) })),
              ...at,
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
              ...at,
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
              ...at,
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
              ...at,
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

    // space to pan. The old guard required `document.body` as the target, so after clicking a
    // history row (focus stays on the button) space+drag silently did nothing. The pointer is
    // the intent signal: while it hovers the stage, space belongs to the canvas, not to a
    // focused control — but never while typing into a field.
    stage.addEventListener("pointerenter", () => { this.pointerInStage = true; });
    stage.addEventListener("pointerleave", () => { this.pointerInStage = false; });
    addEventListener("keydown", (event) => {
      const tag = event.target.tagName;
      const typing = tag === "INPUT" || tag === "TEXTAREA" || event.target.isContentEditable;
      if (event.code === "Space" && !event.repeat && !typing
        && (event.target === document.body || this.pointerInStage)) {
        event.preventDefault();
        this.spacePan = true;
      }
    });
    addEventListener("keyup", (event) => {
      if (event.code === "Space") this.spacePan = false;
    });
  }

  startText(point, event, existingIndex = -1) {
    if (this.readOnly) return;
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
        const at = this.isVideo && this.video
          ? { at: Number(this.video.currentTime.toFixed(3)) }
          : {};
        this.mutate(() => {
          this.annotations.push({
            id: uid(),
            type: "text",
            color: this.color,
            x: Math.round(point.x),
            y: Math.round(point.y),
            size: this.textSize,
            text,
            ...at,
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
    if (this.viewing || this.locked) {
      const editor = $("#note-editor");
      if (editor) editor.classList.add("hidden");
      // The align/layer/group bar is a set of write buttons: leaving it armed after a submit was
      // one of the ways the lock leaked, and an armed button that refuses clicks is worse.
      const tools = $("#selection-tools");
      if (tools) tools.classList.add("hidden");
      $("#ann-count").textContent = String(this.annotations.length);
      body.innerHTML = this.annotations.map((annotation, index) => {
        const label = String(
          annotation.note || annotation.text || `${annotation.type}`,
        );
        const at = typeof annotation.at === "number" ? timecode(annotation.at) : "";
        const until = typeof annotation.until === "number" ? `–${timecode(annotation.until)}` : "";
        return `<div class="ann-row readonly">
          <span class="ann-num" style="background:${escapeHtml(annotation.color || "#e11d48")}">${index + 1}</span>
          <span class="label"><b>${escapeHtml(label)}</b><span>${escapeHtml(annotation.type)}${
            at ? ` · ${escapeHtml(at + until)}` : ""
          }</span></span>
        </div>`;
      }).join("") || `<div class="inspector-empty">No annotations in this round.</div>`;
      return;
    }
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
      // A pen has `points`, not `start`/`end`. Reaching for `start.x` here threw before the
      // inspector list (and, because `mutate` renders the inspector first, the canvas badge)
      // could be built; the live stroke hid the failure. Every shape type gets an explicit label.
      const label = annotation.type === "text"
        ? escapeHtml(annotation.text || "")
        : annotation.type === "rect" || annotation.type === "ellipse" || annotation.type === "arrow"
          ? `${Math.round(annotation.start.x)}, ${Math.round(annotation.start.y)} → ${Math.round(annotation.end.x)}, ${Math.round(annotation.end.y)}`
          : annotation.type === "pen"
            ? `freehand · ${annotation.points?.length || 0} points`
            : `${Math.round(annotation.start?.x ?? 0)}, ${Math.round(annotation.start?.y ?? 0)}`;
      const note = String(annotation.note || "").trim();
      const primary = note ? escapeHtml(note) : label;
      const at = typeof annotation.at === "number" ? timecode(annotation.at) : "";
      const until = typeof annotation.until === "number" ? `–${timecode(annotation.until)}` : "";
      const range = at ? `${at}${until}` : "";
      const meta = `${annotation.type}${annotation.group ? " · group" : ""}${
        range ? ` · ${range}` : ""
      }`;
      const rangeButton = this.isVideo && at
        ? `<button class="end" data-end="${index}" aria-label="${
          typeof annotation.until === "number" ? "Clear range end" : "Set range end at the current time"
        }" title="${
          typeof annotation.until === "number" ? "Clear the range end" : "Set the range end to the current time"
        }">${typeof annotation.until === "number" ? "×" : "⏱"}</button>`
        : "";
      return `
        <div class="ann-row" data-index="${index}" aria-current="${this.selected.has(index)}">
          <span class="ann-num" style="background:${escapeHtml(annotation.color || "#e11d48")}">${index + 1}</span>
          <span class="label"><b title="${primary}">${primary}</b><span>${escapeHtml(meta)}</span></span>
          ${rangeButton}
          <button class="del" data-delete="${index}" title="Delete">✕</button>
        </div>`;
    }).join("");

    $$(".ann-row", body).forEach((row) => {
      row.addEventListener("click", (event) => {
        if (event.target.dataset.delete !== undefined || event.target.dataset.end !== undefined) return;
        const index = Number(row.dataset.index);
        const additive = event.shiftKey || event.ctrlKey || event.metaKey;
        this.selectAt(index, additive);
        if (!additive) {
          if (this.isVideo && typeof this.annotations[index].at === "number") {
            this.seekTo(this.annotations[index].at, index);
          } else {
            this.revealSelection();
          }
        }
      });
    });
    $$(".end", body).forEach((button) => {
      button.addEventListener("click", (event) => {
        event.stopPropagation();
        const index = Number(button.dataset.end);
        const annotation = this.annotations[index];
        if (!this.video || !annotation) return;
        this.mutate(() => {
          if (typeof annotation.until === "number") {
            delete annotation.until;
            return;
          }
          const at = annotation.at || 0;
          const duration = Number(this.video.duration) || at + 1;
          let end = Math.max(this.video.currentTime, at + 0.04);
          // Clicking the range button while parked on the annotation would otherwise stamp a
          // zero-length range that the agent cannot use. Give it a second of video.
          if (end - at < 0.05) end = at + 1;
          annotation.until = Number(Math.min(duration, end).toFixed(3));
        });
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
    if (this.video) this.renderTimelineMarks();
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
    list.className = "inspector-empty";
    list.textContent = "Loading…";
    try {
      const data = await api(`/api/sessions/${encodeURIComponent(this.sessionId)}/history`);
      const rounds = data.submissions || [];
      this.historyEntries = rounds;
      const currentArchived = rounds.find((round) => round.current);
      const rows = [];
      // The live count belongs to the canvas, never to the round being previewed.
      const liveCount = this.viewing
        ? (this.live?.count ?? 0)
        : (this.annotations || []).length;
      // `Current` is the way back from a historical round; it is omitted when the newest
      // archived round *is* the live one (labelled "Round N (current)" instead).
      if (!currentArchived) {
        rows.push(`<div class="ann-row history-row live${this.viewing ? "" : " selected"}"
                        data-round="current" role="button" tabindex="0">
          <span class="glyph">●</span>
          <span class="label"><b>Current</b><span>${plural(liveCount, "annotation")} · live</span></span>
        </div>`);
      }
      for (const round of rounds) {
        const isLive = Boolean(round.current);
        const selected = this.viewing
          ? this.viewing.stamp === round.stamp
          : isLive;
        rows.push(`<div class="ann-row history-row${isLive ? " live" : ""}${selected ? " selected" : ""}"
                        data-round="${escapeHtml(round.stamp)}" data-live="${isLive ? "1" : "0"}" role="button" tabindex="0">
          <span class="glyph">${round.round}</span>
          <span class="label">
            <b>Round ${round.round}${round.current ? " (current)" : ""}</b>
            <span>${escapeHtml(stampTime(round.stamp))} · ${plural(round.annotations, "annotation")}${
              round.source ? " · image" : ""
            }</span>
          </span>
        </div>`);
      }
      if (!rows.length) {
        list.className = "inspector-empty";
        list.textContent = "No rounds yet.";
        return;
      }
      list.className = "history-list";
      list.innerHTML = rows.join("");
      const counter = $("#history-count");
      if (counter) counter.textContent = String(rows.length);
      $$("[data-round]", list).forEach((row) => {
        // A round the canvas still shows is the live canvas itself; clicking it must not open a
        // read-only copy of what is already on screen.
        const go = () => row.dataset.round === "current" || row.dataset.live === "1"
          ? this.showCurrent()
          : this.viewRound(row.dataset.round);
        row.addEventListener("click", go);
        row.addEventListener("keydown", (event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            go();
          }
        });
      });
    } catch (error) {
      list.className = "inspector-empty";
      list.textContent = `Could not load history: ${error.message}`;
    }
  }

  /// A submitted round is history: show its source screenshot and archived annotations, read-only.
  async viewRound(stamp) {
    if (this.viewing?.stamp === stamp) return;
    this.saveDraft();
    try {
      const response = await fetch(
        `/sessions/${encodeURIComponent(this.sessionId)}/history/${encodeURIComponent(`${stamp}.json`)}`,
      );
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const set = await response.json();
      const round = (this.historyEntries || []).find((entry) => entry.stamp === stamp);
      const file = round?.source ? `${stamp}-source.png` : `${stamp}.png`;
      const image = await this.loadImageFrom(
        `/sessions/${encodeURIComponent(this.sessionId)}/history/${encodeURIComponent(file)}`,
      );
      if (!image) return;
      if (!this.viewing) {
        this.live = {
          image: this.image,
          annotations: this.annotations,
          meta: this.meta,
          count: this.annotations.length,
        };
      }
      this.viewing = {
        stamp,
        round: round?.round || 0,
        annotations: Array.isArray(set.annotations) ? set.annotations : [],
      };
      this.image = image;
      this.annotations = this.viewing.annotations;
      this.selected.clear();
      this.updateViewingBar();
      this.renderHeader();
      this.renderInspector();
      this.fit();
      this.render();
      this.loadHistory();
    } catch (error) {
      console.warn("round view failed", error);
    }
  }

  async showCurrent() {
    this.viewing = null;
    this.live = null;
    this.updateViewingBar();
    try {
      const data = await api(`/api/sessions/${encodeURIComponent(this.sessionId)}`);
      this.meta = data.meta;
      this.serverAnnotations = JSON.stringify(
        (data.annotations && data.annotations.annotations) || [],
      );
      this.annotations = (data.annotations && data.annotations.annotations) || [];
      const draft = this.drafts.get(this.sessionId);
      if (draft) {
        this.annotations = draft.annotations;
        const composer = $("#composer-text");
        if (composer) composer.value = draft.prompt || "";
      }
      await this.loadMedia(this.meta);
      this.selected.clear();
      this.renderHeader();
      this.renderInspector();
      this.fit();
      this.render();
      this.loadHistory();
    } catch (error) {
      console.warn("back to current failed", error);
    }
  }

  updateLock() {
    const before = this.locked;
    this.locked = Boolean(this.meta && this.meta.status === "submitted") && !this.viewing;
    // No modal overlay: the stage status pill is the visible signal. The drawing tools are
    // disabled as well — a button that looks armed but silently refuses every click reads as a
    // broken lock, which is exactly how this was reported.
    const status = $("#stage-status");
    if (status) status.title = this.locked ? "STOP unlocks the canvas for another round." : "";
    $$('.tool[data-tool]').forEach((button) => {
      button.toggleAttribute("disabled", this.readOnly);
    });
    ["#zoom-in", "#zoom-out", "#zoom-fit", "#zoom-100"].forEach((id) => {
      const button = $(id);
      if (button) button.disabled = this.locked;
    });
    const dim = $("#stage-dim");
    if (dim) dim.hidden = !this.locked;
    if (this.locked) {
      this.pan = null;
      this.draw = null;
      this.marquee = null;
      this.drag = null;
      this.pointers.clear();
      this.spacePan = false;
      this.stage.classList.remove("panning");
    }
    this.root.classList.toggle("locked", this.locked);
    // The inspector was rendered before the meta arrived on a reload: re-render it, or a
    // submitted round still shows its delete buttons and write toolbar.
    if (before !== this.locked) this.renderInspector();
  }

  updateViewingBar() {
    const bar = $("#viewing-bar");
    if (!bar) return;
    bar.hidden = !this.viewing;
    // The cursor is the affordance: a historical round can be dragged like a map, and the grab
    // hand says so before the first drag.
    this.stage.classList.toggle("viewing", Boolean(this.viewing));
    if (this.viewing) {
      bar.textContent = `Viewing round ${this.viewing.round || ""} — drag to pan, Ctrl+scroll to zoom · back to current`;
    }
  }

  loadImageFrom(url) {
    return new Promise((resolve) => {
      const image = new Image();
      image.onload = () => resolve(image);
      image.onerror = () => resolve(null);
      image.src = url;
    });
  }

  // ---------------------------------------------------------- conversation

  /// The agent answers with `reply` and narrates with `progress`; without a poll the human would
  /// have to reload to notice, which is the "I submitted and heard nothing" dead end.
  startConversationPolling() {
    clearInterval(this.threadTimer);
    this.loadConversation();
    this.threadTimer = setInterval(() => this.loadConversation({ quiet: true }), 5000);
  }

  async loadConversation({ quiet = false } = {}) {
    const log = $("#conv-log");
    if (!log) return;
    // A historical round is a viewer; the live poll must not overwrite it.
    if (this.viewing) return;
    if (!quiet && !log.childElementCount) {
      log.innerHTML = `<div class="inspector-empty">Loading…</div>`;
    }
    try {
      const id = encodeURIComponent(this.sessionId);
      const [session, thread, events] = await Promise.all([
        api(`/api/sessions/${id}`),
        api(`/api/sessions/${id}/thread`),
        api(`/api/sessions/${id}/events`),
      ]);
      const previousImage = this.meta?.image;
      this.meta = session.meta;
      this.thread = Array.isArray(thread.messages) ? thread.messages : [];
      this.events = Array.isArray(events.events) ? events.events : [];
      this.renderHeader();
      this.renderConversation();
      // The agent posted a new round: the canvas switches to the new screenshot.
      if (previousImage && this.meta.image && this.meta.image !== previousImage) {
        this.annotations = (session.annotations && session.annotations.annotations) || [];
        this.selected.clear();
        this.history = [];
        this.future = [];
        this.ghosts = null;
        await this.loadMedia(this.meta);
        this.fit();
        this.render();
        this.renderInspector();
        this.loadHistory();
      }
    } catch (error) {
      if (!quiet) {
        log.innerHTML = `<div class="inspector-empty">Could not load the conversation: ${escapeHtml(error.message)}</div>`;
      }
    }
  }

  /// The step lines say *what* the agent did; this says *how much* and *whether it is still
  /// going* — the question a human asks while staring at a submitted review. Modelled on the
  /// run summary an interactive agent shows in its own chat ("Used 12 tools: Edit, Shell and Run").
  runSummary() {
    const events = this.events || [];
    // Only this round: an earlier round's steps would inflate the count. A round starts at the
    // submission, or — once the agent has posted the next screenshot — at that post.
    const start = this.meta?.submitted_at
      || (this.thread || []).filter((message) => message.image).map((message) => message.at).pop()
      || "";
    const round = start ? events.filter((event) => String(event.at || "") >= String(start)) : events;
    if (!round.length) return "";

    const toolEvents = round.filter((event) => event.state === "tool");
    // Older agents narrate without `state`; counting their lines is closer than showing nothing.
    const steps = toolEvents.length || round.length;
    const tools = [];
    for (const event of toolEvents.concat(toolEvents.length ? [] : round)) {
      const name = String(event.tool || "").trim();
      if (name && !tools.includes(name)) tools.push(name);
    }
    const used = tools.length
      ? `Used ${plural(steps, "tool")}: ${escapeHtml(listNames(tools))}`
      : `Used ${plural(steps, "tool")}`;
    const last = round[round.length - 1];
    const working = agentWorking(this.meta);
    const replied = this.meta?.agent_state === "replied";
    // `reply` is a message, not the end of the run: the agent often answers and then keeps
    // working for minutes. Only a round that left the agent's hands (a new round or `resolve`)
    // is finished, so "Run finished" must never appear while STOP is still armed (2026-10-30).
    const submitted = this.meta?.status === "submitted";
    const title = working
      ? "Working"
      : replied && submitted
        ? "Agent replied — waiting for it to finish"
        : replied
          ? "Run finished"
          : `Last step ${escapeHtml(relativeTime(last.at) || clockTime(last.at))}`;
    // How long ago the last step was is the honest answer to "is it still working?": a spinner
    // must never be the only evidence.
    const age = relativeTime(last.at);
    const sub = `${used}${(working || replied) && age ? ` · last step ${escapeHtml(age)}` : ""}`;
    const state = working ? " working" : replied && submitted ? " pending" : "";
    return `<div class="conv-run${state}">`
      + `<span class="conv-run-mark" aria-hidden="true"></span><div>`
      + `<div class="conv-run-title">${title}</div>`
      + `<div class="conv-run-sub">${sub}</div></div></div>`;
  }

  renderConversation() {
    const log = $("#conv-log");
    if (!log) return;
    const items = [];
    if (this.meta?.note) {
      items.push({ at: this.meta.created_at || "", kind: "request", text: this.meta.note });
    }
    if (this.meta?.prompt) {
      items.push({ at: this.meta.submitted_at || "", kind: "human", text: this.meta.prompt });
    }
    for (const event of this.events || []) {
      items.push({ at: event.at || "", kind: "event", text: event.message || "", state: event.state });
    }
    for (const message of this.thread || []) {
      items.push({ at: message.at || "", kind: "agent", text: message.message || "" });
    }
    items.sort((a, b) => String(a.at).localeCompare(String(b.at)));

    const activity = $("#conv-activity");
    if (activity) {
      const last = (this.events || [])[this.events.length - 1];
      activity.textContent = agentWorking(this.meta) && last ? `${last.message}…` : "";
    }

    const status = [];
    if (this.draftNotice) {
      status.push(`<div class="conv-wait"><b>Draft discarded.</b> The agent posted a new round, `
        + `so the canvas starts fresh.</div>`);
    }
    if (this.meta?.status === "submitted") {
      if (!this.meta.seen_by_agent_at) {
        status.push(`<div class="conv-wait"><b>Waiting for an agent.</b> Nobody has read this yet — `
          + `submitted ${escapeHtml(relativeTime(this.meta.submitted_at))}. An agent must call `
          + `<code>get_feedback</code> or <code>wait_for_feedback</code>.</div>`);
      } else if (!agentWorking(this.meta)) {
        status.push(`<div class="conv-wait quiet"><b>Agent received this.</b> `
          + `${escapeHtml(relativeTime(this.meta.seen_by_agent_at))}.</div>`);
      }
    }
    if (!items.length && !status.length) {
      log.innerHTML = `<div class="inspector-empty">No messages yet.<br>Write a prompt below, or just annotate and submit.</div>`;
      return;
    }
    const rendered = items.map((item) => {
      const when = clockTime(item.at);
      if (item.kind === "event") {
        return `<div class="conv-event"><span class="conv-dot"></span><span>${escapeHtml(item.text)}</span>`
          + `<time>${escapeHtml(when)}</time></div>`;
      }
      const who = item.kind === "human" ? "you" : "agent";
      return `<div class="conv-msg ${item.kind}">
        <div class="conv-meta">${who}${when ? ` · ${escapeHtml(when)}` : ""}</div>
        <div class="conv-text">${escapeHtml(item.text)}</div>
      </div>`;
    }).join("");
    const html = rendered + status.join("") + this.runSummary();
    const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 40;
    this.convCount = items.length + status.length;
    // Only touch the DOM when something changed, and only follow the tail when the reader is
    // already there — otherwise a 5 s poll yanks them back down mid-sentence.
    if (log.innerHTML !== html) {
      log.innerHTML = html;
      if (atBottom) log.scrollTop = log.scrollHeight;
    }
    if (atBottom) {
      this.seenCount = this.convCount;
      this.setNewBadge(0);
    } else if (this.convCount > this.seenCount) {
      this.setNewBadge(this.convCount - this.seenCount);
    }
  }

  setNewBadge(count) {
    const badge = $("#conv-new");
    if (!badge) return;
    badge.hidden = count <= 0;
    badge.textContent = count === 1 ? "↓ 1 new message" : `↓ ${count} new messages`;
  }

  seenConversation() {
    const log = $("#conv-log");
    if (log) log.scrollTop = log.scrollHeight;
    this.seenCount = this.convCount;
    this.setNewBadge(0);
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

  /// STOP: take the submission back. The agent is expected to die (the bridge watches the
  /// status); an unread round is withdrawn from history by the server.
  async stop() {
    if (this.meta?.status !== "submitted") return;
    if (!confirm("Stop the agent and edit this review?")) return;
    const button = $("#submit");
    button.disabled = true;
    button.textContent = "Stopping…";
    try {
      const result = await api(`/api/sessions/${encodeURIComponent(this.sessionId)}/cancel`, {
        method: "POST",
      });
      if (result?.meta) this.meta = result.meta;
      button.disabled = false;
      this.renderHeader();
      this.renderInspector();
      this.loadHistory();
      this.loadConversation({ quiet: true });
    } catch (error) {
      button.disabled = false;
      this.renderHeader();
      alert(`Could not stop: ${error.message}`);
    }
  }

  /// Unsaved canvas work must survive switching sessions; a submitted round must not.
  saveDraft() {
    const id = this.sessionId;
    if (!id || this.viewing) return;
    const composer = $("#composer-text");
    const prompt = composer ? composer.value.trim() : "";
    const changed = JSON.stringify(this.annotations) !== (this.serverAnnotations || "[]");
    if (!changed && !prompt) {
      this.drafts.delete(id);
    } else {
      this.drafts.set(id, {
        annotations: JSON.parse(JSON.stringify(this.annotations)),
        prompt,
      });
    }
    this.syncDirty();
  }

  /// No Tauri globals in the webview (a remote server must not get them): the desktop shell reads
  /// the dirty flag off the window title instead.
  syncDirty() {
    this.applyTitle();
  }

  applyTitle() {
    const base = `${this.meta?.title || this.meta?.id || "RedLine"} · RedLine`;
    document.title = (this.drafts.size ? "• " : "") + base;
  }

  // ---------------------------------------------------------- submit

  async submit() {
    if (this.meta?.status === "submitted") return this.stop();
    if (!this.image) return;
    const button = $("#submit");
    const label = button.textContent;
    button.disabled = true;
    button.textContent = "Submitting…";
    try {
      const exportCanvas = document.createElement("canvas");
      exportCanvas.width = this.mediaWidth();
      exportCanvas.height = this.mediaHeight();
      const ctx = exportCanvas.getContext("2d");
      ctx.drawImage(this.image, 0, 0);
      for (const annotation of this.annotations) this.paint(ctx, annotation, false);
      this.annotations.forEach((annotation, index) => {
        if (annotation.type !== "text") this.paintBadge(ctx, annotation, index + 1, 14, 2);
      });

      const composer = $("#composer-text");
      const payload = {
        annotations: this.annotations,
        width: this.mediaWidth(),
        height: this.mediaHeight(),
        png: exportCanvas.toDataURL("image/png"),
        note: composer ? composer.value.trim() : "",
      };
      const result = await api(`/api/sessions/${encodeURIComponent(this.sessionId)}/annotations`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload),
      });
      if (result?.meta) this.meta = result.meta;
      this.drafts.delete(this.sessionId);
      this.syncDirty();
      this.serverAnnotations = JSON.stringify(this.annotations);
      if (composer) composer.value = "";
      this.renderHeader();
      this.renderInspector();
      // The server stamped `prompt` and `submitted_at`; show them now instead of waiting for the
      // 5 s conversation poll (otherwise a submit briefly reads "No messages yet").
      this.renderConversation();
      if ($("#history-block")?.open) this.loadHistory();
      await this.loadConversation({ quiet: true });
      this.seenConversation();
      // Hand the keyboard back to the canvas: the review is sent, the next key is a shortcut.
      document.activeElement?.blur?.();
      await this.rail.load();
      button.textContent = "Submitted ✓";
      // Re-render instead of hardcoding the next label: the status is `submitted` now, so the
      // button must become the red STOP with its square, not "Submit again".
      setTimeout(() => {
        button.disabled = false;
        this.renderHeader();
      }, 1400);
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
    const fresh = $("#f-project-new");
    if (fresh) { fresh.value = ""; fresh.hidden = true; }
    const branch = $("#f-branch");
    if (branch) { branch.value = ""; this.suggestBranch(); }
    const project = $("#f-project");
    if (project?.value === "__new__") project.value = "";
    this.fillProjects();
    $("#f-title").focus();
  },
  /// Show what the branch will be called when the field stays empty: a review belongs to a
  /// feature, so the placeholder is `feat/<title>` — never the session id.
  suggestBranch() {
    const input = $("#f-branch");
    if (!input) return;
    const title = ($("#f-title")?.value || "").trim();
    const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 48);
    input.placeholder = `feat/${slug || "my-feature"}`;
  },
  /// Repositories come from the agent sync; older label-only projects stay selectable while they
  /// still have reviews, but empty leftovers are not offered.
  async fillProjects() {
    const select = $("#f-project");
    if (!select) return;
    const previous = select.value;
    try {
      const { projects } = await api("/api/projects");
      const items = projects || [];
      const repos = items.filter((project) => project.repo);
      const others = items.filter(
        (project) => !project.repo && (project.sessions > 0 || project.slug === "inbox"),
      );
      if (!repos.length && !others.length) {
        others.push({ slug: "inbox", name: "Inbox", sessions: 0 });
      }
      const options = ['<option value="" disabled>Choose a project…</option>'];
      if (repos.length) {
        options.push('<optgroup label="Repositories">');
        for (const project of repos) {
          const label = project.repo && project.repo !== project.name
            ? `${project.name} — ${project.repo}`
            : project.name;
          options.push(`<option value="${escapeHtml(project.slug)}">${escapeHtml(label)}</option>`);
        }
        options.push("</optgroup>");
      }
      if (others.length) {
        options.push('<optgroup label="Other projects">');
        for (const project of others) {
          options.push(`<option value="${escapeHtml(project.slug)}">${escapeHtml(project.name)}</option>`);
        }
        options.push("</optgroup>");
      }
      options.push('<option value="__new__">+ New project…</option>');
      select.innerHTML = options.join("");
      const wanted = lastProject.get() || previous;
      select.value = [...select.options].some((option) => option.value === wanted)
        ? wanted
        : "";
      this.syncProjectInput();
    } catch { /* signed out; keep the placeholder and Inbox fallback */ }
  },
  syncProjectInput() {
    const select = $("#f-project");
    const input = $("#f-project-new");
    if (!select || !input) return;
    const creating = select.value === "__new__";
    input.hidden = !creating;
    if (creating) input.focus();
    else lastProject.remember(select.value);
  },
  selectedProject() {
    const select = $("#f-project");
    if (!select) return "";
    if (select.value === "__new__") return ($("#f-project-new")?.value || "").trim();
    return select.value.trim();
  },
  close() { const overlay = $("#overlay"); if (overlay) overlay.hidden = true; },
  wire() {
    const overlay = $("#overlay");
    if (!overlay) return;
    const drop = $("#drop");
    const file = $("#file");
    const preview = $("#drop-preview");
    const videoPreview = $("#drop-video-preview");
    const label = $("#drop-label");
    let picked = null;
    let pickedKind = null;
    let previewUrl = null;
    const clearPreview = () => {
      if (previewUrl) URL.revokeObjectURL(previewUrl);
      previewUrl = null;
      preview.hidden = true;
      preview.removeAttribute("src");
      if (videoPreview) {
        videoPreview.pause();
        videoPreview.hidden = true;
        videoPreview.removeAttribute("src");
      }
    };
    const take = (candidate) => {
      const kind = candidate?.type.startsWith("video/")
        ? "video"
        : candidate?.type.startsWith("image/") ? "image" : null;
      if (!kind) {
        $("#new-error").textContent = "That is not an image or video.";
        return;
      }
      picked = candidate;
      pickedKind = kind;
      clearPreview();
      previewUrl = URL.createObjectURL(candidate);
      const name = candidate.name || (kind === "video" ? "pasted video" : "pasted image");
      const size = `${Math.round(candidate.size / 1024 / 1024 * 10) / 10} MB`;
      if (kind === "video") {
        if (videoPreview) {
          videoPreview.src = previewUrl;
          videoPreview.hidden = false;
          videoPreview.addEventListener("loadedmetadata", () => {
            if (videoPreview.videoWidth > 1920 && !label.textContent.includes("4K")) {
              label.textContent += " · 4K — 1080p recommended";
            }
          }, { once: true });
        }
        drop.classList.add("has-image");
        label.textContent = `${name} — ${size} · video · click to replace`;
      } else {
        preview.src = previewUrl;
        preview.hidden = false;
        drop.classList.add("has-image");
        label.textContent = `${name} — ${Math.round(candidate.size / 1024)} KB · click to replace`;
      }
      const title = $("#f-title");
      if (!title.value) {
        const stem = name.replace(/\.[^.]+$/, "");
        title.value = !stem || stem === "image"
          ? (kind === "video" ? "Pasted video" : "Pasted screenshot")
          : stem;
      }
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
    addEventListener("paste", (event) => {
      const items = [...(event.clipboardData?.items || [])];
      const media = items.find((item) =>
        item.type.startsWith("image/") || item.type.startsWith("video/"));
      const file = media?.getAsFile?.();
      if (!file) return;
      event.preventDefault();
      if (overlay.hidden) NewReview.open();
      take(file);
      $("#new-error").textContent = "Pasted from the clipboard — add a title, then create.";
    });
    addEventListener("dragover", (event) => event.preventDefault());
    addEventListener("drop", (event) => {
      if (!event.dataTransfer.files.length) return;
      event.preventDefault();
      if (overlay.hidden) NewReview.open();
      take(event.dataTransfer.files[0]);
    });
    /// Read duration/dimensions and render the first frame as the poster before uploading.
    const videoDetails = async () => {
      let video = videoPreview;
      let owned = false;
      if (!video || !video.src) {
        video = document.createElement("video");
        video.muted = true;
        video.src = URL.createObjectURL(picked);
        owned = true;
      }
      if (!video.videoWidth || video.readyState < 2) {
        await new Promise((resolve, reject) => {
          video.onloadeddata = resolve;
          video.onerror = () => reject(new Error("Could not read this video."));
          video.load();
        });
      }
      const duration = await resolvedDuration(video);
      if (duration > 300) {
        throw new Error("Video is longer than the 5 minute review limit. Trim the clip first.");
      }
      if (video.currentTime !== 0) {
        await new Promise((resolve) => {
          video.onseeked = resolve;
          video.currentTime = 0;
        });
      }
      const canvas = document.createElement("canvas");
      canvas.width = video.videoWidth;
      canvas.height = video.videoHeight;
      canvas.getContext("2d").drawImage(video, 0, 0);
      const poster = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
      const details = {
        width: video.videoWidth,
        height: video.videoHeight,
        duration,
        poster,
      };
      if (owned) URL.revokeObjectURL(video.src);
      return details;
    };
    $("#new-cancel").addEventListener("click", () => NewReview.close());
    $("#new-close")?.addEventListener("click", () => NewReview.close());
    $("#f-title")?.addEventListener("input", () => NewReview.suggestBranch());
    $("#f-project")?.addEventListener("change", () => NewReview.syncProjectInput());
    $("#new-form").addEventListener("submit", async (event) => {
      event.preventDefault();
      const pageUrl = ($("#f-url")?.value || "").trim();
      if (!picked && !pageUrl) {
        $("#new-error").textContent = "Add a screenshot, or paste a page URL to capture.";
        return;
      }
      const project = NewReview.selectedProject();
      if (!project) {
        $("#new-error").textContent = "Choose a project, or name the new one.";
        return;
      }
      const submit = $("#new-submit");
      submit.disabled = true;
      const branch = ($("#f-branch")?.value || "").trim() || undefined;
      if (!picked) {
        $("#new-error").textContent = "Capturing… (a few seconds)";
        try {
          const body = await api("/api/capture", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              url: pageUrl,
              title: $("#f-title").value.trim() || pageUrl,
              note: $("#f-note").value.trim(),
              viewport: "1440x900",
              agent: "human",
              project: project || undefined,
              branch,
            }),
          });
          lastProject.remember(body.meta.project);
          location.href = `/s/${encodeURIComponent(body.meta.id)}`;
        } catch (error) {
          $("#new-error").textContent = error.message;
          submit.disabled = false;
        }
        return;
      }
      $("#new-error").textContent = "Uploading…";
      if (pickedKind === "video") {
        try {
          if (picked.size > 250 * 1024 * 1024) {
            throw new Error("Video is larger than the 250 MB limit.");
          }
          const details = await videoDetails();
          const form = new FormData();
          form.append("video", picked, picked.name || "video.mp4");
          if (details.poster) form.append("poster", details.poster, "poster.png");
          form.append("title", $("#f-title").value.trim() || picked.name || "Video review");
          form.append("url", pageUrl);
          form.append("note", $("#f-note").value.trim());
          form.append("project", project);
          form.append("agent", "human");
          if (details.duration > 0) {
            form.append("duration_ms", String(Math.round(details.duration * 1000)));
          }
          form.append("width", String(details.width));
          form.append("height", String(details.height));
          if (branch) form.append("branch", branch);
          const body = await api("/api/sessions/video", { method: "POST", body: form });
          lastProject.remember(body.meta.project);
          location.href = `/s/${encodeURIComponent(body.meta.id)}`;
        } catch (error) {
          $("#new-error").textContent = error.message;
          submit.disabled = false;
        }
        return;
      }
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
            url: pageUrl,
            note: $("#f-note").value.trim(),
            image_base64: dataUrl,
            agent: "human",
            project: project || undefined,
            branch,
          }),
        });
        lastProject.remember(body.meta.project);
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

/// The start page is a gallery: the last few reviews with their screenshots. A decided review
/// (confirmed or cancelled) is finished work, not something to resume, so it does not belong
/// here — the rail's All/Done filters are where history lives.
function renderGallery(rail) {
  const box = $("#gallery");
  if (!box) return;
  const sessions = (rail.sessions || [])
    .filter((session) => featureState(session) === "pending")
    .slice(0, 12);
  if (!sessions.length) {
    box.hidden = true;
    return;
  }
  const names = new Map((rail.projects || []).map((project) => [project.slug, project.name]));
  box.hidden = false;
  box.innerHTML = sessions.map((session) => {
    const project = names.get(session.project || "inbox") || (session.project || "Inbox");
    const video = session.media?.kind === "video";
    const length = video && session.media?.duration_ms
      ? timecode(session.media.duration_ms / 1000).replace(/\.\d+$/, "")
      : "";
    return `<a class="card" data-session="${escapeHtml(session.id)}"
      href="/s/${encodeURIComponent(session.id)}">
      <img src="/sessions/${encodeURIComponent(session.id)}/image" loading="lazy" alt="" />
      ${video ? `<span class="media-chip">▶${length ? ` ${escapeHtml(length)}` : ""}</span>` : ""}
      <div class="card-body">
        <b>${escapeHtml(session.title || session.id)}</b>
        <span>${escapeHtml(project)} · ${escapeHtml(relativeTime(session.submitted_at || session.created_at))} · ${escapeHtml(
          featureState(session) === "pending" ? session.status : featureState(session),
        )}${session.branch ? ` · ${escapeHtml(session.branch)}` : ""}</span>
      </div>
    </a>`;
  }).join("");
  // An agent-supplied video may have no poster; hide the broken-image glyph instead of showing
  // it on the card.
  $$("img", box).forEach((img) => {
    const mark = () => {
      if (img.complete && img.naturalWidth === 0) {
        img.closest(".card")?.classList.add("no-poster");
      }
    };
    img.addEventListener("error", mark, { once: true });
    mark();
  });
}

async function bootList() {
  bindRailResize();
  const refresh = async () => {
    await rail.load();
    renderGallery(rail);
  };
  const rail = new Rail({
    body: $("#rail-body"),
    chips: $("#chips"),
    search: $("#search"),
    purge: $("#purge-cancelled"),
    onSelect: (id) => { location.href = `/s/${encodeURIComponent(id)}`; },
    onAction: async (action, id) => {
      try {
        const session = rail.sessions.find((s) => s.id === id);
        const title = session?.title || id;
        const result = await featureAction(action, id, title, featureState(session || {}));
        if (result) await refresh();
      } catch (error) {
        alert(`Could not ${action}: ${error.message}`);
      }
    },
    onPurge: async (ids) => {
      for (const id of ids) {
        await api(`/api/sessions/${encodeURIComponent(id)}`, { method: "DELETE" });
      }
      await refresh();
    },
    onBulk: async (action, ids) => {
      if (action === "confirm") {
        const choice = await askFeature({
          title: `Confirm ${ids.length} feature${ids.length === 1 ? "" : "s"}?`,
          body: "Each selected session will be marked as confirmed and closed. Your agent may open a pull request and merge it into the integration branch.",
          actions: [
            { value: "keep", label: "Keep working" },
            { value: "confirm", label: "Confirm features", kind: "primary" },
          ],
        });
        if (choice !== "confirm") return;
      }
      if (action === "delete"
        && !confirm(`Delete ${ids.length} session(s)? Their files go too.`)) return;
      try {
        for (const id of ids) {
          if (action === "delete") {
            await api(`/api/sessions/${encodeURIComponent(id)}`, { method: "DELETE" });
          } else {
            await api(`/api/sessions/${encodeURIComponent(id)}/confirm-feature`, { method: "POST" });
          }
        }
      } catch (error) {
        alert(`Could not ${action}: ${error.message}`);
      }
      rail.exitBulk();
      await refresh();
    },
  });
  await rail.load();
  renderGallery(rail);
  // A dashboard restored from the back-forward cache keeps the DOM it had when the human left it,
  // including gallery cards for reviews that have since been confirmed or cancelled. Refresh it.
  addEventListener("pageshow", (event) => {
    if (event.persisted) refresh();
  });
  NewReview.wire();
  $("#new-review").addEventListener("click", () => NewReview.open());
  const count = $("#sessions-count");
  if (count) count.textContent = plural(rail.sessions.length, "session");
}

/* ------------------------------------------------------------------ boot */

document.addEventListener("DOMContentLoaded", () => {
  if (matchMedia("(pointer: coarse)").matches || navigator.maxTouchPoints > 0) {
    document.body.classList.add("touch");
  }
  Theme.apply(Theme.current());
  $$("[data-theme-toggle]").forEach((button) =>
    button.addEventListener("click", () => Theme.toggle()));
  $$("[data-agent-settings]").forEach((button) =>
    button.addEventListener("click", () => AgentSettings.open()));
  AgentStatus.start();
  window.redlineAgentStatus = AgentStatus;
  $$("#agent-status").forEach((chip) =>
    chip.addEventListener("click", () => ConnectAgent.open()));
  $$("#agent-connect").forEach((button) =>
    button.addEventListener("click", () => ConnectAgent.open()));
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
