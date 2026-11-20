# RedLine Agent (tray app)

A small Tauri v2 app around the headless `redline-agent` bridge. It is separate from the review
client: it installs the bridge, starts it at login, supervises it and shows the same live activity
the server exposes at `/agent`.

- Tray: Open, Open RedLine, Start/Stop agent, Start at login, Check for updates, Quit.
- Window: connection state, current review and step, elapsed time, the bridge's local output and
  the run's server-side output, and a settings form (server URL, agent token, agent id, repo root,
  default repo, OpenCode server, model).
- The UI is a loopback page (`http://127.0.0.1:8791/`) served by the app; the webview uses no
  Tauri APIs. The agent token is stored in `%APPDATA%\dev.erikibolya.redline.agent\agent-app.json`
  and is never returned by the local API.
- Updates use their own manifest asset, `agent-latest.json`, so the bridge can move independently
  of the review client's `latest.json`.

## Build

The Tauri bundle needs the bridge as a resource. From the repository root:

```bash
cargo build --release -p redline-agent --manifest-path desktop/Cargo.toml
cp desktop/target/release/redline-agent.exe desktop/agent-app/bin/redline-agent.exe
cd desktop/agent-app
npx --yes @tauri-apps/cli@^2 build --bundles nsis
```

The release job does this automatically: it downloads the released `redline-agent.exe`, builds
the installer with `TAURI_SIGNING_PRIVATE_KEY`, and uploads
`RedLineAgent_<version>_x64-setup.exe` plus `agent-latest.json`.

The tray app and the desktop app's **Start local agent** are mutually exclusive in practice: the
embedded agent is always `local`, the tray app runs the device id configured in its settings.
