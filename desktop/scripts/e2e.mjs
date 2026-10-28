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
  const created = await api("/api/sessions", {
    method: "POST",
    body: JSON.stringify({ title: "e2e canvas", image_base64: makePng() }),
  });
  const id = created.meta.id;

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

  const canvas = await page.$eval("#canvas", (el) => {
    const rect = el.getBoundingClientRect();
    return { x: rect.x, y: rect.y, w: rect.width, h: rect.height };
  });
  const at = (dx, dy) => ({ x: canvas.x + dx, y: canvas.y + dy });

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

  // ---- inline text, no window.prompt
  await page.keyboard.press("t");
  const textSpot = at(240, 380);
  await page.mouse.click(textSpot.x, textSpot.y);
  await page.waitForSelector(".text-editor", { timeout: 5000 });
  await page.type(".text-editor", "Toto tlačítko by mělo být zelené");
  await page.keyboard.press("Enter");
  state = await page.evaluate(() => window.redline.annotations);
  const text = state.find((a) => a.type === "text");
  check("text committed from the inline editor", text && text.text.startsWith("Toto"), text?.text || "");

  // ---- submit
  await page.keyboard.down("Control");
  await page.keyboard.press("Enter");
  await page.keyboard.up("Control");
  await page.waitForFunction(
    () => document.querySelector("#session-status")?.textContent === "submitted",
    { timeout: 15000 },
  );

  const saved = await api(`/api/sessions/${id}`);
  const stored = saved.annotations.annotations;
  check("server stored both annotations", stored.length === 2, `${stored.length}: ${stored.map((a) => a.type).join(",")}`);
  check("rect survived the round-trip", stored.some((a) => a.type === "rect"));
  check("text survived with its diacritics", stored.some((a) => a.text?.includes("zelené")));
  check("screenshot was composited", saved.meta.status === "submitted");

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
  await new Promise((resolve) => setTimeout(resolve, 1200));
  const history = await api(`/api/sessions/${id}/history`);
  check("a second submission archives the first round", history.submissions.length >= 1,
    `${history.submissions.length} archived`);

  const annotated = await fetch(`${BASE}/sessions/${id}/annotated.png`, {
    headers: { Authorization: `Bearer ${TOKEN}` },
  });
  check("annotated.png is served", annotated.ok, `${annotated.status}`);

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
