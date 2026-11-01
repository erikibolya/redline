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
const projectSelect = $("project");
const branchInput = $("branch");
const statusEl = $("status");
const saveButton = $("save");
const testButton = $("test");
const openLink = $("open-redline");

function setStatus(text, kind = "") {
  statusEl.textContent = text;
  statusEl.className = `status ${kind}`.trim();
}

function values() {
  return {
    serverUrl: normalizeServerUrl(serverInput.value),
    token: tokenInput.value.trim(),
    project: projectSelect.value,
    branch: branchInput.value.trim(),
  };
}

async function requestAccess(config) {
  const granted = await ensurePermission(config.serverUrl);
  if (!granted) throw new Error("Access to the RedLine server was not granted");
}

async function loadProjects(config) {
  const api = new RedLineApi(config);
  const data = await api.projects();
  const current = projectSelect.value || config.project || "";
  projectSelect.innerHTML = '<option value="">Inbox</option>';
  for (const project of data.projects || []) {
    const option = document.createElement("option");
    option.value = project.slug;
    option.textContent = project.name || project.slug;
    projectSelect.append(option);
  }
  projectSelect.value = current;
}

async function testConnection() {
  testButton.disabled = true;
  setStatus("Testing…");
  try {
    const config = values();
    await requestAccess(config);
    const api = new RedLineApi(config);
    const health = await api.health();
    await loadProjects(config);
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
    const config = values();
    if (!config.token) throw new Error("Token is required");
    await requestAccess(config);
    const saved = await saveConfig(config);
    openLink.href = saved.serverUrl;
    await loadProjects(saved).catch(() => {});
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
  branchInput.value = config.branch;
  openLink.href = config.serverUrl;
  projectSelect.innerHTML = '<option value="">Inbox</option>';
  if (config.project) {
    const option = document.createElement("option");
    option.value = config.project;
    option.textContent = config.project;
    projectSelect.append(option);
  }
  if (config.token) {
    await loadProjects(config).catch(() => {});
    projectSelect.value = config.project || "";
  }
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
init();
