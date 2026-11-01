import { RedLineApi } from "./lib/api.js";

let active = null;

function mimeType() {
  const candidates = ["video/webm;codecs=vp9", "video/webm;codecs=vp8", "video/webm"];
  return (
    candidates.find((type) => globalThis.MediaRecorder?.isTypeSupported?.(type)) || "video/webm"
  );
}

async function posterFromBlob(blob, fallbackWidth, fallbackHeight) {
  const url = URL.createObjectURL(blob);
  const video = document.createElement("video");
  video.muted = true;
  video.playsInline = true;
  video.preload = "auto";
  video.src = url;
  try {
    await new Promise((resolve, reject) => {
      video.addEventListener("loadeddata", resolve, { once: true });
      video.addEventListener("error", () => reject(video.error || new Error("cannot read video")), {
        once: true,
      });
    });
    const width = video.videoWidth || fallbackWidth || 1280;
    const height = video.videoHeight || fallbackHeight || 720;
    if (video.videoWidth && video.videoHeight) {
      try {
        video.currentTime = 0.05;
        await new Promise((resolve) => {
          video.addEventListener("seeked", resolve, { once: true });
          setTimeout(resolve, 500);
        });
      } catch {
        // the first frame is still usable
      }
    }
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    canvas.getContext("2d").drawImage(video, 0, 0, width, height);
    return { poster: canvas.toDataURL("image/png"), width, height };
  } finally {
    URL.revokeObjectURL(url);
  }
}

async function startRecording({ streamId }) {
  if (active) throw new Error("A recording is already running");
  if (!streamId) throw new Error("No tab stream");
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: false,
    video: {
      mandatory: {
        chromeMediaSource: "tab",
        chromeMediaSourceId: streamId,
      },
    },
  });
  const chunks = [];
  const type = mimeType();
  const recorder = new MediaRecorder(stream, {
    mimeType: type,
    videoBitsPerSecond: 2_500_000,
  });
  recorder.addEventListener("dataavailable", (event) => {
    if (event.data?.size) chunks.push(event.data);
  });
  const settings = stream.getVideoTracks()[0]?.getSettings?.() || {};
  active = {
    stream,
    recorder,
    chunks,
    type,
    startedAt: Date.now(),
    width: settings.width || 1280,
    height: settings.height || 720,
    videoBlob: null,
    poster: null,
    durationMs: 0,
  };
  recorder.start(1000);
  return { startedAt: active.startedAt, mimeType: type, width: active.width, height: active.height };
}

async function stopRecording() {
  if (!active) throw new Error("No recording is running");
  const current = active;
  active = null;
  current.durationMs = Math.max(1, Date.now() - current.startedAt);
  if (current.recorder.state !== "inactive") {
    await new Promise((resolve) => {
      current.recorder.addEventListener("stop", resolve, { once: true });
      current.recorder.stop();
    });
  }
  for (const track of current.stream.getTracks()) track.stop();
  const videoBlob = new Blob(current.chunks, { type: current.type || "video/webm" });
  let poster = null;
  try {
    poster = await posterFromBlob(videoBlob, current.width, current.height);
  } catch {
    // A missing poster is not a reason to lose the clip; the rail falls back to a video card.
  }
  current.videoBlob = videoBlob;
  current.poster = poster?.poster || null;
  current.width = poster?.width || current.width;
  current.height = poster?.height || current.height;
  // Keep the stopped recording in a module-level slot until the upload message arrives. The
  // service worker never sees the video bytes.
  stopped = current;
  return {
    durationMs: current.durationMs,
    width: current.width,
    height: current.height,
    poster: current.poster,
    bytes: videoBlob.size,
  };
}

let stopped = null;

async function uploadRecording({ config, trace, title, url, note, project, branch }) {
  const current = stopped;
  if (!current) throw new Error("No stopped recording to upload");
  const api = new RedLineApi(config);
  const posterBlob = current.poster ? await (await fetch(current.poster)).blob() : null;
  const created = await api.uploadVideo({
    video: current.videoBlob,
    poster: posterBlob,
    trace,
    title,
    url,
    note,
    project,
    branch,
    durationMs: current.durationMs,
    width: current.width,
    height: current.height,
  });
  stopped = null;
  return created;
}

function discard() {
  if (active) {
    try {
      if (active.recorder.state !== "inactive") active.recorder.stop();
    } catch {
      // ignore
    }
    for (const track of active.stream.getTracks()) track.stop();
  }
  active = null;
  stopped = null;
}

const RECORDER_MESSAGES = new Set([
  "start-recording",
  "stop-recording",
  "upload-recording",
  "discard-recording",
]);

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!RECORDER_MESSAGES.has(message?.type)) return false;
  (async () => {
    try {
      if (message.type === "start-recording") {
        sendResponse({ ok: true, ...(await startRecording(message)) });
      } else if (message.type === "stop-recording") {
        sendResponse({ ok: true, ...(await stopRecording()) });
      } else if (message.type === "upload-recording") {
        sendResponse({ ok: true, ...(await uploadRecording(message)) });
      } else if (message.type === "discard-recording") {
        discard();
        sendResponse({ ok: true });
      }
    } catch (error) {
      sendResponse({ ok: false, error: error.message || String(error) });
    }
  })();
  return true;
});
