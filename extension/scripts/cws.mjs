#!/usr/bin/env node
/**
 * Publish the extension to the Chrome Web Store (API v2) without third-party actions.
 *
 * The store cannot create the first listing through the API: publish 0.7.x once by hand, then
 * this script uploads and submits every later version. It needs a Google Cloud service account
 * that is listed under Account in the Chrome Web Store developer dashboard:
 *
 *   CWS_PUBLISHER_ID           publisher id (Dashboard -> Publisher -> Settings)
 *   CWS_SERVICE_ACCOUNT_JSON   the service account key JSON, or
 *   CWS_SERVICE_ACCOUNT_FILE   a path to that JSON
 *   CWS_EXTENSION_ID           optional; defaults to extension/store/cws.json
 *
 * Commands:
 *   status    show what the store currently has published/submitted
 *   release   upload the packaged ZIP and submit it for review when the manifest is newer
 *   upload    upload only (no review submission)
 *   publish   submit the last uploaded package for review
 *
 * Flags:
 *   --dry-run   print what would happen and exit without changing the store
 */
import { createSign } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const API = "https://chromewebstore.googleapis.com";
const TOKEN_URL = "https://oauth2.googleapis.com/token";
const SCOPE = "https://www.googleapis.com/auth/chromewebstore";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ACCEPTED_SUBMISSION_STATES = new Set(["PENDING_REVIEW", "STAGED"]);

/** Compare dotted Chrome extension versions numerically (0.7.10 is newer than 0.7.9). */
export function compareVersions(a, b) {
  const left = String(a).split(".").map((part) => Number.parseInt(part, 10) || 0);
  const right = String(b).split(".").map((part) => Number.parseInt(part, 10) || 0);
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) return difference < 0 ? -1 : 1;
  }
  return 0;
}

/** The newest crxVersion in a revision's distribution channels, or null when there is none. */
export function versionFromRevision(revision) {
  let newest = null;
  for (const channel of revision?.distributionChannels ?? []) {
    const version = channel?.crxVersion;
    if (version && (newest === null || compareVersions(version, newest) > 0)) newest = version;
  }
  return newest;
}

/** The submission (review queue) view of fetchStatus, or null when nothing is queued. */
export function submittedFromStatus(status) {
  const revision = status?.submittedItemRevisionStatus;
  const version = versionFromRevision(revision);
  return version ? { version, state: revision.state } : null;
}

/**
 * Decide what a release should do. A version that is live needs no upload; a version already in
 * review must not be uploaded twice. A rejected or cancelled submission is retried.
 */
export function planUpdate({ manifestVersion, published, submitted }) {
  if (published && compareVersions(published, manifestVersion) === 0) {
    return { action: "skip", reason: `${manifestVersion} is already published` };
  }
  if (published && compareVersions(published, manifestVersion) > 0) {
    return {
      action: "error",
      reason: `the store is already at ${published}, newer than the manifest ${manifestVersion}`,
    };
  }
  if (submitted) {
    if (compareVersions(submitted.version, manifestVersion) > 0) {
      return {
        action: "error",
        reason: `the store already has ${submitted.version} submitted, newer than ${manifestVersion}`,
      };
    }
    if (
      compareVersions(submitted.version, manifestVersion) === 0 &&
      ACCEPTED_SUBMISSION_STATES.has(submitted.state)
    ) {
      return {
        action: "skip",
        reason: `${manifestVersion} is already submitted (${submitted.state})`,
      };
    }
  }
  return {
    action: "upload",
    reason: published
      ? `the store has ${published}; submitting ${manifestVersion}`
      : `the store has no published version; submitting ${manifestVersion}`,
  };
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function loadConfig() {
  const store = readJson(resolve(ROOT, "store/cws.json"));
  const publisherId = process.env.CWS_PUBLISHER_ID || store.publisherId;
  const extensionId = process.env.CWS_EXTENSION_ID || store.extensionId;
  if (!publisherId) {
    throw new Error(
      "CWS_PUBLISHER_ID is required (Developer Dashboard -> Publisher -> Settings)",
    );
  }
  if (!extensionId) {
    throw new Error("CWS_EXTENSION_ID is required, or set extensionId in extension/store/cws.json");
  }
  const manifest = readJson(resolve(ROOT, "manifest.json"));
  return {
    publisherId,
    extensionId,
    manifestVersion: manifest.version,
    zipPath: resolve(ROOT, "..", "dist", `redline-extension-${manifest.version}.zip`),
    name: `publishers/${publisherId}/items/${extensionId}`,
  };
}

function loadServiceAccount() {
  if (process.env.CWS_SERVICE_ACCOUNT_JSON) {
    return JSON.parse(process.env.CWS_SERVICE_ACCOUNT_JSON);
  }
  if (process.env.CWS_SERVICE_ACCOUNT_FILE) {
    return readJson(process.env.CWS_SERVICE_ACCOUNT_FILE);
  }
  throw new Error("CWS_SERVICE_ACCOUNT_JSON or CWS_SERVICE_ACCOUNT_FILE is required");
}

async function accessToken(serviceAccount) {
  if (!serviceAccount.client_email || !serviceAccount.private_key) {
    throw new Error("the service account JSON has no client_email/private_key");
  }
  const now = Math.floor(Date.now() / 1000);
  const header = Buffer.from(JSON.stringify({ alg: "RS256", typ: "JWT" })).toString("base64url");
  const claims = Buffer.from(
    JSON.stringify({
      iss: serviceAccount.client_email,
      scope: SCOPE,
      aud: TOKEN_URL,
      iat: now - 30,
      exp: now + 3600,
    }),
  ).toString("base64url");
  const unsigned = `${header}.${claims}`;
  const signature = createSign("RSA-SHA256")
    .update(unsigned)
    .sign(serviceAccount.private_key)
    .toString("base64url");
  const response = await fetch(TOKEN_URL, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
      assertion: `${unsigned}.${signature}`,
    }),
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`token exchange failed: ${response.status} ${text.slice(0, 400)}`);
  }
  const payload = JSON.parse(text);
  if (!payload.access_token) throw new Error("token exchange returned no access_token");
  return payload.access_token;
}

async function request(token, path, init = {}) {
  const response = await fetch(`${API}${path}`, {
    ...init,
    headers: {
      authorization: `Bearer ${token}`,
      ...(init.body && !init.headers?.["content-type"]
        ? { "content-type": "application/json" }
        : {}),
      ...init.headers,
    },
  });
  const text = await response.text();
  let payload = null;
  try {
    payload = text ? JSON.parse(text) : null;
  } catch {
    payload = null;
  }
  if (!response.ok) {
    const detail = payload ? JSON.stringify(payload) : text.slice(0, 500);
    throw new Error(`${init.method ?? "GET"} ${path} failed: ${response.status} ${detail}`);
  }
  return payload;
}

async function fetchStatus(token, name) {
  return request(token, `/v2/${name}:fetchStatus`);
}

async function currentState(token, config) {
  const status = await fetchStatus(token, config.name);
  return {
    status,
    published: versionFromRevision(status.publishedItemRevisionStatus),
    submitted: submittedFromStatus(status),
  };
}

function printState({ status, published, submitted }) {
  console.log(`published: ${published ?? "(none)"}`);
  console.log(
    `submitted: ${submitted ? `${submitted.version} (${submitted.state})` : "(none)"}`,
  );
  if (status.lastAsyncUploadState) console.log(`upload: ${status.lastAsyncUploadState}`);
  if (status.takenDown) console.log("taken down: yes — check the developer dashboard");
  if (status.warned) console.log("warning: yes — check the developer dashboard");
}

async function upload(token, config) {
  console.log(`uploading ${config.zipPath}`);
  let body;
  try {
    body = readFileSync(config.zipPath);
  } catch {
    throw new Error(`package not found: ${config.zipPath} (run extension/scripts/package.py)`);
  }
  const result = await request(token, `/upload/v2/${config.name}:upload`, {
    method: "POST",
    headers: { "content-type": "application/zip" },
    body,
  });
  console.log(`upload state: ${result?.uploadState ?? "(unknown)"}`);
  if (result?.uploadState === "SUCCESS") return;
  if (result?.uploadState !== "IN_PROGRESS") {
    throw new Error(`upload failed: ${JSON.stringify(result)}`);
  }
  for (let attempt = 0; attempt < 60; attempt += 1) {
    await new Promise((done) => setTimeout(done, 2000));
    const status = await fetchStatus(token, config.name);
    console.log(`upload state: ${status.lastAsyncUploadState ?? "(unknown)"}`);
    if (status.lastAsyncUploadState === "SUCCESS") return;
    if (status.lastAsyncUploadState !== "IN_PROGRESS") {
      throw new Error(`upload failed: ${status.lastAsyncUploadState}`);
    }
  }
  throw new Error("upload did not finish within two minutes");
}

async function publish(token, config) {
  const result = await request(token, `/v2/${config.name}:publish`, {
    method: "POST",
    body: JSON.stringify({ publishType: "DEFAULT_PUBLISH" }),
  });
  console.log(`submitted: ${result?.state ?? "(unknown)"}`);
  for (const warning of result?.warningInfo?.warnings ?? []) {
    console.log(`warning: ${warning.reason}: ${warning.description}`);
  }
}

async function run() {
  const [command = "status", ...args] = process.argv.slice(2);
  const dryRun = args.includes("--dry-run");
  const config = loadConfig();

  if (command === "plan" || command === "status") {
    const token = await accessToken(loadServiceAccount());
    const state = await currentState(token, config);
    printState(state);
    const plan = planUpdate({ manifestVersion: config.manifestVersion, ...state });
    console.log(`manifest: ${config.manifestVersion}`);
    console.log(`plan: ${plan.action} — ${plan.reason}`);
    if (command === "plan" && plan.action === "error") process.exitCode = 1;
    return;
  }

  if (!["release", "upload", "publish"].includes(command)) {
    throw new Error(`unknown command "${command}" (status, plan, release, upload, publish)`);
  }

  const token = await accessToken(loadServiceAccount());
  const state = await currentState(token, config);
  printState(state);

  let plan = { action: "upload", reason: "upload requested" };
  if (command === "release") {
    plan = planUpdate({ manifestVersion: config.manifestVersion, ...state });
    console.log(`plan: ${plan.action} — ${plan.reason}`);
    if (plan.action === "skip") return;
    if (plan.action === "error") throw new Error(plan.reason);
  }
  if (dryRun) {
    console.log("dry run: no upload or publish was made");
    return;
  }

  if (command === "publish") {
    await publish(token, config);
    return;
  }

  await upload(token, config);
  if (command === "upload") return;
  await publish(token, config);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  run().catch((error) => {
    console.error(`cws: ${error.message}`);
    process.exitCode = 1;
  });
}
