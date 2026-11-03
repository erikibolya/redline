export const DEFAULT_REDACTION = Object.freeze({
  headers: ["authorization", "cookie", "set-cookie", "x-api-key", "proxy-authorization"],
  query: ["token", "key", "secret", "password", "code", "session"],
  bodies: false,
});

const SECRET_QUERY = /(?:^|[?&])(token|key|secret|password|code|session)=([^&#]*)/gi;
const BEARER = /\bBearer\s+[A-Za-z0-9._~+/-]+=*/g;
const ASSIGNMENT = /\b(token|key|secret|password|code|session)=([^\s&,;]+)/gi;

export function redactText(value, max = 2048) {
  let text = String(value ?? "");
  text = text.replace(BEARER, "Bearer [redacted]");
  text = text.replace(ASSIGNMENT, "$1=[redacted]");
  if (text.length > max) text = `${text.slice(0, max - 1)}…`;
  return text;
}

export function redactUrl(raw) {
  const value = String(raw || "");
  try {
    const url = new URL(value);
    for (const key of [...url.searchParams.keys()]) {
      const lower = key.toLowerCase();
      if (
        DEFAULT_REDACTION.query.some(
          (secret) =>
            lower === secret || lower.endsWith(`_${secret}`) || lower.endsWith(`-${secret}`),
        )
      ) {
        url.searchParams.set(key, "[redacted]");
      }
    }
    const out = url.toString();
    return out.length > 4096 ? `${out.slice(0, 4095)}…` : out;
  } catch {
    const out = value.replace(SECRET_QUERY, "$1=[redacted]");
    return out.length > 4096 ? `${out.slice(0, 4095)}…` : out;
  }
}

export function consoleText(args = []) {
  const parts = [];
  for (const arg of args) {
    if (arg == null) continue;
    if (typeof arg === "string" || typeof arg === "number" || typeof arg === "boolean") {
      parts.push(String(arg));
    } else if (arg.value !== undefined) {
      parts.push(String(arg.value));
    } else if (arg.description) {
      parts.push(String(arg.description));
    } else if (arg.type) {
      parts.push(String(arg.type));
    }
  }
  return redactText(parts.join(" "));
}

export function makeTrace({ startedAt, durationMs, page = {}, events = [], truncated = false }) {
  return {
    version: 1,
    kind: "browser-flow",
    started_at: new Date(startedAt).toISOString(),
    duration_ms: Math.max(1, Math.round(durationMs)),
    page: { url: redactUrl(page.url || ""), title: redactText(page.title || "", 512) },
    browser: {
      name: "Chrome",
      version: (globalThis.navigator?.userAgent || "").slice(0, 512),
      platform: globalThis.navigator?.platform || "",
    },
    redaction: {
      headers: [...DEFAULT_REDACTION.headers],
      query: [...DEFAULT_REDACTION.query],
      bodies: DEFAULT_REDACTION.bodies,
    },
    truncated: Boolean(truncated),
    events,
  };
}
