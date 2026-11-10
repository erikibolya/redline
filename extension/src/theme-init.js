/* Apply the saved theme before the first paint. An inline script would be blocked by the
   extension CSP, and a module is deferred until after the stylesheet has painted. */
try {
  const saved = JSON.parse(localStorage.getItem("redline:theme") || "null");
  document.documentElement.dataset.theme =
    saved || (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");
} catch {
  document.documentElement.dataset.theme = "dark";
}
