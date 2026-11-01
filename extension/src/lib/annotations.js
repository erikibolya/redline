export const TOOLS = ["select", "rect", "arrow", "pen", "text"];

export function newId() {
  if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  return `ann-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function createAnnotation(type, start, style = {}) {
  const base = {
    id: newId(),
    type,
    color: style.color || "#e11d48",
    width: Number(style.width) || 3,
  };
  if (type === "text") return { ...base, x: start.x, y: start.y, text: "" };
  if (type === "pen") return { ...base, points: [start] };
  return { ...base, start, end: { ...start } };
}

export function finishAnnotation(annotation, end, points) {
  if (annotation.type === "pen") {
    annotation.points = points && points.length ? points : annotation.points;
  } else if (annotation.type !== "text") {
    annotation.end = { ...end };
  }
  return annotation;
}

export function annotationBounds(annotation) {
  if (!annotation) return null;
  if (annotation.type === "text") {
    const width = Math.max(60, String(annotation.text || "").length * 9 + 12);
    return { x: annotation.x, y: annotation.y - 16, width, height: 28 };
  }
  if (annotation.type === "pen") {
    const points = annotation.points || [];
    if (!points.length) return null;
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const point of points) {
      minX = Math.min(minX, point.x);
      minY = Math.min(minY, point.y);
      maxX = Math.max(maxX, point.x);
      maxY = Math.max(maxY, point.y);
    }
    return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
  }
  const start = annotation.start;
  const end = annotation.end;
  if (!start || !end) return null;
  return {
    x: Math.min(start.x, end.x),
    y: Math.min(start.y, end.y),
    width: Math.abs(end.x - start.x),
    height: Math.abs(end.y - start.y),
  };
}

function distanceToSegment(point, a, b) {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  if (dx === 0 && dy === 0) return Math.hypot(point.x - a.x, point.y - a.y);
  const t = Math.max(
    0,
    Math.min(1, ((point.x - a.x) * dx + (point.y - a.y) * dy) / (dx * dx + dy * dy)),
  );
  return Math.hypot(point.x - (a.x + t * dx), point.y - (a.y + t * dy));
}

export function hitTest(annotations, point) {
  for (let index = annotations.length - 1; index >= 0; index -= 1) {
    const annotation = annotations[index];
    if (annotation.type === "text") {
      const bounds = annotationBounds(annotation);
      if (
        bounds &&
        point.x >= bounds.x - 6 &&
        point.x <= bounds.x + bounds.width + 6 &&
        point.y >= bounds.y - 6 &&
        point.y <= bounds.y + bounds.height + 6
      ) {
        return annotation;
      }
    } else if (annotation.type === "pen") {
      const points = annotation.points || [];
      for (let i = 1; i < points.length; i += 1) {
        if (distanceToSegment(point, points[i - 1], points[i]) <= 8) return annotation;
      }
    } else if (annotation.type === "arrow") {
      if (distanceToSegment(point, annotation.start, annotation.end) <= 8) return annotation;
    } else {
      const bounds = annotationBounds(annotation);
      if (
        bounds &&
        point.x >= bounds.x - 5 &&
        point.x <= bounds.x + bounds.width + 5 &&
        point.y >= bounds.y - 5 &&
        point.y <= bounds.y + bounds.height + 5
      ) {
        return annotation;
      }
    }
  }
  return null;
}

export function moveAnnotation(annotation, dx, dy) {
  if (annotation.type === "text") {
    annotation.x += dx;
    annotation.y += dy;
  } else if (annotation.type === "pen") {
    for (const point of annotation.points || []) {
      point.x += dx;
      point.y += dy;
    }
  } else {
    annotation.start.x += dx;
    annotation.start.y += dy;
    annotation.end.x += dx;
    annotation.end.y += dy;
  }
  return annotation;
}

function drawArrowHead(ctx, start, end, color) {
  const angle = Math.atan2(end.y - start.y, end.x - start.x);
  const size = 13;
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.moveTo(end.x, end.y);
  ctx.lineTo(
    end.x - size * Math.cos(angle - Math.PI / 7),
    end.y - size * Math.sin(angle - Math.PI / 7),
  );
  ctx.lineTo(
    end.x - size * Math.cos(angle + Math.PI / 7),
    end.y - size * Math.sin(angle + Math.PI / 7),
  );
  ctx.closePath();
  ctx.fill();
}

function drawBadge(ctx, x, y, number) {
  ctx.save();
  ctx.beginPath();
  ctx.arc(x, y, 10, 0, Math.PI * 2);
  ctx.fillStyle = "#e11d48";
  ctx.fill();
  ctx.fillStyle = "#fff";
  ctx.font = "600 11px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(String(number), x, y + 0.5);
  ctx.restore();
}

export function drawAnnotation(ctx, annotation, number, selected = false) {
  if (!annotation) return;
  ctx.save();
  ctx.lineWidth = annotation.width || 3;
  ctx.strokeStyle = annotation.color || "#e11d48";
  ctx.fillStyle = annotation.color || "#e11d48";
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  if (annotation.type === "rect") {
    const { start, end } = annotation;
    ctx.strokeRect(start.x, start.y, end.x - start.x, end.y - start.y);
    drawBadge(ctx, start.x, start.y, number);
  } else if (annotation.type === "arrow") {
    const { start, end } = annotation;
    ctx.beginPath();
    ctx.moveTo(start.x, start.y);
    ctx.lineTo(end.x, end.y);
    ctx.stroke();
    drawArrowHead(ctx, start, end, annotation.color || "#e11d48");
    drawBadge(ctx, start.x, start.y, number);
  } else if (annotation.type === "pen") {
    const points = annotation.points || [];
    if (points.length) {
      ctx.beginPath();
      ctx.moveTo(points[0].x, points[0].y);
      for (const point of points.slice(1)) ctx.lineTo(point.x, point.y);
      ctx.stroke();
      drawBadge(ctx, points[0].x, points[0].y, number);
    }
  } else if (annotation.type === "text") {
    const text = annotation.text || "(note)";
    ctx.font = "600 16px system-ui, sans-serif";
    const width = Math.max(60, ctx.measureText(text).width + 16);
    ctx.fillStyle = "rgba(15, 23, 42, 0.82)";
    ctx.fillRect(annotation.x - 4, annotation.y - 20, width, 28);
    ctx.fillStyle = "#fff";
    ctx.textBaseline = "middle";
    ctx.fillText(text, annotation.x + 4, annotation.y - 6);
    drawBadge(ctx, annotation.x - 4, annotation.y - 20, number);
  }
  if (selected) {
    const bounds = annotationBounds(annotation);
    if (bounds) {
      ctx.setLineDash([6, 4]);
      ctx.lineWidth = 1.5;
      ctx.strokeStyle = "#2563eb";
      ctx.strokeRect(bounds.x - 6, bounds.y - 6, bounds.width + 12, bounds.height + 12);
    }
  }
  ctx.restore();
}

export function drawScene(canvas, bitmap, annotations, selectedId = null, preview = null) {
  if (!bitmap) return;
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  const ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(bitmap, 0, 0);
  annotations.forEach((annotation, index) => {
    drawAnnotation(ctx, annotation, index + 1, annotation.id === selectedId);
  });
  if (preview) drawAnnotation(ctx, preview, annotations.length + 1, false);
}

export function composePng(bitmap, annotations) {
  const canvas = document.createElement("canvas");
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  const ctx = canvas.getContext("2d");
  ctx.drawImage(bitmap, 0, 0);
  annotations.forEach((annotation, index) => drawAnnotation(ctx, annotation, index + 1, false));
  return canvas.toDataURL("image/png");
}
