import test from "node:test";
import assert from "node:assert/strict";
import { applyTheme } from "../src/lib/theme.js";

function fakeDocument() {
  const button = {
    innerHTML: "",
    title: "",
    setAttribute(name, value) {
      this[name] = value;
    },
  };
  const documentElement = { dataset: {}, style: {} };
  globalThis.document = {
    documentElement,
    querySelectorAll: (selector) => (selector === "[data-theme-toggle]" ? [button] : []),
  };
  return { button, documentElement };
}

test("the theme toggle offers the scheme it switches to", () => {
  const { button, documentElement } = fakeDocument();
  try {
    applyTheme("light");
    assert.equal(documentElement.dataset.theme, "light");
    assert.equal(documentElement.style.colorScheme, "light");
    assert.match(button.innerHTML, /<svg/);
    assert.equal(button.title, "Switch to dark theme");
    assert.equal(button["aria-label"], "Switch to dark theme");

    applyTheme("dark");
    assert.equal(documentElement.dataset.theme, "dark");
    assert.equal(button.title, "Switch to light theme");
    assert.notEqual(button.innerHTML, "");
  } finally {
    delete globalThis.document;
  }
});
