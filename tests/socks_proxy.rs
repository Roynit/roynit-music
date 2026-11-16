use base64::Engine as _;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::Duration,
};
fn headers(stream: &mut TcpStream) -> Vec<u8> {
    let mut data = Vec::new();
    let mut byte = [0];
    while !data.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        data.push(byte[0]);
        assert!(data.len() < 16384);
    }
    data
}
fn server(auth: bool, body: Option<Vec<u8>>) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut greeting = [0; 3];
        stream.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting, [5, 1, if auth { 2 } else { 0 }]);
        stream.write_all(&[5, if auth { 2 } else { 0 }]).unwrap();
        if auth {
            let mut head = [0; 2];
            stream.read_exact(&mut head).unwrap();
            let mut user = vec![0; head[1] as usize];
            stream.read_exact(&mut user).unwrap();
            let mut length = [0];
            stream.read_exact(&mut length).unwrap();
            let mut pass = vec![0; length[0] as usize];
            stream.read_exact(&mut pass).unwrap();
            assert_eq!(user, b"tester");
            assert_eq!(pass, b"p@ss");
            stream.write_all(&[1, 0]).unwrap();
        }
        let mut connect = [0; 5];
        stream.read_exact(&mut connect).unwrap();
        assert_eq!(&connect[..4], &[5, 1, 0, 3]);
        let mut host = vec![0; connect[4] as usize];
        stream.read_exact(&mut host).unwrap();
        let mut port = [0; 2];
        stream.read_exact(&mut port).unwrap();
        stream
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80])
            .unwrap();
        if let Some(body) = body {
            let request = String::from_utf8(headers(&mut stream)).unwrap();
            assert!(request.starts_with("GET /"));
            assert!(!request.to_lowercase().contains("proxy-authorization"));
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
            let _ = stream.write_all(&body);
        } else {
            let mut request = [0; 4];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"ping");
            stream.write_all(b"pong").unwrap();
        }
        String::from_utf8(host).unwrap()
    });
    (address, thread)
}
#[test]
fn authenticated_socks_routes_http_and_rejects_unauthorized_local_clients() {
    let (address, server) = server(true, Some(b"OK".to_vec()));
    let client = riff_core::network::Config {
        proxy: Some(format!("socks5h://tester:p%40ss@{address}")),
    }
    .client()
    .unwrap();
    let relay = url::Url::parse(client.proxy.as_ref().unwrap()).unwrap();
    let mut unauthorized =
        TcpStream::connect((relay.host_str().unwrap(), relay.port().unwrap())).unwrap();
    unauthorized
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    unauthorized
        .write_all(b"CONNECT example.invalid:443 HTTP/1.1\r\nHost: example.invalid\r\n\r\n")
        .unwrap();
    assert!(
        String::from_utf8(headers(&mut unauthorized))
            .unwrap()
            .contains("407")
    );
    assert_eq!(
        client
            .get("http://example.invalid/test")
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .unwrap(),
        "OK"
    );
    assert_eq!(server.join().unwrap(), "example.invalid");
    assert!(!client.config.status().to_string().contains("p%40ss"));
}
#[test]
fn socks_connect_tunnel_preserves_raw_tls_payload() {
    let (address, server) = server(false, None);
    let client = riff_core::network::Config {
        proxy: Some(format!("socks5://{address}")),
    }
    .client()
    .unwrap();
    let proxy = url::Url::parse(client.proxy.as_ref().unwrap()).unwrap();
    let auth = base64::engine::general_purpose::STANDARD.encode(format!(
        "{}:{}",
        proxy.username(),
        proxy.password().unwrap()
    ));
    let mut stream =
        TcpStream::connect((proxy.host_str().unwrap(), proxy.port().unwrap())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(format!("CONNECT music.example:443 HTTP/1.1\r\nHost: music.example\r\nProxy-Authorization: Basic {auth}\r\n\r\n").as_bytes()).unwrap();
    assert!(
        String::from_utf8(headers(&mut stream))
            .unwrap()
            .contains("200")
    );
    stream.write_all(b"ping").unwrap();
    let mut reply = [0; 4];
    stream.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"pong");
    drop(stream);
    assert_eq!(server.join().unwrap(), "music.example");
}
#[test]
#[ignore = "Checks actual FFmpeg HTTP audio decoding through the local SOCKS5 relay"]
fn ffmpeg_audio_uses_socks_via_authenticated_http_relay() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tone.wav");
    riff_core::tools::run(
        "ffmpeg",
        [
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            path.to_str().unwrap(),
        ],
        Duration::from_secs(10),
    )
    .unwrap();
    let (address, server) = server(false, Some(std::fs::read(path).unwrap()));
    let client = riff_core::network::Config {
        proxy: Some(format!("socks5h://{address}")),
    }
    .client()
    .unwrap();
    riff_core::tools::run(
        "ffmpeg",
        [
            "-v",
            "error",
            "-http_proxy",
            client.proxy.as_deref().unwrap(),
            "-f",
            "wav",
            "-i",
            "http://audio.invalid/tone.wav",
            "-t",
            "0.1",
            "-f",
            "null",
            "-",
        ],
        Duration::from_secs(15),
    )
    .unwrap();
    assert_eq!(server.join().unwrap(), "audio.invalid");
}

#[test]
fn delayed_local_client_does_not_inherit_listener_nonblocking_mode() {
    let (address, server) = server(false, Some(b"OK".to_vec()));
    let client = riff_core::network::Config {
        proxy: Some(format!("socks5h://{address}")),
    }
    .client()
    .unwrap();
    let proxy = url::Url::parse(client.proxy.as_ref().unwrap()).unwrap();
    let mut stream =
        TcpStream::connect((proxy.host_str().unwrap(), proxy.port().unwrap())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let auth = base64::engine::general_purpose::STANDARD.encode(format!(
        "{}:{}",
        proxy.username(),
        proxy.password().unwrap()
    ));
    write!(stream,"GET http://example.invalid/song HTTP/1.1\r\nHost: example.invalid\r\nProxy-Authorization: Basic {auth}\r\nConnection: close\r\n\r\n").unwrap();
    let response = String::from_utf8(headers(&mut stream)).unwrap();
    assert!(response.contains("200"));
    assert_eq!(server.join().unwrap(), "example.invalid");
}
