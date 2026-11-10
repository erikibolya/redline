/** Review package generation for offline captures: Markdown, RedLine JSON and a store-only ZIP. */

export function timecode(seconds) {
  const value = Math.max(0, Number(seconds) || 0);
  const minutes = Math.floor(value / 60);
  const rest = value - minutes * 60;
  return `${String(minutes).padStart(2, "0")}:${rest.toFixed(3).padStart(6, "0")}`;
}

export function frameFileName(at) {
  return `frames/${Number(at || 0).toFixed(3)}.png`;
}

export function traceSummary(trace) {
  const events = Array.isArray(trace?.events) ? trace.events : [];
  const summary = {
    console: 0,
    console_errors: 0,
    network: 0,
    network_failed: 0,
    actions: 0,
    markers: 0,
  };
  const highlights = [];
  const addHighlight = (t, kind, text) => {
    if (highlights.length >= 8) return;
    const bounded = String(text || "");
    highlights.push({
      t: Math.max(0, Math.round(Number(t) || 0)),
      kind,
      text: bounded.length > 240 ? `${bounded.slice(0, 239)}…` : bounded,
    });
  };
  for (const event of events) {
    const kind = String(event.kind || "");
    if (kind === "console") {
      summary.console += 1;
      if (event.level === "error") {
        summary.console_errors += 1;
        addHighlight(event.t, "console", event.text);
      }
    } else if (kind === "exception") {
      summary.console += 1;
      summary.console_errors += 1;
      addHighlight(event.t, "exception", event.text);
    } else if (kind === "network") {
      summary.network += 1;
      const failed = event.failed || Number(event.status) >= 400;
      if (failed) {
        summary.network_failed += 1;
        addHighlight(
          event.t,
          "network",
          `${event.method || "GET"} ${event.url || ""} → ${
            event.status ? event.status : "failed"
          }`,
        );
      }
    } else if (kind === "action") {
      summary.actions += 1;
    } else if (kind === "marker") {
      summary.markers += 1;
    }
  }
  return { summary, highlights };
}

function annotationLine(annotation, index) {
  const at = annotation.at != null ? ` @ ${timecode(annotation.at)}` : "";
  if (annotation.type === "text") {
    return `${index}. **Text note**${at} — ${annotation.text || "(empty)"}`;
  }
  if (annotation.type === "pen") {
    return `${index}. **Pen**${at} — ${(annotation.points || []).length} points`;
  }
  if (annotation.type === "arrow") {
    const start = annotation.start || {};
    const end = annotation.end || {};
    return `${index}. **Arrow**${at} — (${Math.round(start.x || 0)}, ${Math.round(
      start.y || 0,
    )}) → (${Math.round(end.x || 0)}, ${Math.round(end.y || 0)})`;
  }
  if (annotation.type === "rect" || annotation.type === "ellipse") {
    const start = annotation.start || {};
    const end = annotation.end || {};
    const label = annotation.type === "ellipse" ? "Ellipse" : "Rectangle";
    return `${index}. **${label}**${at} — (${Math.round(start.x || 0)}, ${Math.round(
      start.y || 0,
    )}) → (${Math.round(end.x || 0)}, ${Math.round(end.y || 0)})`;
  }
  return `${index}. **${annotation.type || "annotation"}**${at}`;
}

export function buildReviewMarkdown({
  id,
  title,
  url,
  note,
  media,
  width,
  height,
  annotations = [],
  frames = [],
  trace = null,
}) {
  const lines = [`# Visual review: ${title || "Untitled"}`, ""];
  if (url) lines.push(`- Page: ${url}`);
  if (note) lines.push(`- Note: ${note}`);
  lines.push(`- Session: ${id}`);
  lines.push(`- Status: local export`);
  lines.push(`- Exported: ${new Date().toISOString()}`);
  if (media) {
    const parts = [`- Media: ${media.kind}`];
    if (media.duration_ms) parts.push(`duration ${timecode(media.duration_ms / 1000)}`);
    if (width && height) parts.push(`${width}×${height}`);
    lines.push(parts.join(", "));
  } else if (width && height) {
    lines.push(`- Image: ${width}×${height}`);
  }
  if (frames.length) {
    lines.push(`- Annotated frames: ${frames.map((frame) => timecode(frame.at)).join(", ")}`);
  }
  lines.push("");

  if (trace) {
    const { summary, highlights } = traceSummary(trace);
    lines.push("## Browser trace", "");
    lines.push(
      `- ${timecode((trace.duration_ms || 0) / 1000)} · ${summary.actions} actions · ${
        summary.network
      } requests (${summary.network_failed} failed) · ${summary.console} console messages (${
        summary.console_errors
      } errors) · ${summary.markers} markers${trace.truncated ? " · truncated" : ""}`,
    );
    if (highlights.length) {
      lines.push("", "### Trace highlights", "");
      for (const highlight of highlights) {
        lines.push(
          `- @ ${timecode(highlight.t / 1000)} ${highlight.kind} — ${highlight.text}`,
        );
      }
    }
    lines.push("");
  }

  lines.push("## Annotations", "");
  if (!annotations.length) {
    lines.push("_No annotations._");
  } else {
    annotations.forEach((annotation, index) => {
      lines.push(annotationLine(annotation, index + 1));
    });
  }
  lines.push("");
  lines.push("## How to use this package", "");
  lines.push(
    "Attach this folder (or the ZIP) to your AI assistant. `annotations.json` has the structured annotations, `annotated.png` or `video.webm` is the visual, `trace.json` is console/network/action evidence, and `AI-PROMPT.md` has a ready prompt.",
  );
  return `${lines.join("\n")}\n`;
}

export function buildAnnotationsJson({
  id,
  title,
  url,
  note,
  media,
  width,
  height,
  annotations = [],
  frames = [],
  trace = null,
}) {
  const payload = {
    session: id,
    title,
    url,
    note,
    annotations,
    submitted_at: new Date().toISOString(),
  };
  if (media) {
    payload.media = media;
    payload.image = { file: "poster.png", width, height };
  } else {
    payload.image = { file: "annotated.png", width, height };
  }
  if (frames.length) {
    payload.frames = frames.map((frame) => ({
      at: Number(frame.at) || 0,
      file: frameFileName(frame.at),
      mime: "image/png",
    }));
  }
  if (trace) payload.trace = trace;
  return payload;
}

export const AI_PROMPT = `# RedLine review package

This folder contains a UI review captured in a browser.

- \`review.md\` — human-readable summary and annotations.
- \`annotations.json\` — structured annotations (type, timecode, coordinates, text, note).
- \`annotated.png\` — the screenshot with the annotations drawn on it.
- \`video.webm\`, \`poster.png\`, \`frames/\`, \`contact-sheet.png\` — for a recorded flow.
- \`trace.json\` — console, network and user-action events (metadata only; no request bodies or headers).

Please:

1. Read \`review.md\` and \`annotations.json\`.
2. Use the annotated image/video and the trace as evidence.
3. Implement the requested change or explain what is wrong.
4. Do not run git, deploy or merge unless explicitly asked.
`;

const ZIP_FLAGS = 0x0800; // UTF-8 file names
const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(bytes) {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i += 1) {
    c = CRC_TABLE[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  }
  return (c ^ 0xffffffff) >>> 0;
}

function u16(value) {
  const bytes = new Uint8Array(2);
  new DataView(bytes.buffer).setUint16(0, value & 0xffff, true);
  return bytes;
}

function u32(value) {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setUint32(0, value >>> 0, true);
  return bytes;
}

function dosDateTime(date = new Date()) {
  const year = Math.max(1980, date.getFullYear());
  return {
    time:
      ((date.getHours() & 0x1f) << 11) |
      ((date.getMinutes() & 0x3f) << 5) |
      ((date.getSeconds() & 0x3f) >> 1),
    date:
      (((year - 1980) & 0x7f) << 9) |
      (((date.getMonth() + 1) & 0x0f) << 5) |
      (date.getDate() & 0x1f),
  };
}

async function bytesOf(data) {
  if (data instanceof Uint8Array) return data;
  if (data instanceof Blob) return new Uint8Array(await data.arrayBuffer());
  return new TextEncoder().encode(String(data ?? ""));
}

/**
 * Store-only ZIP (no compression). Deliberately dependency-free: the extension has no build step.
 * `files` is `[{ name, data: string | Blob | Uint8Array }]`.
 */
export async function buildZip(files) {
  const parts = [];
  const central = [];
  const stamp = dosDateTime();
  let offset = 0;
  for (const file of files) {
    const name = new TextEncoder().encode(file.name);
    const data = await bytesOf(file.data);
    const crc = crc32(data);
    const header = [
      u32(0x04034b50),
      u16(20),
      u16(ZIP_FLAGS),
      u16(0),
      u16(stamp.time),
      u16(stamp.date),
      u32(crc),
      u32(data.length),
      u32(data.length),
      u16(name.length),
      u16(0),
    ];
    const headerLength = header.reduce((sum, part) => sum + part.length, 0) + name.length;
    parts.push(...header, name, data);
    central.push(
      u32(0x02014b50),
      u16(20),
      u16(20),
      u16(ZIP_FLAGS),
      u16(0),
      u16(stamp.time),
      u16(stamp.date),
      u32(crc),
      u32(data.length),
      u32(data.length),
      u16(name.length),
      u16(0),
      u16(0),
      u16(0),
      u16(0),
      u32(0),
      u32(offset),
      name,
    );
    offset += headerLength + data.length;
  }
  const centralOffset = offset;
  const centralSize = central.reduce((sum, part) => sum + part.length, 0);
  parts.push(...central);
  parts.push(
    u32(0x06054b50),
    u16(0),
    u16(0),
    u16(files.length),
    u16(files.length),
    u32(centralSize),
    u32(centralOffset),
    u16(0),
  );
  return new Blob(parts, { type: "application/zip" });
}
