use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// A stand-in for googlevideo: bounded ranges are served, a whole-file request is refused with 403.
fn fake_upstream(body: Vec<u8>) -> u16 {
    fake_upstream_with(body, 0)
}

/// `failures` is how many leading requests are answered with a 503 before the server behaves.
fn fake_upstream_with(body: Vec<u8>, failures: usize) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let body = body.clone();
            let seen = seen.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                if seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < failures {
                    let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    return;
                }
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut range = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = value.trim().to_owned();
                    }
                }
                let reply = match range.split_once('-') {
                    Some((a, b)) if !b.is_empty() => {
                        let (a, b): (usize, usize) = (a.parse().unwrap(), b.parse().unwrap());
                        let part = &body[a..=b.min(body.len() - 1)];
                        let head = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            part.len()
                        );
                        [head.as_bytes(), part].concat()
                    }
                    _ => b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                };
                let _ = stream.write_all(&reply);
            });
        }
    });
    port
}

fn get(port: u16, path: &str, range: &str) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: x\r\nRange: bytes={range}\r\n\r\n").unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    (String::from_utf8_lossy(&raw[..split]).into_owned(), raw[split..].to_vec())
}

#[test]
fn relay_serves_whole_file_requests_as_bounded_ranges() {
    let body: Vec<u8> = (0..2_600_000u32).map(|i| (i % 251) as u8).collect();
    let upstream = fake_upstream(body.clone());
    let url = format!("http://127.0.0.1:{upstream}/v?clen={}", body.len());
    let local = relay(&url, "test-agent").expect("relay address");
    let parsed = url::Url::parse(&local).unwrap();
    let (port, path) = (parsed.port().unwrap(), parsed.path().to_owned());

    // What FFmpeg sends first: the open-ended range the upstream would refuse.
    let (head, data) = get(port, &path, "0-");
    assert!(head.starts_with("HTTP/1.1 206"), "{head}");
    assert_eq!(data.len(), body.len());
    assert!(data == body, "relayed bytes differ from the source");

    // A seek lands in the middle of the file.
    let (_, tail) = get(port, &path, "2000000-");
    assert_eq!(tail, body[2_000_000..]);

    // Unknown paths and out-of-range requests are refused rather than forwarded.
    assert!(get(port, "/nope", "0-").0.starts_with("HTTP/1.1 404"));
    assert!(get(port, &path, "9999999-").0.starts_with("HTTP/1.1 416"));
}

#[test]
fn relay_retries_a_failed_chunk_so_ffmpeg_never_sees_a_gap() {
    let body: Vec<u8> = (0..700_000u32).map(|i| (i % 241) as u8).collect();
    // The first two requests to the "server" fail; the relay must retry, not hand FFmpeg a short file.
    let upstream = fake_upstream_with(body.clone(), 2);
    let url = format!("http://127.0.0.1:{upstream}/v?clen={}", body.len());
    let local = relay(&url, "test-agent").expect("relay address");
    let parsed = url::Url::parse(&local).unwrap();
    let (head, data) = get(parsed.port().unwrap(), parsed.path(), "0-");
    assert!(head.starts_with("HTTP/1.1 206"), "{head}");
    assert!(data == body, "got {} of {} bytes", data.len(), body.len());
}
