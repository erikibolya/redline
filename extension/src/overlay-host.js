/**
 * Injected into the active tab by the side panel. It owns one fixed iframe; the editor lives in
 * the extension origin, so page CSS and page scripts cannot reach the token or the canvas.
 *
 * The content script only creates and removes the host. Tool commands and state travel over
 * chrome.runtime messages between the side panel and the overlay iframe.
 */
(() => {
  if (globalThis.__redlineOverlayHostInstalled) return;
  globalThis.__redlineOverlayHostInstalled = true;

  const HOST_ID = "redline-overlay-host";
  const state = {
    sessionId: null,
    host: null,
    iframe: null,
    previousHtmlOverflow: "",
    previousBodyOverflow: "",
  };

  function restoreScroll() {
    document.documentElement.style.overflow = state.previousHtmlOverflow;
    document.body.style.overflow = state.previousBodyOverflow;
  }

  function onKey(event) {
    if (!state.host) return;
    event.stopPropagation();
    event.preventDefault();
    if (event.key === "Escape") {
      chrome.runtime
        .sendMessage({ type: "redline-overlay-escape", sessionId: state.sessionId })
        .catch(() => {});
    }
  }

  function remove() {
    if (state.host) state.host.remove();
    state.host = null;
    state.iframe = null;
    state.sessionId = null;
    document.removeEventListener("keydown", onKey, true);
    restoreScroll();
  }

  function open(sessionId) {
    if (state.host && state.sessionId === sessionId) {
      state.iframe?.focus();
      return;
    }
    remove();
    state.sessionId = sessionId;
    state.previousHtmlOverflow = document.documentElement.style.overflow;
    state.previousBodyOverflow = document.body.style.overflow;
    document.documentElement.style.overflow = "hidden";
    document.body.style.overflow = "hidden";

    const host = document.createElement("div");
    host.id = HOST_ID;
    host.dataset.sessionId = sessionId;
    host.style.cssText = [
      "position:fixed",
      "inset:0",
      "z-index:2147483647",
      "background:#0f172a",
      "display:block",
      "margin:0",
      "padding:0",
    ].join(";");

    const iframe = document.createElement("iframe");
    iframe.src = chrome.runtime.getURL(
      `src/overlay.html#session=${encodeURIComponent(sessionId)}`,
    );
    iframe.title = "RedLine annotation overlay";
    iframe.setAttribute("allowtransparency", "false");
    iframe.style.cssText = [
      "width:100%",
      "height:100%",
      "border:0",
      "display:block",
      "background:#0f172a",
    ].join(";");
    iframe.addEventListener("load", () => iframe.focus());

    host.append(iframe);
    document.documentElement.append(host);
    state.host = host;
    state.iframe = iframe;
    document.addEventListener("keydown", onKey, true);
  }

  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (message?.type === "redline-overlay-open") {
      open(message.sessionId);
      sendResponse({ ok: true, open: true, sessionId: state.sessionId });
    } else if (message?.type === "redline-overlay-close") {
      if (!message.sessionId || message.sessionId === state.sessionId) remove();
      sendResponse({ ok: true, open: Boolean(state.host) });
    } else if (message?.type === "redline-overlay-status") {
      sendResponse({ ok: true, open: Boolean(state.host), sessionId: state.sessionId });
    }
    return false;
  });
})();
