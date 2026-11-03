// The popup, options page and side panel share one theme preference. localStorage is synchronous,
// so the head bootstrap can paint the right scheme before the module loads; chrome.storage is
// asynchronous and would flash the wrong theme first.
const KEY = "redlineTheme";

/// The toggle shows the theme it switches *to*, not the current one (the same rule as the RedLine
/// app header). A glyph is not a promise: an emoji font may not have a sun at all.
export const THEME_ICONS = {
  light: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z"/></svg>',
  dark: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.2 5.2l1.4 1.4M17.4 17.4l1.4 1.4M18.8 5.2l-1.4 1.4M6.6 17.4l-1.4 1.4"/></svg>',
};

function schemeIsLight() {
  try {
    return matchMedia("(prefers-color-scheme: light)").matches;
  } catch {
    return false;
  }
}

export function preferredTheme() {
  try {
    const saved = localStorage.getItem(KEY);
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    // storage can be unavailable in a privacy-restricted profile; fall back to the OS
  }
  return schemeIsLight() ? "light" : "dark";
}

export function applyTheme(theme) {
  const value = theme === "light" ? "light" : "dark";
  document.documentElement.dataset.theme = value;
  document.documentElement.style.colorScheme = value;
  for (const button of document.querySelectorAll("[data-theme-toggle]")) {
    // Dark theme offers the sun (switch to light), light theme offers the moon.
    button.innerHTML = THEME_ICONS[value === "light" ? "light" : "dark"];
    const label = value === "light" ? "Switch to dark theme" : "Switch to light theme";
    button.title = label;
    button.setAttribute("aria-label", label);
  }
  return value;
}

export function setTheme(theme) {
  const value = applyTheme(theme);
  try {
    localStorage.setItem(KEY, value);
  } catch {
    // the toggle still works for this page even when the preference cannot persist
  }
  return value;
}

export function toggleTheme() {
  return setTheme(preferredTheme() === "light" ? "dark" : "light");
}
