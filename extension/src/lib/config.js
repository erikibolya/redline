const KEY = "redlineConfig";
const PROJECT_MEMORY_KEY = "redlineProjectMemory";

export const DEFAULT_CONFIG = Object.freeze({
  serverUrl: "https://redline.ibolya.cz",
  token: "",
  project: "",
  branch: "",
});

export async function loadConfig() {
  const stored = await chrome.storage.local.get(KEY);
  return { ...DEFAULT_CONFIG, ...(stored[KEY] || {}) };
}

export async function saveConfig(config) {
  const normalized = {
    serverUrl: normalizeServerUrl(config.serverUrl),
    token: String(config.token || "").trim(),
    project: String(config.project || "").trim(),
    branch: String(config.branch || "").trim(),
  };
  await chrome.storage.local.set({ [KEY]: normalized });
  return normalized;
}

export function normalizeServerUrl(value) {
  const text = String(value || "").trim().replace(/\/+$/, "");
  if (!text) throw new Error("Server URL is required");
  const url = new URL(text);
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error("Server URL must be http(s)");
  }
  return url.origin + (url.pathname === "/" ? "" : url.pathname.replace(/\/+$/, ""));
}

export function originPattern(serverUrl) {
  const url = new URL(normalizeServerUrl(serverUrl));
  return `${url.origin}/*`;
}

export async function ensurePermission(serverUrl) {
  const pattern = originPattern(serverUrl);
  if (await chrome.permissions.contains({ origins: [pattern] })) return true;
  return chrome.permissions.request({ origins: [pattern] });
}

export function isConfigured(config) {
  return Boolean(config && config.serverUrl && config.token);
}

/* The last destination is a per-page choice, not a global connection setting. The page origin
   wins; a new origin falls back to the project chosen last anywhere. The Inbox is an explicit
   empty string, so a human who chose Inbox is not silently moved back to an old project. */
function pageOrigin(pageUrl) {
  try {
    const url = new URL(String(pageUrl || ""));
    return /^https?:$/.test(url.protocol) ? url.origin : "";
  } catch {
    return "";
  }
}

export async function projectForCapture(pageUrl) {
  const stored = await chrome.storage.local.get(PROJECT_MEMORY_KEY);
  const memory = stored[PROJECT_MEMORY_KEY];
  if (!memory || typeof memory !== "object") return null;
  const byOrigin = memory.byOrigin && typeof memory.byOrigin === "object" ? memory.byOrigin : {};
  const origin = pageOrigin(pageUrl);
  if (origin && Object.prototype.hasOwnProperty.call(byOrigin, origin)) {
    return typeof byOrigin[origin] === "string" ? byOrigin[origin] : "";
  }
  return typeof memory.last === "string" ? memory.last : null;
}

export async function rememberProject(pageUrl, project) {
  const stored = await chrome.storage.local.get(PROJECT_MEMORY_KEY);
  const previous = stored[PROJECT_MEMORY_KEY];
  const memory = previous && typeof previous === "object" ? { ...previous } : {};
  const byOrigin =
    memory.byOrigin && typeof memory.byOrigin === "object" ? { ...memory.byOrigin } : {};
  const slug = String(project || "").trim();
  const origin = pageOrigin(pageUrl);
  if (origin) {
    // Re-insert so the most recently used origins are the ones kept when the map is capped.
    delete byOrigin[origin];
    byOrigin[origin] = slug;
  }
  const entries = Object.entries(byOrigin);
  memory.byOrigin = Object.fromEntries(entries.slice(-50));
  memory.last = slug;
  await chrome.storage.local.set({ [PROJECT_MEMORY_KEY]: memory });
}
