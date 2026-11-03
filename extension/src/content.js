(() => {
  if (globalThis.__redlineCapture) return;
  globalThis.__redlineCapture = true;

  const listeners = [];

  function cssEscape(value) {
    return globalThis.CSS?.escape ? CSS.escape(value) : String(value).replace(/["\\]/g, "\\$&");
  }

  function selectorFor(element) {
    if (!(element instanceof Element)) return "";
    const testId = element.getAttribute("data-testid") || element.getAttribute("data-test-id");
    if (testId) return `[data-testid="${cssEscape(testId)}"]`;
    if (element.id) return `#${cssEscape(element.id)}`;
    const aria = element.getAttribute("aria-label");
    if (aria) return `${element.tagName.toLowerCase()}[aria-label="${cssEscape(aria)}"]`;
    const name = element.getAttribute("name");
    if (name) return `${element.tagName.toLowerCase()}[name="${cssEscape(name)}"]`;
    const parts = [];
    let current = element;
    for (let depth = 0; current && depth < 5; depth += 1) {
      let part = current.tagName.toLowerCase();
      const parent = current.parentElement;
      if (parent) {
        const same = [...parent.children].filter((child) => child.tagName === current.tagName);
        if (same.length > 1) part += `:nth-of-type(${same.indexOf(current) + 1})`;
      }
      parts.unshift(part);
      if (current.id) {
        parts[0] = `#${cssEscape(current.id)}`;
        break;
      }
      current = parent;
    }
    return parts.join(" > ");
  }

  function labelFor(element) {
    if (!(element instanceof Element)) return "";
    const text =
      element.getAttribute("aria-label") ||
      element.getAttribute("alt") ||
      element.getAttribute("title") ||
      element.getAttribute("placeholder") ||
      element.textContent ||
      "";
    return String(text).replace(/\s+/g, " ").trim().slice(0, 120);
  }

  function interactiveTarget(target) {
    if (!(target instanceof Element)) return null;
    return target.closest(
      'button, a, input, select, textarea, label, form, [role="button"], [role="link"], [role="menuitem"], [contenteditable="true"]',
    );
  }

  function actionFor(kind, event) {
    const element = interactiveTarget(event.target);
    if (!element) return null;
    const tag = element.tagName.toLowerCase();
    const action = {
      t: Date.now(),
      kind: "action",
      action: kind,
      selector: selectorFor(element),
      tag,
      role: element.getAttribute("role") || "",
      label: labelFor(element),
    };
    if (kind === "change" && (tag === "select" || tag === "textarea" || tag === "input")) {
      const type = (element.getAttribute("type") || "").toLowerCase();
      if (tag === "select" || type === "checkbox" || type === "radio") {
        action.value = element.value || (element.checked ? "checked" : "unchecked");
      } else {
        action.value = type === "password" ? "[redacted]" : `[${element.value.length} chars]`;
      }
    }
    return action;
  }

  function add(type, handler, options = {}) {
    window.addEventListener(type, handler, options);
    listeners.push(() => window.removeEventListener(type, handler, options));
  }

  add(
    "click",
    (event) => {
      const action = actionFor("click", event);
      if (action) chrome.runtime.sendMessage({ type: "redline-action", action }).catch(() => {});
    },
    true,
  );
  add(
    "change",
    (event) => {
      const action = actionFor("change", event);
      if (action) chrome.runtime.sendMessage({ type: "redline-action", action }).catch(() => {});
    },
    true,
  );
  add(
    "submit",
    (event) => {
      const action = actionFor("submit", event);
      if (action) chrome.runtime.sendMessage({ type: "redline-action", action }).catch(() => {});
    },
    true,
  );

  chrome.runtime.onMessage.addListener((message) => {
    if (message?.type === "redline-stop") {
      for (const remove of listeners.splice(0)) remove();
      delete globalThis.__redlineCapture;
    }
  });
})();
