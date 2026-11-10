import { RedLineApi } from "./lib/api.js";
import {
  ensurePermission,
  loadConfig,
  normalizeServerUrl,
  saveConfig,
} from "./lib/config.js";

const $ = (id) => document.getElementById(id);
const serverInput = $("server-url");
const tokenInput = $("token");
const statusEl = $("status");
const saveButton = $("save");
const testButton = $("test");
const openLink = $("open-redline");

function setStatus(text, kind = "") {
  statusEl.textContent = text;
  statusEl.className = `status ${kind}`.trim();
}

async function values() {
  const existing = await loadConfig();
  return {
    serverUrl: normalizeServerUrl(serverInput.value),
    token: tokenInput.value.trim(),
    // The connection form does not own the destination anymore; keep what is stored.
    project: existing.project || "",
    branch: existing.branch || "",
  };
}

async function requestAccess(config) {
  const granted = await ensurePermission(config.serverUrl);
  if (!granted) throw new Error("Access to the RedLine server was not granted");
}

async function testConnection() {
  testButton.disabled = true;
  setStatus("Testing…");
  try {
    const config = await values();
    await requestAccess(config);
    const api = new RedLineApi(config);
    const health = await api.health();
    await api.projects();
    setStatus(`Connected to RedLine ${health.version || ""}`.trim(), "ok");
  } catch (error) {
    setStatus(error.message || String(error), "err");
  } finally {
    testButton.disabled = false;
  }
}

async function save() {
  saveButton.disabled = true;
  setStatus("Saving…");
  try {
    const config = await values();
    if (!config.token) throw new Error("Token is required");
    await requestAccess(config);
    const saved = await saveConfig(config);
    openLink.href = saved.serverUrl;
    setStatus("Saved. You can capture a page now.", "ok");
  } catch (error) {
    setStatus(error.message || String(error), "err");
  } finally {
    saveButton.disabled = false;
  }
}

async function init() {
  const config = await loadConfig();
  serverInput.value = config.serverUrl;
  tokenInput.value = config.token;
  openLink.href = config.serverUrl;
}

saveButton.addEventListener("click", save);
testButton.addEventListener("click", testConnection);
serverInput.addEventListener("change", () => {
  try {
    openLink.href = normalizeServerUrl(serverInput.value);
  } catch {
    // keep the previous href; validation reports it on save/test
  }
});
init()
  .catch((error) => setStatus(error.message || String(error), "err"))
  .finally(() => {
    // Lets the extension e2e (and a human on a slow profile) know the stored config is loaded;
    // filling the form before this point could be overwritten by init().
    document.body.dataset.ready = "1";
  });
