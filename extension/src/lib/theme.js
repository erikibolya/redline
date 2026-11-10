/* Theme shared by the side panel and the in-page overlay. Extension pages share one origin, so
   localStorage keeps them in step; the overlay also receives an explicit set-theme command while
   it is open. The icons are the desktop editor's: the button shows the theme it switches to. */
const KEY = "redline:theme";

export const THEME_ICONS = {
  light: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z"/></svg>',
  dark: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.2 5.2l1.4 1.4M17.4 17.4l1.4 1.4M18.8 5.2l-1.4 1.4M6.6 17.4l-1.4 1.4"/></svg>',
};

export function currentTheme() {
  const applied = document.documentElement.dataset.theme;
  if (applied === "light" || applied === "dark") return applied;
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) || "null");
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    // A private window can refuse localStorage; the media query is still a sane default.
  }
  return matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

export function applyTheme(theme, { persist = true } = {}) {
  const value = theme === "light" ? "light" : "dark";
  document.documentElement.dataset.theme = value;
  if (persist) {
    try {
      localStorage.setItem(KEY, JSON.stringify(value));
    } catch {
      // The theme still applies for this page even when the preference cannot be stored.
    }
  }
  return value;
}

export function initTheme() {
  return applyTheme(currentTheme(), { persist: false });
}

export function toggleTheme() {
  return applyTheme(currentTheme() === "light" ? "dark" : "light");
}

export function watchTheme(onChange) {
  addEventListener("storage", (event) => {
    if (event.key !== KEY) return;
    let next = null;
    try {
      next = JSON.parse(event.newValue || "null");
    } catch {
      return;
    }
    if (next !== "light" && next !== "dark") return;
    applyTheme(next, { persist: false });
    onChange?.(next);
  });
}
