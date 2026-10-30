use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// How long a reply may take: a generation on a slow CPU is the longest.
const REPLY_TIMEOUT: Duration = Duration::from_secs(300);

/// How long `/health` may take. A loaded server answers at once, and one
/// that is frozen (a hung GPU driver) accepts the connection and never
/// replies: waiting as long as for a reply would hang every commit.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

/// Whether the server at `socket` answers `/health` with 200 within `timeout`.
pub fn healthy_within(socket: &Path, timeout: Duration) -> bool {
    matches!(
        request(socket, "GET", "/health", None, timeout),
        Ok((200, _))
    )
}

/// One HTTP/1.1 request over the UNIX socket; returns status and body.
pub fn http(
    socket: &Path,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> std::io::Result<(u16, String)> {
    request(socket, method, path, body, REPLY_TIMEOUT)
}

fn request(
    socket: &Path,
    method: &str,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> std::io::Result<(u16, String)> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(timeout))?;
    let body = body.unwrap_or("");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_http_response(&raw).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Parses a complete HTTP/1.1 response, with a Content-Length or chunked
/// body (or one that ends with the connection).
pub fn parse_http_response(raw: &[u8]) -> Result<(u16, String), String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no end of headers")?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| "headers are not UTF-8")?;
    let mut body = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("no status code")?;
    let header = |name: &str| {
        head.split("\r\n").skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_owned())
        })
    };
    let decoded;
    if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        decoded = dechunk(body)?;
        body = &decoded;
    } else if let Some(length) = header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        body = body
            .get(..length)
            .ok_or("body shorter than Content-Length")?;
    }
    let text = String::from_utf8(body.to_vec()).map_err(|_| "body is not UTF-8")?;
    Ok((status, text))
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("truncated chunk size")?;
        let size_text = std::str::from_utf8(&data[..line_end]).map_err(|_| "bad chunk size")?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| format!("bad chunk size {size_text:?}"))?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(data.get(..size).ok_or("truncated chunk")?);
        data = data.get(size + 2..).ok_or("truncated chunk")?;
    }
}
