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
