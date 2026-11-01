import test from "node:test";
import assert from "node:assert/strict";
import {
  annotationBounds,
  createAnnotation,
  finishAnnotation,
  hitTest,
  moveAnnotation,
  TOOLS,
} from "../src/lib/annotations.js";

test("the core tool set is the one the side panel renders", () => {
  assert.deepEqual(TOOLS, ["select", "rect", "arrow", "pen", "text"]);
});

test("a rectangle gets bounds, hit-tests inside and moves as one shape", () => {
  const rect = createAnnotation("rect", { x: 10, y: 20 }, { color: "#fff", width: 3 });
  finishAnnotation(rect, { x: 110, y: 70 });
  assert.deepEqual(annotationBounds(rect), { x: 10, y: 20, width: 100, height: 50 });
  assert.equal(hitTest([rect], { x: 50, y: 40 })?.id, rect.id);
  assert.equal(hitTest([rect], { x: 200, y: 200 }), null);
  moveAnnotation(rect, 5, -5);
  assert.deepEqual(rect.start, { x: 15, y: 15 });
  assert.deepEqual(rect.end, { x: 115, y: 65 });
});

test("an arrow hit-tests its line, not its bounding box", () => {
  const arrow = createAnnotation("arrow", { x: 0, y: 0 });
  finishAnnotation(arrow, { x: 100, y: 100 });
  assert.equal(hitTest([arrow], { x: 50, y: 50 })?.id, arrow.id);
  assert.equal(hitTest([arrow], { x: 90, y: 10 }), null);
});

test("a pen keeps its points and hit-tests the stroke", () => {
  const pen = createAnnotation("pen", { x: 0, y: 0 });
  finishAnnotation(pen, { x: 0, y: 0 }, [
    { x: 0, y: 0 },
    { x: 30, y: 0 },
    { x: 30, y: 30 },
  ]);
  assert.equal(pen.points.length, 3);
  assert.equal(hitTest([pen], { x: 15, y: 1 })?.id, pen.id);
  assert.equal(hitTest([pen], { x: 0, y: 60 }), null);
});

test("a text note has a usable bounds rectangle", () => {
  const text = createAnnotation("text", { x: 100, y: 100 });
  text.text = "the total is stale";
  const bounds = annotationBounds(text);
  assert.equal(bounds.x, 100);
  assert.ok(bounds.width > 60);
  assert.equal(hitTest([text], { x: 110, y: 95 })?.id, text.id);
});
