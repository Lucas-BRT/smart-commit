use std::{
    env,
    io::{Read, Write},
    net::TcpStream,
    process::ExitCode,
    time::Duration,
};

/// `smart-commit-website --health`: GET / on the local port, for the container healthcheck.
/// The runtime image is `scratch`, so it has no curl.
fn healthy() -> bool {
    let port = env::var("PORT").unwrap_or_else(|_| "3000".into());
    let Ok(mut stream) = TcpStream::connect(format!("127.0.0.1:{port}")) else {
        return false;
    };
    let timeout = Some(Duration::from_secs(2));
    let _ = stream.set_read_timeout(timeout);
    let request = b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n";
    let mut head = [0; 12];
    stream.write_all(request).is_ok()
        && stream.read_exact(&mut head).is_ok()
        && head.ends_with(b" 200")
}

#[tokio::main]
async fn main() -> ExitCode {
    if env::args().nth(1).as_deref() == Some("--health") {
        return if healthy() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    topcoat::start(smart_commit_website::router())
        .await
        .unwrap();
    ExitCode::SUCCESS
}
