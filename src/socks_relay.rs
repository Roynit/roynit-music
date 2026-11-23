//! Authenticated loopback HTTP CONNECT bridge so FFmpeg and yt-dlp use SOCKS too.
use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use std::{
    io::{Read, Write},
    net::{IpAddr, Shutdown, TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
pub struct Relay {
    pub proxy: String,
    stop: Arc<AtomicBool>,
    listener: Option<std::thread::JoinHandle<()>>,
}
impl Relay {
    pub fn start(upstream: &str) -> Result<Self> {
        let upstream = url::Url::parse(upstream)?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let secret = uuid::Uuid::new_v4().to_string();
        let authorization = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("riff:{secret}"))
        );
        let proxy = format!("http://riff:{secret}@{}", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        if active.load(Ordering::Relaxed) >= 32 {
                            let _ =
                                stream.write_all(b"HTTP/1.1 503 Busy\r\nConnection: close\r\n\r\n");
                            continue;
                        }
                        active.fetch_add(1, Ordering::Relaxed);
                        let counter = active.clone();
                        let stop = flag.clone();
                        let proxy = upstream.clone();
                        let auth = authorization.clone();
                        std::thread::spawn(move || {
                            if serve(&mut stream, &proxy, &auth, &stop).is_err() {
                                let _ = stream.shutdown(Shutdown::Both);
                            }
                            counter.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            proxy,
            stop,
            listener: Some(thread),
        })
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.listener.take() {
            let _ = thread.join();
        }
    }
}
fn decoded(text: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let raw = text.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            ensure!(i + 2 < raw.len(), "Invalid proxy credentials");
            bytes.push(u8::from_str_radix(
                std::str::from_utf8(&raw[i + 1..i + 3])?,
                16,
            )?);
            i += 3;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    Ok(bytes)
}
fn connect(proxy: &url::Url, host: &str, port: u16) -> Result<TcpStream> {
    let address = (
        proxy
            .host_str()
            .context("No SOCKS host")?
            .trim_matches(['[', ']']),
        proxy.port().unwrap_or(1080),
    );
    let mut stream = address
        .to_socket_addrs()?
        .take(3)
        .find_map(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(10)).ok())
        .context("Could not connect to SOCKS proxy")?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let authenticated = !proxy.username().is_empty();
    let method = if authenticated { 2 } else { 0 };
    stream.write_all(&[5, 1, method])?;
    let mut reply = [0; 2];
    stream.read_exact(&mut reply)?;
    ensure!(reply == [5, method], "SOCKS authentication method refused");
    if authenticated {
        let user = decoded(proxy.username())?;
        let password = decoded(proxy.password().unwrap_or(""))?;
        ensure!(
            (1..=255).contains(&user.len()) && (1..=255).contains(&password.len()),
            "SOCKS username/password must be 1..255 bytes"
        );
        let mut auth = vec![1, user.len() as u8];
        auth.extend(user);
        auth.push(password.len() as u8);
        auth.extend(password);
        stream.write_all(&auth)?;
        stream.read_exact(&mut reply)?;
        ensure!(reply == [1, 0], "SOCKS credentials were refused");
    }
    let host = host.trim_matches(['[', ']']);
    let mut request = vec![5, 1, 0];
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            request.push(1);
            request.extend(ip.octets());
        }
        Ok(IpAddr::V6(ip)) => {
            request.push(4);
            request.extend(ip.octets());
        }
        Err(_) => {
            ensure!(host.len() <= 255, "SOCKS target hostname too long");
            request.extend([3, host.len() as u8]);
            request.extend(host.as_bytes());
        }
    }
    request.extend(port.to_be_bytes());
    stream.write_all(&request)?;
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    ensure!(
        header[0] == 5 && header[1] == 0,
        "SOCKS connection was refused"
    );
    let count = match header[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut length = [0];
            stream.read_exact(&mut length)?;
            length[0] as usize
        }
        _ => anyhow::bail!("Invalid SOCKS address reply"),
    };
    let mut bound = vec![0; count + 2];
    stream.read_exact(&mut bound)?;
    Ok(stream)
}
fn serve(
    local: &mut TcpStream,
    proxy: &url::Url,
    authorization: &str,
    stop: &Arc<AtomicBool>,
) -> Result<()> {
    local.set_read_timeout(Some(Duration::from_secs(10)))?;
    local.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        local.read_exact(&mut byte)?;
        bytes.push(byte[0]);
        ensure!(bytes.len() < 16384, "Proxy headers too large");
    }
    let headers = std::str::from_utf8(&bytes)?;
    let mut lines = headers.split("\r\n");
    let request = lines.next().context("No proxy request")?;
    let fields: Vec<_> = request.split_whitespace().collect();
    ensure!(fields.len() == 3, "Malformed proxy request");
    let authorized = lines
        .clone()
        .filter_map(|line| line.split_once(':'))
        .any(|(name, value)| {
            name.eq_ignore_ascii_case("proxy-authorization") && value.trim() == authorization
        });
    if !authorized {
        local.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"Roy Music\"\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    }
    let tunnel = fields[0] == "CONNECT";
    let target = url::Url::parse(&if tunnel {
        format!("https://{}", fields[1])
    } else {
        fields[1].to_owned()
    })?;
    ensure!(
        ["http", "https"].contains(&target.scheme())
            && target.username().is_empty()
            && target.password().is_none(),
        "Invalid proxy target"
    );
    let mut remote = connect(
        proxy,
        target.host_str().context("No target host")?,
        target.port_or_known_default().context("No target port")?,
    )?;
    if tunnel {
        local.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
    } else {
        let path = format!(
            "{}{}",
            target.path(),
            target.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        let mut forwarded = format!("{} {} {}\r\n", fields[0], path, fields[2]);
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let name = line.split(':').next().unwrap_or("");
            if name.eq_ignore_ascii_case("proxy-authorization")
                || name.eq_ignore_ascii_case("proxy-connection")
                || name.eq_ignore_ascii_case("connection")
            {
                continue;
            }
            forwarded.push_str(line);
            forwarded.push_str("\r\n");
        }
        forwarded.push_str("Connection: close\r\n\r\n");
        remote.write_all(forwarded.as_bytes())?;
    }
    local.set_read_timeout(Some(Duration::from_millis(250)))?;
    remote.set_read_timeout(Some(Duration::from_millis(250)))?;
    let mut local_read = local.try_clone()?;
    let mut remote_write = remote.try_clone()?;
    let flag = stop.clone();
    let upload = std::thread::spawn(move || {
        let _ = forward(&mut local_read, &mut remote_write, &flag);
        let _ = remote_write.shutdown(Shutdown::Write);
    });
    let result = forward(&mut remote, local, stop);
    let _ = local.shutdown(Shutdown::Write);
    let _ = remote.shutdown(Shutdown::Both);
    let _ = upload.join();
    result
}
fn forward(input: &mut TcpStream, output: &mut TcpStream, stop: &AtomicBool) -> Result<()> {
    let mut bytes = [0; 16384];
    while !stop.load(Ordering::Relaxed) {
        match input.read(&mut bytes) {
            Ok(0) => break,
            Ok(count) => output.write_all(&bytes[..count])?,
            Err(e)
                if [std::io::ErrorKind::WouldBlock, std::io::ErrorKind::TimedOut]
                    .contains(&e.kind()) =>
            {
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
