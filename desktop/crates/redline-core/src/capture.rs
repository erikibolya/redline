//! Headless screenshot capture for the `capture_review` tool.
//!
//! Deliberately shells out to a Chromium binary instead of embedding a browser engine: the
//! default server image stays ~4 MB and works everywhere, and only hosts that want capture need
//! the `:browser` image (Debian + Chromium) or a Chrome on PATH.

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CaptureRequest {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

impl CaptureRequest {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            width: 1440,
            height: 900,
        }
    }
}

/// `REDLINE_CHROME` wins, then the usual binary names on PATH.
pub fn find_browser() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("REDLINE_CHROME") {
        if !explicit.trim().is_empty() {
            return Some(PathBuf::from(explicit));
        }
    }
    let names = [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "chrome",
    ];
    for name in names {
        if let Some(found) = which(name) {
            return Some(found);
        }
    }
    None
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for suffix in ["", ".exe"] {
            let candidate = dir.join(format!("{name}{suffix}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Capture with an explicit browser path (`None` → no browser available).
pub fn capture_from(browser: Option<&Path>, request: &CaptureRequest) -> Result<Vec<u8>, String> {
    let browser = browser.ok_or_else(|| {
        "no Chromium found on this host. Use the :browser image, or set REDLINE_CHROME to a \
         chrome/chromium binary."
            .to_string()
    })?;

    wait_for_host(&request.url);

    let work =
        std::env::temp_dir().join(format!("redline-capture-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&work)
        .map_err(|err| format!("cannot create {}: {err}", work.display()))?;
    let shot = work.join("shot.png");

    let output = Command::new(browser)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--no-sandbox",
            "--hide-scrollbars",
            "--disable-dev-shm-usage",
            &format!("--user-data-dir={}", work.join("profile").display()),
            &format!("--window-size={},{}", request.width, request.height),
            &format!("--screenshot={}", shot.display()),
            "--virtual-time-budget=5000",
            &request.url,
        ])
        .output()
        .map_err(|err| format!("cannot run {}: {err}", browser.display()));

    let result = match output {
        Ok(output) if !output.status.success() => Err(format!(
            "browser exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .last()
                .unwrap_or_default()
        )),
        Ok(_) => {
            std::fs::read(&shot).map_err(|err| format!("browser produced no screenshot: {err}"))
        }
        Err(err) => Err(err),
    };
    let _ = std::fs::remove_dir_all(&work);

    let bytes = result?;
    if bytes.is_empty() {
        return Err("browser produced an empty screenshot".to_string());
    }
    Ok(bytes)
}

pub fn capture(request: &CaptureRequest) -> Result<Vec<u8>, String> {
    let browser = find_browser();
    capture_from(browser.as_deref(), request)
}

/// Give a local dev server a moment to answer, so a capture right after `npm start` works.
/// Best effort: never fails the capture.
fn wait_for_host(url: &str) {
    let Some(rest) = url.split("://").nth(1) else {
        return;
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), port.parse().unwrap_or(80)),
        None => (authority.to_string(), 80),
    };
    if host != "127.0.0.1" && host != "localhost" {
        return;
    }
    for _ in 0..20 {
        if TcpStream::connect_timeout(
            &match format!("{host}:{port}").parse() {
                Ok(addr) => addr,
                Err(_) => return,
            },
            Duration::from_millis(250),
        )
        .is_ok()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_browser_is_an_actionable_error() {
        let error = capture_from(None, &CaptureRequest::new("http://example.com")).unwrap_err();
        assert!(error.contains(":browser"));
        assert!(error.contains("REDLINE_CHROME"));
    }

    #[test]
    fn viewport_parsing_is_forgiving() {
        assert_eq!(parse_viewport("1440x900").unwrap(), (1440, 900));
        assert_eq!(parse_viewport(" 800 x 600 ").unwrap(), (800, 600));
        assert!(parse_viewport("big").is_err());
    }
}

/// Shared by the MCP tool: `"1440x900"` → dimensions.
pub fn parse_viewport(value: &str) -> Result<(u32, u32), String> {
    let cleaned = value.trim().to_lowercase().replace(' ', "");
    let (width, height) = cleaned
        .split_once('x')
        .ok_or_else(|| "viewport must look like 1440x900".to_string())?;
    let width = width
        .parse()
        .map_err(|_| "viewport width must be a number".to_string())?;
    let height = height
        .parse()
        .map_err(|_| "viewport height must be a number".to_string())?;
    if width < 100 || height < 100 || width > 8000 || height > 8000 {
        return Err("viewport must be between 100 and 8000 in each dimension".to_string());
    }
    Ok((width, height))
}
