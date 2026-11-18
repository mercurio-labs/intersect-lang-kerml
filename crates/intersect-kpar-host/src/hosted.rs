//! Explicit host acquisition. Only source KPAR bytes are downloaded; no remote
//! executable or prepared image is admitted through the trusted cache path.
use intersect_model::definitions::pack::FileDigest;
use std::{
    path::Path,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_DOWNLOAD: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub struct FetchLimits {
    pub max_bytes: usize,
    pub timeout_seconds: u32,
}
impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            max_bytes: 16 * 1024 * 1024,
            timeout_seconds: 30,
        }
    }
}

pub struct DownloadedKpar {
    pub requested_url: String,
    pub effective_url: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

/// Use the installed curl client for HTTP and HTTPS with bounded body size,
/// timeout, and redirects restricted to those protocols. Arguments never enter a
/// shell. The staging file is private to this call and removed on every return.
pub fn download(
    url: &str,
    staging_directory: &Path,
    limits: FetchLimits,
) -> Result<DownloadedKpar, String> {
    if limits.max_bytes == 0 || limits.timeout_seconds == 0 {
        return Err("Download limits must be positive".into());
    }
    if url.len() > 8192 {
        return Err("Archive URL is too long".into());
    }
    let iri = iri_string::types::IriAbsoluteStr::new(url)
        .map_err(|e| format!("Invalid archive URL: {e}"))?;
    if !matches!(iri.scheme_str(), "http" | "https") {
        return Err("Hosted KPAR URLs must use HTTP or HTTPS".into());
    }
    std::fs::create_dir_all(staging_directory).map_err(|e| e.to_string())?;
    let sequence = NEXT_DOWNLOAD.fetch_add(1, Ordering::Relaxed);
    let path = staging_directory.join(format!("download-{}-{sequence}.part", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    drop(file);
    struct Staging(std::path::PathBuf);
    impl Drop for Staging {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let staging = Staging(path);
    let client = if cfg!(windows) { "curl.exe" } else { "curl" };
    let output = Command::new(client)
        .args([
            "--disable",
            "--noproxy",
            "localhost,127.0.0.1,::1",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--max-redirs",
            "5",
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--max-filesize",
        ])
        .arg(limits.max_bytes.to_string())
        .arg("--max-time")
        .arg(limits.timeout_seconds.to_string())
        .arg("--output")
        .arg(&staging.0)
        .args(["--write-out", "%{url_effective}", "--url"])
        .arg(url)
        .output()
        .map_err(|e| format!("Cannot start {client}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "KPAR download failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let length = std::fs::metadata(&staging.0)
        .map_err(|e| e.to_string())?
        .len();
    if length > limits.max_bytes as u64 {
        return Err("KPAR download exceeded its byte limit".into());
    }
    let bytes = std::fs::read(&staging.0).map_err(|e| e.to_string())?;
    let effective_url = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    if effective_url.len() > 8192 {
        return Err("Redirected archive URL is too long".into());
    }
    let sha256 = FileDigest::of(&bytes).sha256;
    Ok(DownloadedKpar {
        requested_url: url.into(),
        effective_url,
        bytes,
        sha256,
    })
}
