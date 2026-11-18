//! Quick native transport controls using the same installed HTTP adapter.
use intersect_kpar_host::hosted::{FetchLimits, download};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
};

fn serve(reply: Vec<u8>) -> Result<(String, std::thread::JoinHandle<()>), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let url = format!(
        "http://{}/library.kpar",
        listener.local_addr().map_err(|e| e.to_string())?
    );
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let worker = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(3) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
                    let _ = stream.read(&mut [0; 4096]);
                    let _ = stream.write_all(&reply);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(5))
                }
                Err(_) => break,
            }
        }
    });
    Ok((url, worker))
}
fn staging(name: &str) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"))
        .join(format!("hosted-test-{}-{name}", std::process::id()))
}

#[test]
fn hosted_download_retains_exact_bytes_digest_and_removes_staging_file() -> Result<(), String> {
    let bytes = include_bytes!("../fixtures/motors.kpar");
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    )
    .into_bytes();
    response.extend(bytes);
    let (url, server) = serve(response)?;
    let directory = staging("exact");
    let result = download(&url, &directory, FetchLimits::default());
    server.join().map_err(|_| "Server panicked")?;
    let result = result?;
    assert_eq!(result.bytes, bytes);
    assert_eq!(
        result.sha256,
        intersect_model::definitions::pack::FileDigest::of(bytes).sha256
    );
    assert_eq!(result.requested_url, url);
    assert_eq!(result.effective_url, url);
    assert_eq!(
        std::fs::read_dir(&directory)
            .map_err(|e| e.to_string())?
            .count(),
        0
    );
    std::fs::remove_dir(directory).map_err(|e| e.to_string())?;
    Ok(())
}

#[test]
fn hosted_download_refuses_oversize_http_errors_and_non_http_redirects() -> Result<(), String> {
    for (name, reply) in [
        (
            "size",
            "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nlarge",
        ),
        (
            "status",
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ),
        (
            "redirect",
            "HTTP/1.1 302 Found\r\nLocation: file:///forbidden.kpar\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ),
    ] {
        let (url, server) = serve(reply.as_bytes().to_vec())?;
        let directory = staging(name);
        let result = download(
            &url,
            &directory,
            FetchLimits {
                max_bytes: 10,
                timeout_seconds: 3,
            },
        );
        server.join().map_err(|_| "Server panicked")?;
        assert!(result.is_err(), "{name} unexpectedly succeeded");
        assert_eq!(
            std::fs::read_dir(&directory)
                .map_err(|e| e.to_string())?
                .count(),
            0
        );
        std::fs::remove_dir(directory).map_err(|e| e.to_string())?;
    }
    assert!(
        download(
            "file:///forbidden.kpar",
            &staging("unused"),
            FetchLimits::default()
        )
        .is_err()
    );
    Ok(())
}
