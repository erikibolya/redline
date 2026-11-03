const KEY = "redlineConfig";

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
