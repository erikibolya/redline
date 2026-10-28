//! End-to-end test of the CLI verbs against a real server process.
//!
//! This is the test that guards the migration promise: the v0.1 recipe
//! (`open` → annotate → `wait` → `get` → `remove`) must keep working, with the same exit codes.

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_redline")
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .unwrap()
        .port()
}

struct Server {
    child: std::process::Child,
    dir: tempfile::TempDir,
    port: u16,
}

impl Server {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let port = free_port();
        let child = Command::new(binary())
            .args([
                "serve",
                "--port",
                &port.to_string(),
                "--data-dir",
                dir.path().to_str().unwrap(),
                "--token",
                "test-token",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn server");

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return Self { child, dir, port };
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("server did not start on port {port}");
    }

    fn data_dir(&self) -> &Path {
        self.dir.path()
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let output = Command::new(binary())
            .args(args)
            .args(["--data-dir", self.dir.path().to_str().unwrap()])
            .args(["--port", &self.port.to_string()])
            .output()
            .expect("run cli");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )
    }

    fn submit(&self, id: &str) {
        let body = serde_json::json!({
            "annotations": [
                {"type": "text", "x": 5, "y": 6, "text": "Move this up", "color": "#e11d48"}
            ],
            "width": 640,
            "height": 360,
            "png": format!("data:image/png;base64,{PNG_B64}")
        });
        let client = redline::http::Client::new(&format!("http://127.0.0.1:{}", self.port), "test-token").unwrap();
        client
            .post(&format!("/api/sessions/{id}/annotations"), &body)
            .expect("submit");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}


fn write_png(dir: &Path) -> PathBuf {
    use base64::Engine as _;
    let path = dir.join("shot.png");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(&base64::engine::general_purpose::STANDARD.decode(PNG_B64).unwrap())
        .unwrap();
    path
}

#[test]
fn the_v01_recipe_still_works() {
    let server = Server::start();
    let image = write_png(server.data_dir());

    // 1. open — creates a session and prints the review URL.
    let (code, stdout, stderr) = server.run(&["open", image.to_str().unwrap(), "--title", "Button / default", "--note", "Check hover"]);
    assert_eq!(code, 0, "open failed: {stderr}");
    assert!(stdout.contains("session: "), "{stdout}");
    let id = stdout
        .lines()
        .find_map(|line| line.strip_prefix("session: "))
        .expect("session id")
        .trim()
        .to_string();
    assert!(stdout.contains(&format!("/s/{id}?t=test-token")), "{stdout}");

    // 2. list — shows it as open.
    let (code, stdout, _) = server.run(&["list"]);
    assert_eq!(code, 0);
    assert!(stdout.contains(&id), "{stdout}");
    assert!(stdout.contains("open"), "{stdout}");

    // 3. wait — exits 2 on timeout, which the agent workflow depends on.
    let (code, _, stderr) = server.run(&["wait", &id, "--timeout", "1", "--interval", "1"]);
    assert_eq!(code, 2, "expected timeout exit code, stderr: {stderr}");
    assert!(stderr.contains("timeout"), "{stderr}");

    // 4. the human submits; wait then returns the annotations.
    server.submit(&id);
    let (code, stdout, stderr) = server.run(&["wait", &id, "--timeout", "5", "--interval", "1"]);
    assert_eq!(code, 0, "wait failed: {stderr}");
    assert!(stdout.contains("Move this up"), "{stdout}");

    // 5. get — same JSON as v0.1.
    let (code, stdout, _) = server.run(&["get", &id]);
    assert_eq!(code, 0);
    assert!(stdout.contains("\"status\": \"submitted\""));
    assert!(stdout.contains("Move this up"));

    // 6. unknown id is exit 1, not a crash.
    let (code, _, stderr) = server.run(&["get", "does-not-exist"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("session not found"));

    // 7. remove — deletes the folder, and deletes it once.
    let folder = server.data_dir().join("sessions").join(&id);
    assert!(folder.is_dir());
    let (code, stdout, _) = server.run(&["remove", &id, "--yes"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("removed"), "{stdout}");
    assert!(!folder.exists());
    let (code, _, _) = server.run(&["remove", &id, "--yes"]);
    assert_eq!(code, 1);
}

#[test]
fn json_output_is_machine_readable() {
    let server = Server::start();
    let image = write_png(server.data_dir());

    let (code, stdout, _) = server.run(&[
        "open",
        image.to_str().unwrap(),
        "--title",
        "JSON",
        "--json",
    ]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(parsed["meta"]["title"], "JSON");
    assert!(parsed["review_url"].as_str().unwrap().contains("/s/"));
    assert_eq!(parsed["meta"]["agent"], "cli");

    let (code, stdout, _) = server.run(&["list", "--json"]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed.as_array().unwrap().len(), 1);
}

#[test]
fn client_fails_cleanly_when_nothing_is_listening() {
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let output = Command::new(binary())
        .args([
            "list",
            "--data-dir",
            dir.path().to_str().unwrap(),
            "--port",
            &port.to_string(),
            "--token",
            "x",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot reach redline"), "{stderr}");
    assert!(stderr.contains("redline serve"), "the error should say what to do: {stderr}");
}

#[test]
fn wrong_token_is_reported_not_ignored() {
    let server = Server::start();
    let image = write_png(server.data_dir());
    let output = Command::new(binary())
        .args(["open", image.to_str().unwrap(), "--title", "x"])
        .args(["--data-dir", server.data_dir().to_str().unwrap()])
        .args(["--port", &server.port.to_string()])
        .args(["--token", "wrong-token"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("401"), "{stderr}");
}
