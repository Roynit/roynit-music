use riff_core::{model::Effects, player::Player};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};
fn wav() -> Vec<u8> {
    let len = 2400u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&len.to_le_bytes());
    out.resize(44 + len as usize, 0);
    out
}
#[test]
#[ignore = "Real FFmpeg/native muted audio against controlled local HTTP failure/EOF fixtures"]
fn decoder_terminal_state_precedes_eof_and_detects_truncated_streams() {
    for (status, duration, failure) in [(403, 20., true), (200, 0.15, false), (200, 20., true)] {
        println!("HTTP fixture status={status}, duration={duration}");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Ok((mut stream, _)) = listener.accept() {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut block = [0; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = stream.read(&mut block).unwrap();
                        assert!(n > 0, "Incomplete HTTP request");
                        request.extend_from_slice(&block[..n]);
                        assert!(request.len() < 16384);
                    }
                    let body = if status == 200 { wav() } else { Vec::new() };
                    let header = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        status,
                        if status == 200 { "OK" } else { "Forbidden" },
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(&body);
                    return;
                }
                if Instant::now() > deadline {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let mut player = Player::default();
        player
            .play(
                "fixture".into(),
                format!("http://{address}/audio?token=do-not-expose"),
                vec![],
                duration,
                Effects {
                    muted: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !player.finished() {
            assert!(
                Instant::now() < deadline,
                "decoder did not terminate: {}",
                player.snapshot()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let state = player.snapshot();
        assert_eq!(
            !state["decoder_error"].as_str().unwrap().is_empty(),
            failure,
            "{state}"
        );
        assert!(
            !state["decoder_error"]
                .as_str()
                .unwrap()
                .contains("do-not-expose")
        );
        assert_eq!(state["playing"], false);
        player.stop();
        assert_eq!(player.snapshot()["buffering"], false);
        assert_eq!(player.snapshot()["decoder_error"], "");
        server.join().unwrap();
    }
}
