export class ApiError extends Error {
  constructor(status, message, data) {
    super(message || `RedLine returned ${status}`);
    this.name = "ApiError";
    this.status = status;
    this.data = data;
  }
}

export class RedLineApi {
  constructor({ serverUrl, token }) {
    this.serverUrl = String(serverUrl || "").replace(/\/+$/, "");
    this.token = String(token || "");
  }

  headers(extra = {}) {
    return {
      Authorization: `Bearer ${this.token}`,
      ...extra,
    };
  }

  async request(path, { method = "GET", body, headers = {} } = {}) {
    const init = {
      method,
      headers: this.headers({
        ...(body === undefined ? {} : { "Content-Type": "application/json" }),
        ...headers,
      }),
    };
    if (body !== undefined) init.body = JSON.stringify(body);
    const response = await fetch(`${this.serverUrl}${path}`, init);
    const text = await response.text();
    let data = text;
    try {
      data = text ? JSON.parse(text) : null;
    } catch {
      // keep the raw text for a useful error
    }
    if (!response.ok) {
      const message =
        (data && data.error) || (typeof data === "string" ? data : "") || response.statusText;
      throw new ApiError(response.status, message, data);
    }
    return data;
  }

  health() {
    return this.request("/health");
  }

  async projects() {
    return this.request("/api/projects");
  }

  async session(id) {
    return this.request(`/api/sessions/${encodeURIComponent(id)}`);
  }

  async createImageSession({ imageDataUrl, title, url, note = "", project, branch }) {
    return this.request("/api/sessions", {
      method: "POST",
      body: {
        title,
        url,
        note,
        image_base64: imageDataUrl,
        agent: "extension",
        project: project || undefined,
        branch: branch || undefined,
      },
    });
  }

  async submit(id, { annotations, width, height, png, note = "" }) {
    return this.request(`/api/sessions/${encodeURIComponent(id)}/annotations`, {
      method: "POST",
      body: { annotations, width, height, png, note },
    });
  }

  async uploadVideo({
    video,
    poster,
    trace,
    title,
    url,
    note = "",
    project,
    branch,
    durationMs,
    width,
    height,
    fps,
  }) {
    const form = new FormData();
    form.append("video", video, "flow.webm");
    if (poster) form.append("poster", poster, "poster.png");
    if (trace) {
      form.append("trace", new Blob([JSON.stringify(trace)], { type: "application/json" }), "trace.json");
    }
    form.append("title", title || "Browser flow");
    form.append("url", url || "");
    form.append("note", note || "");
    if (project) form.append("project", project);
    if (branch) form.append("branch", branch);
    if (durationMs) form.append("duration_ms", String(Math.round(durationMs)));
    if (width) form.append("width", String(width));
    if (height) form.append("height", String(height));
    if (fps) form.append("fps", String(fps));
    const response = await fetch(`${this.serverUrl}/api/sessions/video`, {
      method: "POST",
      headers: this.headers(),
      body: form,
    });
    const text = await response.text();
    let data = text;
    try {
      data = text ? JSON.parse(text) : null;
    } catch {
      // keep raw text
    }
    if (!response.ok) {
      const message =
        (data && data.error) || (typeof data === "string" ? data : "") || response.statusText;
      throw new ApiError(response.status, message, data);
    }
    return data;
  }

  async deleteSession(id) {
    return this.request(`/api/sessions/${encodeURIComponent(id)}`, { method: "DELETE" });
  }

  async imageBlob(id) {
    const response = await fetch(
      `${this.serverUrl}/sessions/${encodeURIComponent(id)}/image`,
      { headers: this.headers() },
    );
    if (!response.ok) {
      throw new ApiError(response.status, `cannot load screenshot (${response.status})`);
    }
    return response.blob();
  }
}
