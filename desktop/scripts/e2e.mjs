/**
 * End-to-end test: drives the real editor in Chromium and asserts what the server received.
 *
 * Covers the parts that only exist in the browser: pointer→image coordinate mapping under zoom,
 * dragging an existing annotation, inline text entry, and submission.
 *
 * Run against a disposable container (see scripts/e2e.sh):
 *   REDLINE_BASE=http://redline-e2e:8788 REDLINE_TOKEN=e2e-token CHROME_URL=http://redline-e2e:9222 \
 *     node scripts/e2e.mjs
 */
import puppeteer from "puppeteer-core";
import zlib from "node:zlib";

const BASE = process.env.REDLINE_BASE || "http://127.0.0.1:8788";
const TOKEN = process.env.REDLINE_TOKEN || "e2e-token";
const CHROME = process.env.CHROME_URL || "http://127.0.0.1:9222";

/** A real 1440x900 screenshot, so the fitted zoom is a realistic fraction (not 800%). */
function makePng(width = 1440, height = 900) {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc32 = (buf) => {
    let c = 0xffffffff;
    for (const byte of buf) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type, data) => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([length, body, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;  // bit depth
  header[9] = 6;  // RGBA
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y += 1) {
    const row = y * (width * 4 + 1);
    raw[row] = 0;
    for (let x = 0; x < width; x += 1) {
      const p = row + 1 + x * 4;
      const shade = 226 + Math.round(12 * Math.sin(x / 90));
      raw[p] = shade; raw[p + 1] = shade; raw[p + 2] = shade + 6; raw[p + 3] = 255;
    }
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", zlib.deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]).toString("base64");
}

const results = [];
function check(name, condition, detail = "") {
  results.push({ name, ok: Boolean(condition), detail });
  console.log(`  ${condition ? "ok  " : "FAIL"} ${name}${detail ? ` — ${detail}` : ""}`);
}

async function api(path, options = {}) {
  const response = await fetch(`${BASE}${path}`, {
    ...options,
    headers: {
      Authorization: `Bearer ${TOKEN}`,
      "content-type": "application/json",
      ...(options.headers || {}),
    },
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(`${path} → ${response.status} ${JSON.stringify(body)}`);
  return body;
}

async function drag(page, from, to, steps = 12) {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(to.x, to.y, { steps });
  await page.mouse.up();
}

const main = async () => {
  // A real screenshot size, so the fitted zoom is not 1:1 — coordinate mapping must survive it.
  // Submitting locks the canvas now; tests that keep drawing must stop first. Ask the "agent"
  // to read the submission so the round is kept instead of withdrawn.
  const stopReview = async () => {
    await api("/mcp", {
      method: "POST",
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "tools/call",
        params: { name: "get_feedback", arguments: { session_id: id } },
      }),
    });
    // The submit button is briefly disabled ("Submitted ✓") right after a submit.
    await page.waitForFunction(() => {
      const button = document.querySelector("#submit");
      return button && !button.disabled && button.textContent.trim() === "STOP";
    }, { timeout: 5000 });
    page.once("dialog", (dialog) => dialog.accept());
    await page.click("#submit");
    await page.waitForFunction(() => window.redline.meta?.status === "open", { timeout: 10000 });
  };

  const created = await api("/api/sessions", {
    method: "POST",
    body: JSON.stringify({
      title: "e2e canvas",
      image_base64: makePng(),
      project: "E2E",
      branch: "feat/e2e-canvas",
    }),
  });
  const id = created.meta.id;

  // The agent reports repos with portable keys; the UI and sessions use the resulting project.
  const synced = await api("/api/agent/repos", {
    method: "POST",
    body: JSON.stringify({
      agent_id: "e2e",
      repos: [{ key: "e2e-repo", name: "E2E Repo" }],
    }),
  });
  check("agent repo sync creates a project with a repo key",
    synced.projects?.[0]?.repo === "e2e-repo" && synced.projects?.[0]?.agent === "e2e",
    JSON.stringify(synced.projects?.[0] || {}));
  const projects = await api("/api/projects");
  const repoProject = projects.projects.find((project) => project.slug === "e2e-repo");
  check("project list exposes the repo and agent",
    repoProject?.repo === "e2e-repo" && repoProject?.agent === "e2e",
    JSON.stringify(repoProject || {}));

  const traversal = await fetch(`${BASE}/api/agent/repos`, {
    method: "POST",
    headers: { Authorization: `Bearer ${TOKEN}`, "content-type": "application/json" },
    body: JSON.stringify({ agent_id: "e2e", repos: [{ key: "../escape", name: "no" }] }),
  });
  check("agent repo sync rejects path traversal", traversal.status === 400, `status ${traversal.status}`);

  const browser = await puppeteer.connect({
    browserURL: CHROME,
    defaultViewport: { width: 1280, height: 820 },
  });
  const page = await browser.newPage();
  const jsErrors = [];
  page.on("pageerror", (error) => jsErrors.push(error.message));

  await page.goto(`${BASE}/s/${id}?t=${TOKEN}`, { waitUntil: "networkidle2" });
  await page.waitForFunction(() => window.redline && window.redline.image, { timeout: 15000 });

  const view = await page.evaluate(() => ({
    scale: window.redline.view.scale,
    x: window.redline.view.x,
    y: window.redline.view.y,
  }));
  check("editor booted with an image", true, `scale ${view.scale.toFixed(3)}`);

  // ---- the header is a row of labelled icons; status lives in the toast and the rail card
  const chrome = await page.evaluate(() => {
    const header = document.querySelector(".header");
    const buttons = [...header.querySelectorAll(".header-actions .btn.icon")];
    const gear = document.querySelector("[data-agent-settings] svg");
    const gearStyle = gear ? getComputedStyle(gear) : null;
    return {
      pills: header.querySelectorAll(".pill").length,
      stale: Boolean(document.querySelector("#session-status, #agent-state")),
      count: buttons.length,
      svg: buttons.every((button) => Boolean(button.querySelector("svg"))),
      labelled: buttons.every((button) =>
        Boolean((button.title || "").trim() && (button.getAttribute("aria-label") || "").trim())),
      separator: Boolean(header.querySelector(".header-sep")),
      gear: gearStyle ? { fill: gearStyle.fill, stroke: gearStyle.stroke } : null,
      gearPath: gear ? gear.getAttribute("viewBox") : "",
      panelMarks: ["#toggle-rail", "#toggle-inspector"].every((selector) => {
        const mark = document.querySelector(`${selector} svg .panel`);
        return mark && getComputedStyle(mark).fill !== "none";
      }),
    };
  });
  check("the header carries no status pills — the toast and the rail card say it",
    chrome.pills === 0 && !chrome.stale, JSON.stringify(chrome));
  check("every header action is a labelled SVG icon",
    chrome.count === 5 && chrome.svg && chrome.labelled && chrome.separator
      && chrome.panelMarks, JSON.stringify(chrome));
  check("agent settings is a solid gear, not a text glyph",
    chrome.gear && chrome.gear.fill !== "none" && chrome.gear.stroke === "none"
      && chrome.gearPath === "0 0 24 24",
    JSON.stringify(chrome.gear));

  // The two panel toggles click, flip their pressed state, and the canvas bitmap follows the grid.
  const panelToggles = await page.evaluate(() => {
    const app = document.querySelector("#app");
    const rail = document.querySelector("#toggle-rail");
    const inspector = document.querySelector("#toggle-inspector");
    const bitmap = () => document.querySelector("#canvas").width;
    const before = bitmap();
    rail.click();
    const hidden = {
      rail: app.classList.contains("rail-hidden"),
      pressed: rail.getAttribute("aria-pressed"),
      title: rail.title,
      bitmap: bitmap(),
    };
    rail.click();
    const backRail = { pressed: rail.getAttribute("aria-pressed"), bitmap: bitmap() };
    inspector.click();
    const hidInspector = app.classList.contains("inspector-hidden");
    const inspectorPressed = inspector.getAttribute("aria-pressed");
    inspector.click();
    return {
      before, hidden, backRail, hidInspector, inspectorPressed,
      restored: !app.classList.contains("inspector-hidden") && !app.classList.contains("rail-hidden"),
      pressedBack: inspector.getAttribute("aria-pressed"),
    };
  });
  check("the panel icons hide their panel, resize the canvas and say which state is on",
    panelToggles.hidden.rail && panelToggles.hidden.pressed === "false" && panelToggles.hidden.title.startsWith("Show")
      && panelToggles.hidden.bitmap > panelToggles.before
      && panelToggles.backRail.pressed === "true" && panelToggles.backRail.bitmap === panelToggles.before
      && panelToggles.hidInspector && panelToggles.inspectorPressed === "false" && panelToggles.restored
      && panelToggles.pressedBack === "true",
    JSON.stringify(panelToggles));

  // The theme button swaps a moon/sun SVG instead of an emoji glyph.
  const themeIcons = await page.evaluate(() => {
    const button = document.querySelector("[data-theme-toggle]");
    const read = () => ({
      theme: document.documentElement.dataset.theme,
      svg: Boolean(button.querySelector("svg")),
      title: button.title,
      text: button.textContent.trim(),
    });
    const first = read();
    button.click();
    const second = read();
    button.click();
    return { first, second, back: read() };
  });
  check("the theme toggle swaps sun and moon SVGs, not glyphs",
    themeIcons.first.svg && themeIcons.second.svg && themeIcons.first.title !== themeIcons.second.title
      && themeIcons.first.text === "" && themeIcons.second.text === ""
      && themeIcons.first.theme !== themeIcons.second.theme
      && themeIcons.back.theme === themeIcons.first.theme,
    JSON.stringify(themeIcons));

  // Ctrl+V in the page fills the New review dialog from the clipboard.
  await page.evaluate(async () => {
    const canvas = document.createElement("canvas");
    canvas.width = 320;
    canvas.height = 200;
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#2f81f7";
    ctx.fillRect(0, 0, 320, 200);
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
    const data = new DataTransfer();
    data.items.add(new File([blob], "image.png", { type: "image/png" }));
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: data });
    window.dispatchEvent(event);
  });
  await page.waitForFunction(() => !document.querySelector("#overlay").hidden, { timeout: 5000 });
  const pastedTitle = await page.$eval("#f-title", (el) => el.value);
  check("ctrl+v fills the new review dialog", pastedTitle === "Pasted screenshot", pastedTitle);
  const preview = await page.evaluate(() => {
    const img = document.querySelector("#drop-preview");
    if (!img) return null;
    return { hidden: img.hidden, blob: img.src.startsWith("blob:"), w: img.naturalWidth };
  });
  check("pasted image is previewed as a thumbnail",
    preview && !preview.hidden && preview.blob && preview.w === 320, JSON.stringify(preview));

  await page.waitForFunction(
    () => [...document.querySelectorAll("#f-project option")].some((option) => option.value === "e2e-repo"),
    { timeout: 5000 },
  );
  const picker = await page.evaluate(() => {
    const select = document.querySelector("#f-project");
    const group = (label) => [
      ...(select?.querySelectorAll(`optgroup[label="${label}"] option`) || []),
    ].map((option) => option.value);
    return {
      tag: select?.tagName,
      values: [...(select?.options || [])].map((option) => option.value),
      repos: group("Repositories"),
      others: group("Other projects"),
      placeholder: select?.value || "",
      required: select?.required,
      urlHint: document.querySelector("#url-hint")?.textContent.trim() || "",
      capture: Boolean(document.querySelector("#f-capture")),
    };
  });
  check("new review picks a repo project from a grouped select",
    picker.tag === "SELECT" && picker.values.includes("e2e") && picker.values.includes("inbox")
      && picker.values.includes("e2e-repo") && picker.repos.includes("e2e-repo")
      && picker.others.includes("e2e") && picker.values.includes("__new__")
      && picker.required && !picker.capture,
    JSON.stringify(picker));
  check("new review preselects the project of the open review",
    picker.placeholder === "e2e", picker.placeholder);
  check("the page URL field explains capture and metadata",
    picker.urlHint.includes("captures it"), picker.urlHint);

  await page.select("#f-project", "__new__");
  const freshProject = await page.evaluate(() => ({
    visible: !document.querySelector("#f-project-new").hidden,
    focused: document.activeElement === document.querySelector("#f-project-new"),
  }));
  check("choosing a new project reveals its name field",
    freshProject.visible && freshProject.focused, JSON.stringify(freshProject));
  await page.click("#new-cancel");

  await page.waitForSelector(".rail-project", { timeout: 5000 });
  const projectHeader = await page.$eval(".rail-project .pname", (el) => el.textContent.trim());
  check("rail groups sessions by project", projectHeader === "E2E", `header=${projectHeader}`);

  const branch = await page
    .$eval('.session[aria-current="true"] .sub .branch', (el) => el.textContent.trim())
    .catch(() => "");
  check("the rail shows the feature branch next to the time", branch === "feat/e2e-canvas", branch);

  // ---- agent concurrency settings are server-side and editable from the UI
  await page.click("[data-agent-settings]");
  await page.waitForSelector("#agent-settings-dialog[open]", { timeout: 5000 });
  const settingsBefore = await page.evaluate(() => ({
    global: document.querySelector("#agent-max-parallel").value,
    project: document.querySelector("#agent-project-max-parallel").value,
    lease: document.querySelector("#agent-lease-seconds").value,
    hasE2E: Boolean(document.querySelector('#agent-project-list input[data-project="e2e"]')),
  }));
  check("agent settings open with the server defaults",
    settingsBefore.global === "2" && settingsBefore.project === "1"
      && settingsBefore.lease === "120" && settingsBefore.hasE2E,
    JSON.stringify(settingsBefore));
  await page.$eval("#agent-max-parallel", (input) => { input.value = "3"; });
  await page.$eval("#agent-project-max-parallel", (input) => { input.value = "2"; });
  await page.$eval('#agent-project-list input[data-project="e2e"]', (input) => { input.value = "2"; });
  await page.click("#agent-settings-save");
  await page.waitForFunction(
    () => !document.querySelector("#agent-settings-dialog")?.open,
    { timeout: 10000 },
  );
  const savedSettings = await api("/api/agent/settings");
  const e2eProject = (savedSettings.projects || []).find((project) => project.slug === "e2e");
  check("agent settings save globally and per project",
    savedSettings.max_parallel === 3 && savedSettings.project_max_parallel === 2
      && e2eProject?.max_parallel === 2 && e2eProject?.effective_max_parallel === 2,
    JSON.stringify({
      max: savedSettings.max_parallel,
      project: savedSettings.project_max_parallel,
      e2e: e2eProject,
    }));

  const canvas = await page.$eval("#canvas", (el) => {
    const rect = el.getBoundingClientRect();
    return { x: rect.x, y: rect.y, w: rect.width, h: rect.height };
  });
  const at = (dx, dy) => ({ x: canvas.x + dx, y: canvas.y + dy });

  /** Open a fresh session so a tool test cannot inherit another tool's annotations. */
  const freshSession = async (title) => {
    const made = await api("/api/sessions", {
      method: "POST",
      body: JSON.stringify({ title, image_base64: makePng(), project: "E2E" }),
    });
    const sessionId = made.meta.id;
    await page.evaluate((id) => window.redline.openSession(id), sessionId);
    await page.waitForFunction(
      (id) => window.redline.sessionId === id
        && Boolean(window.redline.image) && window.redline.view.fitted,
      { timeout: 10000 },
      sessionId,
    );
    return sessionId;
  };

  /** The row, number, label and painted badge for the last annotation in a fresh session. */
  const readLastAnnotation = async () => page.evaluate(() => {
    const editor = window.redline;
    const index = editor.annotations.length - 1;
    const annotation = editor.annotations[index];
    const row = document.querySelector(`#ann-list .ann-row[data-index="${index}"]`);
    const anchor = annotation?.start || annotation?.points?.[0] || { x: 0, y: 0 };
    const inset = 2 / editor.view.scale;
    const radius = 12 / editor.view.scale;
    const x = Math.round((editor.view.x + (anchor.x + inset + radius * 0.7) * editor.view.scale) * editor.dpr);
    const y = Math.round((editor.view.y + (anchor.y + inset) * editor.view.scale) * editor.dpr);
    const pixel = document.querySelector("#canvas").getContext("2d").getImageData(x, y, 1, 1).data;
    return {
      type: annotation?.type,
      count: editor.annotations.length,
      rows: document.querySelectorAll("#ann-list .ann-row").length,
      number: row?.querySelector(".ann-num")?.textContent.trim() || "",
      label: row?.querySelector(".label b")?.textContent.trim() || "",
      pixel: [pixel[0], pixel[1], pixel[2], pixel[3]],
    };
  });

  // ---- tools must respond to clicks (they sit inside the stage, which captures pointers)
  for (const tool of ["rect", "ellipse", "arrow", "pen", "select"]) {
    await page.click(`.tool[data-tool="${tool}"]`);
    const state = await page.evaluate(() => ({
      tool: window.redline.tool,
      marquee: Boolean(window.redline.marquee),
    }));
    check(`clicking the ${tool} tool selects it`, state.tool === tool && !state.marquee,
      `${state.tool}${state.marquee ? " + marquee started" : ""}`);
  }

  // ---- draw a rectangle
  await page.keyboard.press("r");
  await drag(page, at(200, 160), at(420, 300));
  let state = await page.evaluate(() => window.redline.annotations);
  check("rectangle drawn", state.length === 1 && state[0].type === "rect", JSON.stringify(state[0] || null));

  const rect = state[0];
  const expected = {
    x: (200 - view.x) / view.scale,
    y: (160 - view.y) / view.scale,
  };
  const tolerance = 1.5 + 2 / view.scale;
  check(
    "coordinates account for the fitted zoom",
    Math.abs(rect.start.x - expected.x) < tolerance && Math.abs(rect.start.y - expected.y) < tolerance,
    `got ${rect.start.x},${rect.start.y} expected ~${expected.x.toFixed(1)},${expected.y.toFixed(1)}`,
  );
  check(
    "coordinates are whole pixels",
    Number.isInteger(rect.start.x) && Number.isInteger(rect.end.y),
    `${rect.start.x}, ${rect.end.y}`,
  );

  // ---- move it with the select tool
  // Drawing focuses the note field on the next frame; wait for it, then hand the keyboard back.
  await page
    .waitForFunction(() => document.activeElement?.id === "note-text", { timeout: 2000 })
    .catch(() => {});
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press("v");
  const centre = at(310, 230);
  await drag(page, centre, { x: centre.x + 60, y: centre.y + 40 });
  state = await page.evaluate(() => window.redline.annotations);
  const moved = state[0];
  const dx = (moved.start.x - rect.start.x) * view.scale;
  const dy = (moved.start.y - rect.start.y) * view.scale;
  check(
    "dragging moves the annotation by the pointer delta",
    Math.abs(dx - 60) < tolerance * view.scale && Math.abs(dy - 40) < tolerance * view.scale,
    `moved ${dx.toFixed(1)},${dy.toFixed(1)} screen px`,
  );

  // ---- undo restores the old position
  await page.keyboard.down("Control");
  await page.keyboard.press("z");
  await page.keyboard.up("Control");
  state = await page.evaluate(() => window.redline.annotations);
  check(
    "ctrl+z restores the previous geometry",
    Math.abs(state[0].start.x - rect.start.x) < 0.001,
    `${state[0].start.x} vs ${rect.start.x}`,
  );

  // ---- the note lives in the panel, attached to the shape
  await page.keyboard.press("v");
  await page.mouse.click(centre.x, centre.y);
  await page.waitForFunction(
    () => !document.querySelector("#note-editor").classList.contains("hidden"),
    { timeout: 5000 },
  );
  await page.click("#note-text");
  await page.type("#note-text", "Toto tlačítko by mělo být zelené");
  state = await page.evaluate(() => window.redline.annotations);
  const noted = state[state.length - 1];
  check("note attached to the drawn shape", noted?.note?.startsWith("Toto"), noted?.note || "");

  // ---- submit with a prompt from the composer
  await page.click("#composer-text");
  await page.type("#composer-text", "Make the button green and bump the contrast");
  await page.keyboard.down("Control");
  await page.keyboard.press("Enter");
  await page.keyboard.up("Control");
  await page.waitForFunction(
    () => document.querySelector("#submit")?.textContent.includes("STOP")
      && window.redline.meta?.status === "submitted",
    { timeout: 15000 },
  );

  const saved = await api(`/api/sessions/${id}`);
  const stored = saved.annotations.annotations;
  check("server stored the shape", stored.length === 1, `${stored.length}: ${stored.map((a) => a.type).join(",")}`);
  check("rect survived the round-trip", stored.some((a) => a.type === "rect"));
  check("note survived with its diacritics", stored.some((a) => a.note?.includes("zelené")));
  check("screenshot was composited", saved.meta.status === "submitted");
  check("prompt travelled with the submission", saved.meta.prompt === "Make the button green and bump the contrast",
    String(saved.meta.prompt));
  await page.waitForFunction(
    () => (document.querySelector("#conv-log")?.textContent || "").includes("Make the button green"),
    { timeout: 5000 },
  );
  const conversation = await page.evaluate(() => document.querySelector("#conv-log")?.textContent || "");
  check("prompt appears in the conversation", conversation.includes("Make the button green"),
    conversation.slice(0, 80));
  const waiting = await page.evaluate(() => document.querySelector(".conv-wait")?.textContent || "");
  check("submission says it is waiting for an agent", waiting.includes("Waiting for an agent"),
    waiting.slice(0, 60));

  // ---- the rail says the same thing. A submitted round is the agent's ball; it used to read
  // "new" (unread by the human), which said nothing about who was expected to act next.
  // `submit()` updates the header before its rail reload finishes; wait for the rail itself
  // rather than re-rendering the previous session list.
  await page.evaluate(() => window.redline.rail.load());
  const queuedRail = await page.evaluate(() => {
    const row = document.querySelector('.session[aria-current="true"]');
    return {
      badge: row?.querySelector(".badge.waiting")?.textContent.trim() || "",
      unread: Boolean(row?.querySelector(".badge.new")),
      pwait: Boolean(document.querySelector(".rail-project .pwait")),
      counts: document.querySelectorAll(".rail-project .pcount").length,
    };
  });
  check("the rail says a submitted round is waiting for an agent",
    queuedRail.badge === "waiting for agent" && !queuedRail.unread, JSON.stringify(queuedRail));
  check("the project header keeps one count, not a waiting count beside it",
    !queuedRail.pwait && queuedRail.counts > 0, JSON.stringify(queuedRail));

  // ---- the status toast sits in the middle of the stage, clear of the toolbar
  const toastBox = await page.evaluate(() => {
    const toast = document.querySelector("#stage-status");
    const bar = document.querySelector(".options");
    const stage = document.querySelector("#stage");
    if (!toast || toast.hidden || !bar || !stage) return null;
    const a = toast.getBoundingClientRect();
    const b = bar.getBoundingClientRect();
    const s = stage.getBoundingClientRect();
    return {
      overlap: a.top < b.bottom && a.bottom > b.top && a.left < b.right && a.right > b.left,
      offCentre: Math.abs((a.left + a.right) / 2 - (s.left + s.right) / 2),
      offMiddle: Math.abs((a.top + a.bottom) / 2 - (s.top + s.bottom) / 2),
      toastTop: Math.round(a.top),
      barBottom: Math.round(b.bottom),
    };
  });
  check("status toast sits below the toolbar", toastBox && !toastBox.overlap,
    toastBox ? `toast top ${toastBox.toastTop} · toolbar bottom ${toastBox.barBottom}` : "no toast");
  check("status toast is centred in the stage where the old lock modal was",
    toastBox && toastBox.offCentre <= 2 && toastBox.offMiddle <= 2,
    toastBox ? `off centre ${Math.round(toastBox.offCentre)} · off middle ${Math.round(toastBox.offMiddle)}` : "no toast");
  const noModal = await page.evaluate(() => !document.querySelector("#review-lock"));
  check("the submitted lock is a status toast, not a dimming modal", noModal);

  // ---- a fresh load of a submitted session must not die before the event poll lands
  await page.reload({ waitUntil: "networkidle0" });
  let reloaded = { stage: "", status: "" };
  for (let attempt = 0; attempt < 40; attempt += 1) {
    reloaded = await page.evaluate(() => ({
      stage: document.querySelector("#stage-empty")?.textContent || "",
      status: document.querySelector("#stage-status")?.textContent || "",
    }));
    if (reloaded.stage || reloaded.status) break;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  check("a submitted session survives a page reload", !reloaded.stage.includes("Could not load")
    && reloaded.status.length > 0, (reloaded.stage || reloaded.status).slice(0, 70));

  // ---- the conversation clock is the viewer's wall clock, not the server's UTC
  await page.emulateTimezone("Europe/Prague");
  await page.evaluate(() => window.redline.renderConversation());
  const clocks = await page.evaluate(() => {
    const iso = window.redline.meta?.submitted_at || "";
    const shown = document.querySelector("#conv-log .conv-msg.human .conv-meta")?.textContent || "";
    const local = new Date(iso).toLocaleTimeString(undefined,
      { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
    return { shown, local, utc: `${iso.slice(11, 13)}:${iso.slice(14, 16)}` };
  });
  check("conversation clocks use the viewer's timezone",
    clocks.local !== clocks.utc && clocks.shown.includes(clocks.local),
    `shown ${clocks.shown.trim()} · local ${clocks.local} · utc ${clocks.utc}`);
  await page.emulateTimezone(null);
  const composerEmpty = await page.$eval("#composer-text", (el) => el.value === "");
  check("composer clears after submit", composerEmpty);

  // ---- the conversation carries a live run summary, like the agent's own chat ("Working —
  // used 2 tools: Edit and Shell"), so the human sees the agent's work as it happens.
  for (const [message, tool] of [["Reading the annotations", "edit"], ["Running the e2e", "shell"]]) {
    await api("/mcp", {
      method: "POST",
      body: JSON.stringify({
        jsonrpc: "2.0", id: 7, method: "tools/call",
        params: { name: "progress", arguments: { session_id: id, message, state: "tool", tool } },
      }),
    });
  }
  await page.evaluate(() => window.redline.loadConversation({ quiet: true }));
  const runCard = await page.evaluate(() => {
    const card = document.querySelector("#conv-log .conv-run");
    return card ? { text: card.textContent, working: card.classList.contains("working") } : null;
  });
  check("conversation summarises the running agent",
    runCard?.working && runCard.text.includes("Working")
      && runCard.text.includes("Used 2 tools: Edit and Shell"),
    runCard ? runCard.text : "no run summary");

  // ---- the rail says which review and which project an agent is working on, without opening it
  await page.evaluate(() => window.redline.rail.refreshAgentState());
  const busyRail = await page.evaluate(() => {
    const row = document.querySelector(".session.working");
    return {
      row: row ? row.textContent : "",
      badge: Boolean(row?.querySelector(".badge.work .spin")),
      project: Boolean(document.querySelector(".rail-project .pworking .spin")),
    };
  });
  check("the rail marks the review the agent is working on",
    busyRail.badge && busyRail.row.includes("working"), busyRail.row.trim());
  check("the rail marks the whole project as busy", busyRail.project);

  // A resolved/cancelled run must lose the indicator again; a spinner that never stops is worse
  // than none, because it reads as an agent stuck on the task.
  await api("/mcp", {
    method: "POST",
    body: JSON.stringify({
      jsonrpc: "2.0", id: 8, method: "tools/call",
      params: { name: "reply", arguments: { session_id: id, message: "Done for now" } },
    }),
  });
  await page.evaluate(() => window.redline.rail.refreshAgentState());
  const idleRail = await page.evaluate(() => ({
    row: Boolean(document.querySelector(".session.working")),
    project: Boolean(document.querySelector(".rail-project .pworking")),
    badge: document.querySelector('.session[aria-current="true"] .badge')?.textContent.trim() || "",
  }));
  check("the working indicator clears when the agent replies",
    !idleRail.row && !idleRail.project, JSON.stringify(idleRail));
  check("the rail says replied, not waiting for agent, on an answered open round",
    idleRail.badge === "replied", idleRail.badge);

  // `reply` is a message, not the finish line: the round is still submitted and STOP is still
  // armed, so the card must not claim "Run finished". It says the agent answered and the run has
  // not finished, with the age of the last step so an idle agent is distinguishable from a busy one.
  await page.evaluate(() => window.redline.loadConversation({ quiet: true }));
  const repliedCard = await page.evaluate(() => {
    const card = document.querySelector("#conv-log .conv-run");
    return {
      working: Boolean(card?.classList.contains("working")),
      pending: Boolean(card?.classList.contains("pending")),
      text: card?.textContent || "",
      status: document.querySelector("#stage-status")?.textContent || "",
      stop: document.querySelector("#submit")?.textContent || "",
    };
  });
  check("an answered round that is still submitted does not claim to be finished",
    !repliedCard.working && repliedCard.pending
      && repliedCard.text.includes("Agent replied") && !repliedCard.text.includes("Run finished")
      && repliedCard.text.includes("last step") && repliedCard.stop.includes("STOP"),
    `${repliedCard.text}${repliedCard.stop}`);
  check("the status line does not claim a working agent on an answered round",
    !repliedCard.status.includes("Agent is working") && repliedCard.status.includes("replied"),
    repliedCard.status);

  // The run can resume after the reply (it did, for minutes); a new progress line is evidence
  // that the agent is working again, so the indicator comes back while the round is submitted.
  await api("/mcp", {
    method: "POST",
    body: JSON.stringify({
      jsonrpc: "2.0", id: 9, method: "tools/call",
      params: { name: "progress", arguments: {
        session_id: id, message: "Resumed after the answer", state: "tool", tool: "shell",
      } },
    }),
  });
  await page.evaluate(() => window.redline.loadConversation({ quiet: true }));
  await page.evaluate(() => window.redline.rail.refreshAgentState());
  const resumedCard = await page.evaluate(() => {
    const card = document.querySelector("#conv-log .conv-run");
    return {
      working: Boolean(card?.classList.contains("working")),
      text: card?.textContent || "",
      rail: Boolean(document.querySelector(".session.working")),
    };
  });
  check("a step after the reply revives the working indicator while the round is open",
    resumedCard.working && resumedCard.text.includes("Working") && resumedCard.rail,
    `${resumedCard.text} · rail ${resumedCard.rail}`);
  if (process.env.E2E_SHOT) await page.screenshot({ path: process.env.E2E_SHOT });

  // Those checks need the submitted state; now unlock and keep drawing.
  await stopReview();

  // ---- multi-select with shift-click, then align left
  await page.keyboard.press("r");
  await drag(page, at(620, 430), at(780, 540));
  await page.keyboard.press("v");
  const firstRect = at(310, 230); // centre of the first rectangle
  await page.mouse.click(firstRect.x, firstRect.y);
  await page.keyboard.down("Shift");
  const second = at(700, 485);
  await page.mouse.click(second.x, second.y);
  await page.keyboard.up("Shift");
  const selected = await page.evaluate(() => window.redline.selected.size);
  check("shift-click builds a multi-selection", selected === 2, `${selected} selected`);

  const alignVisible = await page.$eval('[data-align="left"]', (el) => !el.classList.contains("hidden"));
  check("align controls appear for a multi-selection", alignVisible);
  await page.click('[data-align="left"]');

  const rects = await page.evaluate(() =>
    window.redline.annotations.map((a) => (a.type === "rect" ? a.start.x : null)).filter((x) => x !== null));
  check(
    "align left makes both rectangles share an edge",
    rects.length === 2 && Math.abs(rects[0] - rects[1]) <= 1,
    JSON.stringify(rects),
  );

  // ---- resubmit creates history
  await page.keyboard.down("Control");
  await page.keyboard.press("Enter");
  await page.keyboard.up("Control");
  let history = { submissions: [] };
  for (let attempt = 0; attempt < 24; attempt += 1) {
    history = await api(`/api/sessions/${id}/history`);
    if (history.submissions.length) break;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  check("a second submission archives the first round", history.submissions.length >= 1,
    `${history.submissions.length} archived`);

  // ---- history opens a previous round read-only, and Current brings the live canvas back
  if (!(await page.$eval("#history-block", (el) => el.open))) {
    await page.click("#history-block > summary");
  }
  // The block may already have been open from an earlier step, in which case toggling it does
  // not fetch; load explicitly instead of racing the submit's background refresh.
  await page.evaluate(() => window.redline.loadHistory());
  await page.waitForFunction(
    () => document.querySelectorAll("#history-list [data-round]").length > 0,
    { timeout: 5000 },
  );
  const opened = await page.evaluate(() => {
    const rows = [...document.querySelectorAll("#history-list [data-round]")];
    const target = rows.find((row) => row.dataset.round !== "current") || rows[0];
    target?.click();
    return Boolean(target);
  });
  check("history lists at least one round", opened);
  await page.waitForFunction(() => Boolean(window.redline.viewing), { timeout: 5000 });
  const viewing = await page.evaluate(() => ({
    round: window.redline.viewing.round,
    readonly: window.redline.annotations === window.redline.viewing.annotations,
    bar: !document.querySelector("#viewing-bar").hidden,
  }));
  check("history opens a round read-only", viewing.round >= 1 && viewing.readonly && viewing.bar,
    JSON.stringify(viewing));
  await page.click("#viewing-bar");
  await page.waitForFunction(() => window.redline.viewing === null, { timeout: 5000 });
  check("Current returns to the live canvas", true);
  await stopReview();

  // ---- ellipse tool draws a real ellipse
  await page.keyboard.press("e");
  const ellipseSpot = at(420, 620);
  await drag(page, ellipseSpot, { x: ellipseSpot.x + 140, y: ellipseSpot.y + 70 });
  const ellipse = await page.evaluate(() => {
    const shapes = window.redline.annotations.filter((a) => a.type === "ellipse");
    return shapes[shapes.length - 1] ? { ...shapes[shapes.length - 1] } : null;
  });
  check("ellipse tool draws an ellipse", Boolean(ellipse), ellipse ? JSON.stringify(ellipse.type) : "none");

  // ---- group: clicking one member selects the whole group
  // Draw a fresh rectangle inside the image so the coordinates cannot go stale from the
  // alignment above (which moved the earlier one).
  await page.keyboard.press("r");
  await drag(page, at(500, 300), at(600, 400));
  await page.keyboard.press("v");
  const rectA = at(310, 230);
  const rectB = at(550, 350);
  await page.mouse.click(rectA.x, rectA.y);
  await page.keyboard.down("Shift");
  await page.mouse.click(rectB.x, rectB.y);
  await page.keyboard.up("Shift");
  const beforeGroup = await page.evaluate(() => window.redline.selected.size);
  check("selected both rectangles before grouping", beforeGroup === 2, `${beforeGroup} selected`);
  await page.keyboard.press("g");
  const grouped = await page.evaluate(() =>
    window.redline.annotations.filter((a) => a.group).length);
  check("grouping tags the selected annotations", grouped === 2, `${grouped} grouped`);

  await page.mouse.click(rectA.x, rectA.y);
  const groupSelection = await page.evaluate(() => window.redline.selected.size);
  check("clicking one member selects the group", groupSelection === 2, `${groupSelection} selected`);

  await page.keyboard.down("Shift");
  await page.keyboard.press("g");
  await page.keyboard.up("Shift");
  const ungrouped = await page.evaluate(() =>
    window.redline.annotations.filter((a) => a.group).length);
  check("shift+g ungroups", ungrouped === 0, `${ungrouped} still grouped`);

  // ---- z-order
  await page.mouse.click(rectA.x, rectA.y);
  await page.click('[data-layer="front"]');
  const last = await page.evaluate(() => {
    const list = window.redline.annotations;
    return { type: list[list.length - 1].type, x: list[list.length - 1].start?.x };
  });
  check("bring to front moves the selection last in paint order", last.type === "rect", JSON.stringify(last));

  // ---- diff between the archived round and now
  const rounds = await api(`/api/sessions/${id}/history`);
  // The newest entry is the live submission (every submit is a round now); compare the oldest.
  const stamp = (rounds.submissions[rounds.submissions.length - 1] || rounds.submissions[0])?.stamp;
  if (stamp) {
    const diff = await api(`/api/sessions/${id}/history/${stamp}/diff`);
    check("diff endpoint compares rounds", typeof diff.from.annotations === "number"
      && Array.isArray(diff.added) && Array.isArray(diff.removed), JSON.stringify(diff.from));
    check("diff noticed the new annotations", diff.added.length > 0, `${diff.added.length} added`);
  }

  // ---- rail actions: confirm, reopen, cancel, reopen
  const row = `.session[data-id="${id}"]`;
  const clickChip = (label) => page.evaluate((text) => {
    const chip = Array.from(document.querySelectorAll(".chip"))
      .find((element) => element.textContent.trim().startsWith(text));
    chip?.click();
  }, label);

  await page.hover(row);
  await page.click(`${row} [data-act="confirm"]`);
  await page.waitForSelector("#feature-dialog[open]", { timeout: 5000 });
  const confirmDialog = await page.evaluate(() => {
    const accept = document.querySelector('#feature-actions [data-action="confirm"]');
    return {
      title: document.querySelector("#feature-dialog-title").textContent,
      body: document.querySelector("#feature-dialog-body").textContent,
      accept: accept?.textContent.trim() || "",
      primary: Boolean(accept?.classList.contains("primary")),
      count: document.querySelectorAll("#feature-actions button").length,
    };
  });
  check("confirming a feature asks for agreement first",
    confirmDialog.title === "Confirm this feature?"
      && confirmDialog.accept === "Confirm feature"
      && confirmDialog.body.includes("pull request")
      && confirmDialog.primary && confirmDialog.count === 2,
    JSON.stringify(confirmDialog));
  await page.click('#feature-actions [data-action="confirm"]');
  let status = "";
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const meta = await api(`/api/sessions/${id}`);
    status = meta.meta.status;
    if (status === "resolved" && meta.meta.feature_confirmed_at) break;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  // `railAction` reloads the rail after the POST, but the click resolves before that await does;
  // poll the rail's own data instead of reading it a frame too early. The card itself is checked
  // in the Done filter below — a resolved session leaves the default Active view.
  let confirmedState = "";
  for (let attempt = 0; attempt < 30 && confirmedState !== "confirmed"; attempt += 1) {
    confirmedState = await page.evaluate(() => {
      const entry = window.redline.rail.sessions.find((session) => session.id === window.redline.sessionId);
      return entry?.feature_state || "";
    });
    if (confirmedState !== "confirmed") await new Promise((resolve) => setTimeout(resolve, 100));
  }
  const confirmed = await page.evaluate(() => ({
    stamped: Boolean(window.redline.meta?.feature_confirmed_at),
    row: Boolean(document.querySelector('.session[aria-current="true"]')),
  }));
  check("rail 'confirm' closes the feature and records the decision",
    status === "resolved" && confirmed.stamped && confirmedState === "confirmed" && !confirmed.row,
    JSON.stringify({ status, ...confirmed, state: confirmedState }));

  // "All" is the only view that keeps a finished session alongside active work.
  await clickChip("All");
  await page.waitForSelector(row, { timeout: 5000 });
  const allFilter = await page.evaluate(() => ({
    filter: window.redline.rail.filter,
    visible: Boolean(document.querySelector('.session[aria-current="true"]')),
    mine: Boolean(document.querySelector('.chip[data-filter="mine"]')),
  }));
  check("the All filter shows every session and the Mine chip is gone",
    allFilter.filter === "all" && allFilter.visible && !allFilter.mine,
    JSON.stringify(allFilter));

  // Resolved sessions leave the default 'Active' view, so check the badge and reopen them from
  // the 'Done' filter.
  await clickChip("Done");
  await page.waitForSelector(row, { timeout: 5000 });
  const confirmedRow = await page.evaluate((sessionId) => ({
    badge: document.querySelector(`.session[data-id="${sessionId}"] .badge`)?.textContent || "",
    row: Boolean(document.querySelector(`.session[data-id="${sessionId}"] [data-act="reopen"]`)),
  }), id);
  check("a confirmed feature shows a badge and a reopen action",
    confirmedRow.badge.includes("confirmed") && confirmedRow.row, JSON.stringify(confirmedRow));
  await page.hover(row);
  await page.click(`${row} [data-act="menu"]`);
  await page.waitForSelector("#feature-dialog[open]", { timeout: 5000 });
  const decidedMenu = await page.evaluate(() => {
    const buttons = [...document.querySelectorAll("#feature-actions button")];
    return {
      actions: buttons.map((button) => button.textContent.trim()),
      deleteDanger: Boolean(buttons.find((button) => button.dataset.action === "delete")
        ?.classList.contains("danger")),
    };
  });
  check("a confirmed feature's menu offers reopen and delete",
    decidedMenu.actions.join("|") === "Keep working|Reopen|Delete files…"
      && decidedMenu.deleteDanger, JSON.stringify(decidedMenu));
  await page.click('#feature-actions [data-action="keep"]');
  await page.hover(row);
  await page.$eval(`${row} [data-act="reopen"]`, (button) => button.click());
  let reopened = {};
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const meta = await api(`/api/sessions/${id}`);
    reopened = meta.meta;
    if (reopened.status === "open") break;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  check("rail 'reopen' clears the confirmation and puts it back in the queue",
    reopened.status === "open" && !reopened.feature_confirmed_at
      && !reopened.feature_cancelled_at,
    JSON.stringify({ status: reopened.status, confirmed: reopened.feature_confirmed_at }));

  await clickChip("Active");
  await page.waitForSelector(row, { timeout: 5000 });
  await page.hover(row);
  await page.click(`${row} [data-act="menu"]`);
  await page.waitForSelector("#feature-dialog[open]", { timeout: 5000 });
  const featureMenu = await page.evaluate(() => {
    const buttons = [...document.querySelectorAll("#feature-actions button")];
    return {
      title: document.querySelector("#feature-dialog-title").textContent,
      body: document.querySelector("#feature-dialog-body").textContent,
      actions: buttons.map((button) => button.textContent.trim()),
      deleteDanger: Boolean(buttons.find((button) => button.dataset.action === "delete")
        ?.classList.contains("danger")),
    };
  });
  check("the row menu offers cancel and delete without deleting instantly",
    featureMenu.title === "What should happen to this feature?"
      && featureMenu.actions.join("|") === "Keep working|Cancel feature|Delete files…"
      && featureMenu.body.includes("keeps the screenshot")
      && featureMenu.deleteDanger,
    JSON.stringify(featureMenu));
  await page.click('#feature-actions [data-action="cancel-feature"]');
  let cancelled = {};
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const meta = await api(`/api/sessions/${id}`);
    cancelled = meta.meta;
    if (cancelled.feature_cancelled_at) break;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  check("rail 'cancel' records the cancelled feature without deleting the review",
    cancelled.status === "resolved" && Boolean(cancelled.feature_cancelled_at)
      && !cancelled.feature_confirmed_at,
    JSON.stringify({ status: cancelled.status, cancelled: cancelled.feature_cancelled_at }));

  await clickChip("Done");
  await page.waitForSelector(row, { timeout: 5000 });
  await page.hover(row);
  await page.$eval(`${row} [data-act="reopen"]`, (button) => button.click());
  let reopenedCancel = {};
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const meta = await api(`/api/sessions/${id}`);
    reopenedCancel = meta.meta;
    if (reopenedCancel.status === "open") break;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  check("reopening a cancelled feature clears both decisions",
    reopenedCancel.status === "open" && !reopenedCancel.feature_cancelled_at
      && !reopenedCancel.feature_confirmed_at,
    JSON.stringify({ status: reopenedCancel.status }));
  await clickChip("Active");
  await page.waitForSelector(row, { timeout: 5000 });

  const annotated = await fetch(`${BASE}/sessions/${id}/annotated.png`, {
    headers: { Authorization: `Bearer ${TOKEN}` },
  });
  check("annotated.png is served", annotated.ok, `${annotated.status}`);

  // ---- the agent posts the next round into the same session instead of opening a new review
  await api("/mcp", {
    method: "POST",
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: {
        name: "post_round",
        arguments: { session_id: id, note: "e2e next round", image_base64: makePng() },
      },
    }),
  });
  await page.waitForFunction(
    () => {
      const meta = String(window.redline.meta.image || "");
      const src = window.redline.image?.src || "";
      // `meta` changes a poll before the new PNG has decoded, so wait for the canvas too.
      return meta.startsWith("screenshot-") && src.includes(meta);
    },
    { timeout: 25000 },
  );
  const round = await page.evaluate(() => ({
    image: window.redline.meta.image,
    src: window.redline.image?.src || "",
    text: document.querySelector("#conv-log")?.textContent || "",
    picture: Boolean(document.querySelector("#conv-log .conv-img")),
    history: document.querySelector("#history-count")?.textContent || "0",
  }));
  check("agent round replaces the canvas image in place", round.image.startsWith("screenshot-"),
    round.image);
  check("canvas reloads the new round (cache-busted)", round.src.includes(round.image),
    round.src);
  check("agent round appears in the conversation, without a picture",
    round.text.includes("e2e next round") && !round.picture, JSON.stringify(round).slice(0, 120));
  check("round fills the history", Number(round.history) >= 1, round.history);

  // ---- the live row keeps its own count while another round is previewed
  if (!(await page.$eval("#history-block", (el) => el.open))) {
    await page.click("#history-block > summary");
  }
  await page.evaluate(() => window.redline.loadHistory());
  await page.waitForFunction(
    () => Boolean(document.querySelector('#history-list [data-round="current"]')),
    { timeout: 5000 },
  );
  const liveBefore = await page.evaluate(() => window.redline.annotations.length);

  await page.evaluate(() => {
    const rows = [...document.querySelectorAll("#history-list [data-round]")];
    (rows.find((row) => row.dataset.round !== "current") || rows[0]).click();
  });
  await page.waitForFunction(() => Boolean(window.redline.viewing), { timeout: 5000 });
  // `viewRound` re-renders the list asynchronously; wait for the rebuild before reading it.
  await page.waitForFunction(
    () => Boolean(document.querySelector('#history-list [data-round="current"]')),
    { timeout: 5000 },
  );
  const historyState = await page.evaluate(() => ({
    current: document.querySelector('#history-list [data-round="current"]')?.textContent || "",
    live: Boolean(document.querySelector("#history-list .history-row.live")),
    selected: Boolean(document.querySelector("#history-list .history-row.selected")),
  }));
  check("Current keeps the live count while a round is previewed",
    historyState.current.includes(`${liveBefore} annotation`), historyState.current.trim());
  check("history marks the live and the selected row",
    historyState.live && historyState.selected, JSON.stringify(historyState));
  const borders = await page.evaluate(() => {
    const row = document.querySelector("#history-list .history-row.selected");
    const style = getComputedStyle(row);
    return [style.borderTopColor, style.borderRightColor, style.borderBottomColor];
  });
  check("selected row is outlined on every side",
    borders[0] === borders[1] && borders[1] === borders[2], borders.join(" / "));
  await page.click("#viewing-bar");
  await page.waitForFunction(() => window.redline.viewing === null, { timeout: 5000 });

  // ---- unsent annotations survive a session switch
  await page.keyboard.press("r");
  await drag(page, at(300, 520), at(420, 580));
  const beforeSwitch = await page.evaluate(() => window.redline.annotations.length);
  const other = await api("/api/sessions", {
    method: "POST",
    body: JSON.stringify({ title: "draft target", image_base64: makePng() }),
  });
  await page.evaluate((otherId) => window.redline.openSession(otherId), other.meta.id);
  await page.waitForFunction((otherId) => window.redline.sessionId === otherId, {}, other.meta.id);
  await page.evaluate((self) => window.redline.openSession(self), id);
  await page.waitForFunction(
    (self) => window.redline.sessionId === self && window.redline.annotations.length > 0,
    { timeout: 5000 },
    id,
  );
  const afterSwitch = await page.evaluate(() => window.redline.annotations.length);
  check("unsent annotations survive a session switch", afterSwitch === beforeSwitch,
    `${beforeSwitch} → ${afterSwitch}`);

  // ---- submitting locks the canvas; STOP takes the submission back
  // A second annotation, so the read-only list below has a row *pair* to measure a separator on.
  await page.evaluate(() => window.redline.mutate(() => {
    window.redline.annotations.push({
      id: "e2e-second", type: "rect", color: "#e11d48", width: 3,
      start: { x: 120, y: 160 }, end: { x: 260, y: 240 }, note: "second",
    });
  }));
  const beforeStop = await page.evaluate(() => window.redline.annotations.length);
  await page.keyboard.down("Control");
  await page.keyboard.press("Enter");
  await page.keyboard.up("Control");
  await page.waitForFunction(() => window.redline.meta?.status === "submitted", { timeout: 10000 });
  // Let the "Submitted ✓" timeout fire: it used to overwrite STOP with "Submit again".
  await new Promise((resolve) => setTimeout(resolve, 1700));
  const locked = await page.evaluate(() => {
    const button = document.querySelector("#submit");
    const status = document.querySelector("#stage-status");
    return {
      status: status && !status.hidden ? status.textContent.trim() : "",
      locked: Boolean(window.redline.locked),
      stop: button.textContent.trim(),
      red: getComputedStyle(button).backgroundColor,
      square: Boolean(button.querySelector(".stop-square")),
      composer: document.querySelector("#composer-text").disabled,
    };
  });
  check("submitting locks the canvas and turns Submit into a red STOP with a square",
    locked.status.length > 0 && locked.locked && locked.stop === "STOP" && locked.composer
      && locked.red === "rgb(220, 38, 38)" && locked.square,
    JSON.stringify(locked));

  const lockedView = await page.evaluate(() => ({
    dim: !document.querySelector("#stage-dim").hidden,
    zoomDisabled: document.querySelector("#zoom-in").disabled,
    scale: window.redline.view.scale,
    x: window.redline.view.x,
    y: window.redline.view.y,
  }));
  await page.$eval("#stage", (stage) => stage.dispatchEvent(new WheelEvent("wheel", {
    deltaY: -240, ctrlKey: true, bubbles: true, cancelable: true,
  })));
  await page.keyboard.press("+");
  const lockedViewAfter = await page.evaluate(() => ({
    scale: window.redline.view.scale,
    x: window.redline.view.x,
    y: window.redline.view.y,
  }));
  check("a locked canvas dims and ignores scroll and zoom",
    lockedView.dim && lockedView.zoomDisabled
      && lockedViewAfter.scale === lockedView.scale
      && lockedViewAfter.x === lockedView.x && lockedViewAfter.y === lockedView.y,
    JSON.stringify({ lockedView, lockedViewAfter }));

  // The lock must *refuse* edits, not merely report `locked`: the inspector's align/layer/group
  // bar stayed armed after a submit, and Delete/arrow keys still rewrote the selected annotation.
  await page.evaluate(() => window.redline.select(0));
  const guarded = await page.evaluate(() => ({
    before: JSON.stringify(window.redline.annotations),
    tool: window.redline.tool,
    toolsHidden: document.querySelector("#selection-tools").classList.contains("hidden"),
    toolsDisabled: [...document.querySelectorAll(".options .tool[data-tool]")]
      .every((button) => button.disabled),
  }));
  await page.keyboard.press("Delete");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("a");
  await drag(page, { x: 400, y: 500 }, { x: 520, y: 560 });
  const stillLocked = await page.evaluate(() => ({
    after: JSON.stringify(window.redline.annotations),
    tool: window.redline.tool,
  }));
  check("a locked review refuses drawing, deleting, nudging and the inspector write tools",
    guarded.toolsHidden && guarded.toolsDisabled && stillLocked.after === guarded.before
      && stillLocked.tool === guarded.tool,
    JSON.stringify({ toolsHidden: guarded.toolsHidden, toolsDisabled: guarded.toolsDisabled,
      changed: stillLocked.after !== guarded.before, tool: stillLocked.tool }));
  // The read-only list is a list, not a stack of broken frames: the separator between two rows
  // has to be a straight hairline across the whole row. As a `border-top` on a rounded box it
  // painted only the middle segment, so each row looked like an open frame with missing ends.
  const list = await page.evaluate(() => {
    const rows = [...document.querySelectorAll("#ann-list .ann-row")];
    if (rows.length < 2) return { rows: rows.length };
    const style = getComputedStyle(rows[1]);
    const first = rows[0].getBoundingClientRect();
    const second = rows[1].getBoundingClientRect();
    return {
      rows: rows.length,
      readonly: rows.every((row) => row.classList.contains("readonly")
        && getComputedStyle(row).cursor === "default"),
      radius: style.borderTopLeftRadius,
      separator: style.borderTopColor,
      firstSeparator: getComputedStyle(rows[0]).borderTopColor,
      sameWidth: Math.abs(first.width - second.width) < 0.5
        && Math.abs(first.left - second.left) < 0.5,
    };
  });
  check("a locked review lists its annotations as flat read-only rows with a full separator",
    list.rows < 2 || (list.readonly && list.radius === "0px" && list.sameWidth
      && list.separator !== list.firstSeparator
      && list.separator !== "rgba(0, 0, 0, 0)"),
    JSON.stringify(list));

  await page.evaluate(() => window.redline.select(-1));

  await page.waitForFunction(() => {
    const button = document.querySelector("#submit");
    return button && !button.disabled && button.textContent.trim() === "STOP";
  }, { timeout: 5000 });
  page.once("dialog", (dialog) => dialog.accept());
  await page.click("#submit");
  await page.waitForFunction(() => window.redline.meta?.status === "open", { timeout: 10000 });
  const unlocked = await page.evaluate(() => ({
    locked: Boolean(window.redline.locked),
    dim: !document.querySelector("#stage-dim").hidden,
    zoomDisabled: document.querySelector("#zoom-in").disabled,
    label: document.querySelector("#submit").textContent.trim(),
    annotations: window.redline.annotations.length,
  }));
  check("STOP unlocks the canvas and keeps the annotations",
    !unlocked.locked && !unlocked.dim && !unlocked.zoomDisabled
      && unlocked.label === "Submit review" && unlocked.annotations === beforeStop,
    JSON.stringify(unlocked));

  // ---- touch mode: bigger tools, row actions not hover-only
  const touch = await page.evaluate(() => {
    document.body.classList.add("touch");
    const tool = document.querySelector(".options .tool");
    const act = document.querySelector(".session .act");
    const style = getComputedStyle(tool);
    const opacity = act ? getComputedStyle(act).opacity : "1";
    // `getComputedStyle` is live: read the values before dropping the class again.
    const width = style.minWidth;
    const height = style.minHeight;
    document.body.classList.remove("touch");
    return { width, height, opacity };
  });
  check("touch mode enlarges tools and shows row actions",
    touch.width === "40px" && touch.height === "40px" && touch.opacity === "1",
    JSON.stringify(touch));

  // ---- every tool has to work end to end, not just select its button. The pen regression:
  // `renderInspector` labelled shapes from `annotation.start`, but a pen has `points`; the
  // TypeError aborted `mutate` before `render()`, so the live stroke stayed visible with no
  // badge and no inspector row. Clicking the tool alone never caught it.
  const penSession = await api("/api/sessions", {
    method: "POST",
    body: JSON.stringify({ title: "pen regression", image_base64: makePng(), project: "E2E" }),
  });
  const penId = penSession.meta.id;
  await page.evaluate((sessionId) => window.redline.openSession(sessionId), penId);
  await page.waitForFunction(
    (sessionId) => window.redline.sessionId === sessionId
      && Boolean(window.redline.image) && window.redline.view.fitted,
    { timeout: 10000 },
    penId,
  );
  await page.click('.tool[data-tool="pen"]');
  await drag(page, at(300, 200), at(520, 340), 20);
  const pen = await page.evaluate(() => {
    const editor = window.redline;
    const index = editor.annotations.length - 1;
    const annotation = editor.annotations[index];
    const row = document.querySelector(`#ann-list .ann-row[data-index="${index}"]`);
    const first = annotation?.points?.[0] || { x: 0, y: 0 };
    const inset = 2 / editor.view.scale;
    const radius = 12 / editor.view.scale;
    const x = Math.round((editor.view.x + (first.x + inset + radius * 0.7) * editor.view.scale) * editor.dpr);
    const y = Math.round((editor.view.y + (first.y + inset) * editor.view.scale) * editor.dpr);
    const pixel = document.querySelector("#canvas").getContext("2d").getImageData(x, y, 1, 1).data;
    return {
      type: annotation?.type,
      points: annotation?.points?.length || 0,
      count: editor.annotations.length,
      rows: document.querySelectorAll("#ann-list .ann-row").length,
      number: row?.querySelector(".ann-num")?.textContent.trim() || "",
      label: row?.querySelector(".label b")?.textContent.trim() || "",
      pixel: [pixel[0], pixel[1], pixel[2], pixel[3]],
    };
  });
  check("a freehand stroke is numbered and listed like every other annotation",
    pen.type === "pen" && pen.points > 1 && pen.count === 1
      && pen.rows === 1 && pen.number === "1" && pen.label.includes("freehand"),
    JSON.stringify(pen));
  check("the freehand badge is painted on the canvas",
    pen.pixel[0] > 150 && pen.pixel[0] > pen.pixel[1] * 2 && pen.pixel[0] > pen.pixel[2] * 1.5,
    JSON.stringify(pen.pixel));

  // The pen has its own move path (`translateAnnotation` shifts `points`) and bounds/hit test.
  const penBefore = await page.evaluate(() => window.redline.annotations[0].points[0]);
  await page.keyboard.press("v");
  await drag(page, at(400, 264), at(430, 284));
  const penMoved = await page.evaluate(() => ({
    point: window.redline.annotations[0].points[0],
    scale: window.redline.view.scale,
  }));
  const penDx = (penMoved.point.x - penBefore.x) * penMoved.scale;
  const penDy = (penMoved.point.y - penBefore.y) * penMoved.scale;
  check("dragging a freehand stroke moves its points",
    Math.abs(penDx - 30) < 4 && Math.abs(penDy - 20) < 4,
    `moved ${penDx.toFixed(1)},${penDy.toFixed(1)} screen px`);
  await page.keyboard.down("Control");
  await page.keyboard.press("z");
  await page.keyboard.up("Control");
  const penRestored = await page.evaluate(() => window.redline.annotations[0].points[0]);
  check("undo restores a freehand stroke",
    penRestored.x === penBefore.x && penRestored.y === penBefore.y,
    JSON.stringify({ before: penBefore, after: penRestored }));

  await page.click("#submit");
  await page.waitForFunction(() => window.redline.meta?.status === "submitted", { timeout: 10000 });
  const penStored = await api(`/api/sessions/${penId}`);
  const storedPen = penStored.annotations.annotations.find((annotation) => annotation.type === "pen");
  check("a submitted freehand stroke keeps its points on the server",
    Boolean(storedPen) && storedPen.points.length > 1,
    JSON.stringify(storedPen || null));

  // The same full path for the other shape tools: draw, number, list, badge.
  for (const tool of ["ellipse", "arrow"]) {
    await freshSession(`tool: ${tool}`);
    await page.click(`.tool[data-tool="${tool}"]`);
    await drag(page, at(300, 200), at(460, 320), 12);
    const report = await readLastAnnotation();
    check(`${tool} stroke is numbered, listed and badged`,
      report.type === tool && report.count === 1 && report.rows === 1
        && report.number === "1" && report.label.includes("→")
        && report.pixel[0] > 150 && report.pixel[0] > report.pixel[1] * 2
        && report.pixel[0] > report.pixel[2] * 1.5,
      JSON.stringify(report));
  }

  // Text notes are inspector notes / legacy history since v0.8; there is no canvas text tool
  // left to test here. The rectangle's note is covered above.

  // ---- bulk confirm from the rail
  // The rail may still be filtered to Done from the reopen test; new sessions are active.
  await page.evaluate(() => {
    const chip = [...document.querySelectorAll(".chip")]
      .find((element) => element.textContent.trim().startsWith("Active"));
    chip?.click();
  });
  const extra = [];
  for (const title of ["bulk one", "bulk two"]) {
    const made = await api("/api/sessions", {
      method: "POST",
      body: JSON.stringify({ title, image_base64: makePng() }),
    });
    extra.push(made.meta.id);
  }
  await page.evaluate(() => window.redline.rail.load());
  await page.waitForFunction(
    (ids) => ids.every((id) => document.querySelector(`.session[data-id="${id}"]`)),
    { timeout: 5000 },
    extra,
  );
  await page.click("#bulk-mode");
  await page.waitForFunction(() => !document.querySelector("#rail-bulk").hidden);
  await page.evaluate((ids) => {
    for (const id of ids) document.querySelector(`.session[data-id="${id}"]`).click();
  }, extra);
  const bulkSelected = await page.evaluate(() => window.redline.rail.selection.size);
  check("bulk mode selects sessions without opening them", bulkSelected === 2, `${bulkSelected}`);
  await page.click("#bulk-confirm");
  await page.waitForSelector("#feature-dialog[open]", { timeout: 5000 });
  await page.click('#feature-actions [data-action="confirm"]');
  await page.waitForFunction(() => window.redline.rail.bulkMode === false, { timeout: 10000 });
  const bulkStatuses = [];
  const bulkConfirmed = [];
  for (const id of extra) {
    const meta = (await api(`/api/sessions/${id}`)).meta;
    bulkStatuses.push(meta.status);
    bulkConfirmed.push(Boolean(meta.feature_confirmed_at));
  }
  check("bulk confirm marks every selected feature confirmed",
    bulkStatuses.every((status) => status === "resolved")
      && bulkConfirmed.every(Boolean), JSON.stringify({ bulkStatuses, bulkConfirmed }));

  // ---- deleting from the row is one explicit dialog away
  const doomed = await api("/api/sessions", {
    method: "POST",
    body: JSON.stringify({ title: "delete me", image_base64: makePng() }),
  });
  const doomedId = doomed.meta.id;
  const doomedRow = `.session[data-id="${doomedId}"]`;
  await page.evaluate(() => window.redline.rail.load());
  await page.waitForSelector(doomedRow, { timeout: 5000 });
  await page.hover(doomedRow);
  await page.click(`${doomedRow} [data-act="menu"]`);
  await page.waitForSelector("#feature-dialog[open]", { timeout: 5000 });
  await page.click('#feature-actions [data-action="delete"]');
  await page.waitForFunction(
    (sessionId) => !document.querySelector(`.session[data-id="${sessionId}"]`),
    { timeout: 10000 },
    doomedId,
  );
  const afterDelete = await api("/api/sessions");
  check("deleting from the row removes the session and its files",
    !afterDelete.sessions.some((session) => session.id === doomedId), doomedId);

  // ---- "Delete cancelled" is scoped to cancelled features; confirmed ones are the log
  const cancelMe = await api("/api/sessions", {
    method: "POST",
    body: JSON.stringify({ title: "cancel me", image_base64: makePng() }),
  });
  const cancelMeId = cancelMe.meta.id;
  await api(`/api/sessions/${cancelMeId}/cancel-feature`, { method: "POST" });
  await page.evaluate(() => window.redline.rail.load());
  const purge = await page.evaluate(() => ({
    hidden: document.querySelector("#purge-cancelled")?.hidden,
    label: document.querySelector("#purge-cancelled")?.textContent.trim() || "",
  }));
  check("Delete cancelled appears for a cancelled feature",
    purge.hidden === false && purge.label.includes("Delete cancelled"),
    JSON.stringify(purge));
  page.once("dialog", (dialog) => dialog.accept());
  await page.click("#purge-cancelled");
  await page.waitForFunction(
    () => document.querySelector("#purge-cancelled")?.hidden !== false,
    { timeout: 10000 },
  );
  const afterPurge = await api("/api/sessions");
  const remaining = new Set(afterPurge.sessions.map((session) => session.id));
  check("Delete cancelled removes only cancelled features",
    !remaining.has(cancelMeId) && extra.every((id) => remaining.has(id)),
    JSON.stringify({ cancelMeId, extras: extra.map((id) => remaining.has(id)) }));

  // ---- accessible names and a live region
  const a11y = await page.evaluate(() => ({
    canvas: document.querySelector("#canvas")?.getAttribute("role") === "img",
    canvasLabel: Boolean(document.querySelector("#canvas")?.getAttribute("aria-label")),
    composer: Boolean(document.querySelector("#composer-text")?.getAttribute("aria-label")),
    live: document.querySelector("#conv-log")?.getAttribute("aria-live"),
    labels: [...document.querySelectorAll(".session")].every((row) => row.getAttribute("aria-label")),
  }));
  check("accessible names and live region are present",
    a11y.canvas && a11y.canvasLabel && a11y.composer && a11y.live === "polite" && a11y.labels,
    JSON.stringify(a11y));

  // ---- the connect wizard uses the app shell and always offers a way back
  await page.goto(`${BASE}/setup?t=${TOKEN}`, { waitUntil: "networkidle2" });
  const setup = await page.evaluate(() => ({
    back: document.querySelector("#back")?.getAttribute("href") || "",
    primary: document.querySelector("#save")?.className || "",
    secondary: document.querySelector("#back-servers")?.className || "",
    secondaryHref: document.querySelector("#back-servers")?.getAttribute("href") || "",
    localSwitch: Boolean(document.querySelector("#local")),
    card: getComputedStyle(document.querySelector(".setup-card")).borderRadius,
    css: getComputedStyle(document.body).backgroundColor,
  }));
  check("setup page goes back to the server list and has no duplicate standalone switch",
    setup.back.startsWith("/") && setup.primary.includes("accent")
      && setup.secondary.includes("btn") && setup.secondaryHref.includes("/servers")
      && !setup.localSwitch && setup.card !== "0px",
    JSON.stringify(setup));

  await page.goto(`${BASE}/servers?t=${TOKEN}`, { waitUntil: "networkidle2" });
  await page.waitForFunction(
    () => (document.querySelector("#list")?.textContent || "").includes("desktop app"),
    { timeout: 5000 },
  );
  const serversPage = await page.evaluate(() => ({
    title: document.querySelector("h1")?.textContent.trim() || "",
    text: document.querySelector("#list")?.textContent || "",
  }));
  check("servers page is local-only and explains headless mode",
    serversPage.title === "Servers" && serversPage.text.includes("desktop app"),
    JSON.stringify(serversPage));

  // Headless mode has no profiles, so render the page's own row markup and measure the
  // destructive button in both themes: an outline must not inherit the filled `.btn.danger`
  // background, or the red label disappears into a red button.
  const removeButton = await page.evaluate(() => {
    const host = document.getElementById("list");
    host.innerHTML = row({ id: "e2e-alpha", name: "Alpha", url: "https://alpha.example", active: false }, false)
      + row({ id: "e2e-beta", name: "Beta", url: "https://beta.example", active: true }, false);
    const button = host.querySelector("[data-remove]");
    if (!button) return null;
    const rgb = (value) => value.match(/[\d.]+/g).map(Number).slice(0, 3);
    const luminance = ([r, g, b]) => {
      const channel = (c) => {
        const s = c / 255;
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
    };
    const sample = () => {
      const style = getComputedStyle(button);
      const card = getComputedStyle(document.querySelector(".servers-card")).backgroundColor;
      const [light, dark] = [luminance(rgb(style.color)), luminance(rgb(card))].sort((a, b) => b - a);
      return {
        color: style.color,
        background: style.backgroundColor,
        contrast: Number(((light + 0.05) / (dark + 0.05)).toFixed(2)),
      };
    };
    const theme = document.documentElement.dataset.theme || "light";
    document.documentElement.dataset.theme = "dark";
    const dark = sample();
    document.documentElement.dataset.theme = "light";
    const light = sample();
    document.documentElement.dataset.theme = theme;
    return { text: button.textContent.trim(), dark, light };
  });
  check("servers page Remove button is an outline with a readable label in both themes",
    Boolean(removeButton) && removeButton.text === "Remove"
      && removeButton.dark.background === "rgba(0, 0, 0, 0)"
      && removeButton.light.background === "rgba(0, 0, 0, 0)"
      && removeButton.dark.contrast >= 4.5 && removeButton.light.contrast >= 4.5,
    JSON.stringify(removeButton));

  await page.goto(`${BASE}/`);
  await page.waitForSelector(".gallery .card", { timeout: 5000 });
  const cards = await page.$$eval(".gallery .card", (elements) => elements.length);
  check("start page shows a gallery of reviews", cards >= 1, `${cards} cards`);

  await page.click("#new-review");
  await page.waitForFunction(
    () => [...document.querySelectorAll("#f-project option")].some((option) => option.value === "e2e-repo"),
    { timeout: 5000 },
  );
  const listProject = await page.evaluate(() => ({
    select: document.querySelector("#f-project")?.tagName,
    options: [...document.querySelectorAll("#f-project option")].map((option) => option.value),
  }));
  check("start page new review has the same project select",
    listProject.select === "SELECT" && listProject.options.includes("e2e")
      && listProject.options.includes("e2e-repo"), JSON.stringify(listProject));

  // Create through the picker: the project chosen in the select must be the one stored.
  await page.evaluate(async () => {
    const canvas = document.createElement("canvas");
    canvas.width = 120;
    canvas.height = 80;
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#16a34a";
    ctx.fillRect(0, 0, 120, 80);
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
    const data = new DataTransfer();
    data.items.add(new File([blob], "picked.png", { type: "image/png" }));
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: data });
    window.dispatchEvent(event);
  });
  await page.waitForFunction(() => !document.querySelector("#drop-preview").hidden, { timeout: 5000 });
  await page.$eval("#f-title", (el) => { el.value = "from the project picker"; });
  await page.select("#f-project", "e2e-repo");
  await page.click("#new-submit");
  await page.waitForFunction(() => location.pathname.startsWith("/s/"), { timeout: 15000 });
  const pickedId = new URL(page.url()).pathname.split("/").pop();
  const picked = await api(`/api/sessions/${pickedId}`);
  check("creating from the start page files the review in the picked repo project",
    picked.meta.project === "e2e-repo" && picked.meta.title === "from the project picker",
    `${picked.meta.project} · ${picked.meta.title}`);

  // ---- the dividers resize the panels (rail width, inspector width, conversation height)
  const panels = () => page.evaluate(() => ({
    rail: Math.round(document.querySelector(".rail").getBoundingClientRect().width),
    inspector: Math.round(document.querySelector(".inspector").getBoundingClientRect().width),
    conv: Math.round(document.querySelector("#conversation").getBoundingClientRect().height),
    canvas: document.querySelector("#canvas").width,
    stored: {
      rail: Number(localStorage.getItem("redline:railWidth")),
      inspector: Number(localStorage.getItem("redline:inspectorWidth")),
      conv: Number(localStorage.getItem("redline:convHeight")),
    },
  }));
  const handleAt = (selector) => page.$eval(selector, (el) => {
    const rect = el.getBoundingClientRect();
    return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
  });

  const panelsBefore = await panels();
  const railHandle = await handleAt("#rail-resizer");
  await drag(page, railHandle, { x: railHandle.x + 70, y: railHandle.y });
  const railDragged = await panels();
  check("dragging the rail divider widens the session list",
    Math.abs(railDragged.rail - panelsBefore.rail - 70) <= 2,
    `${panelsBefore.rail}px → ${railDragged.rail}px`);
  check("the canvas is re-measured while the rail moves",
    railDragged.canvas < panelsBefore.canvas, `${panelsBefore.canvas} → ${railDragged.canvas}`);

  const inspectorHandle = await handleAt("#inspector-resizer");
  await drag(page, inspectorHandle, { x: inspectorHandle.x - 60, y: inspectorHandle.y });
  const inspectorDragged = await panels();
  check("dragging the inspector divider widens the annotation panel",
    Math.abs(inspectorDragged.inspector - panelsBefore.inspector - 60) <= 2,
    `${panelsBefore.inspector}px → ${inspectorDragged.inspector}px`);

  const convHandle = await handleAt("#conv-resizer");
  await drag(page, convHandle, { x: convHandle.x, y: convHandle.y - 50 });
  const convDragged = await panels();
  check("dragging the conversation divider gives it more room",
    Math.abs(convDragged.conv - panelsBefore.conv - 50) <= 2,
    `${panelsBefore.conv}px → ${convDragged.conv}px`);
  check("panel sizes are remembered",
    Math.abs(convDragged.stored.rail - convDragged.rail) <= 1
      && Math.abs(convDragged.stored.inspector - convDragged.inspector) <= 1
      && Math.abs(convDragged.stored.conv - convDragged.conv) <= 2,
    JSON.stringify(convDragged.stored));

  await page.reload({ waitUntil: "networkidle2" });
  await page.waitForFunction(() => window.redline && window.redline.image, { timeout: 15000 });
  const restored = await panels();
  check("panel sizes survive a reload",
    Math.abs(restored.rail - convDragged.rail) <= 2
      && Math.abs(restored.inspector - convDragged.inspector) <= 2
      && Math.abs(restored.conv - convDragged.conv) <= 2,
    `${restored.rail} / ${restored.inspector} / ${restored.conv}`);

  // The pick survives a reload: the next review starts in the project the human works in.
  await page.goto(`${BASE}/`);
  await page.waitForSelector(".gallery .card", { timeout: 5000 });
  await page.click("#new-review");
  await page.waitForFunction(
    () => [...document.querySelectorAll("#f-project option")].some((option) => option.value === "e2e-repo"),
    { timeout: 5000 },
  );
  const remembered = await page.$eval("#f-project", (el) => el.value);
  check("a new review preselects the last used project after a reload",
    remembered === "e2e-repo", `project=${remembered}`);

  // A review the human creates without naming a branch must still belong to a feature: v0.21
  // fell back to `session/<id>` at claim time, which is not a branch anyone can merge.
  await page.evaluate(async () => {
    const canvas = document.createElement("canvas");
    canvas.width = 320;
    canvas.height = 200;
    canvas.getContext("2d").fillRect(0, 0, 320, 200);
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
    const data = new DataTransfer();
    data.items.add(new File([blob], "image.png", { type: "image/png" }));
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: data });
    window.dispatchEvent(event);
  });
  await page.waitForFunction(() => document.querySelector("#f-title").value === "Pasted screenshot", { timeout: 5000 });
  await page.$eval("#f-title", (el) => {
    el.value = "Branch check";
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const suggestion = await page.$eval("#f-branch", (el) => el.placeholder);
  check("the branch field suggests feat/<title> before it is typed",
    suggestion === "feat/branch-check", suggestion);
  await page.select("#f-project", "e2e");
  await page.click("#new-submit");
  await page.waitForFunction(() => window.redline?.meta?.branch, { timeout: 20000 });
  const newBranch = await page.evaluate(() => window.redline.meta.branch);
  check("a review created without a branch runs on feat/<title>, not session/<id>",
    /^feat\/branch-check-[0-9a-f]{4}$/.test(newBranch), newBranch);
  const railBranch = await page
    .$eval('.session[aria-current="true"] .sub .branch', (el) => el.textContent.trim())
    .catch(() => "");
  check("the rail shows that feature branch", railBranch === newBranch, `${railBranch} vs ${newBranch}`);

  check("no uncaught JS errors", jsErrors.length === 0, jsErrors.join("; "));

  await browser.close();
  const failed = results.filter((r) => !r.ok);
  console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
  process.exit(failed.length ? 1 : 0);
};

main().catch((error) => {
  console.error("e2e failed:", error);
  process.exit(1);
});
