//! Minimal HTTP/1.1 server for tests (one request per connection, closes after responding).

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone, Debug)]
pub(crate) struct Route {
    /// Request path without the query string, e.g. `/file.zip`.
    pub path: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Parse `Range: bytes=N-` and answer 206 (or 416 when N is past the end).
    pub honor_range: bool,
}

impl Route {
    pub fn ok(path: &str, body: Vec<u8>) -> Self {
        Route { path: path.into(), status: 200, headers: vec![], body, honor_range: false }
    }
}

/// Starts the server on an ephemeral port and returns its base url (`http://127.0.0.1:PORT`).
/// Unknown paths get 404. A route with a `transfer-encoding: chunked` header is sent chunked,
/// i.e. without a Content-Length.
pub(crate) async fn serve(routes: Vec<Route>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { break };
            let routes = routes.clone();
            tokio::spawn(async move {
                let _ = handle(stream, &routes).await;
            });
        }
    });
    format!("http://{addr}")
}

async fn handle(mut stream: TcpStream, routes: &[Route]) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    let mut lines = text.split("\r\n");
    let target = lines.next().unwrap_or("").split(' ').nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/");
    let range_offset = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("range"))
        .and_then(|(_, v)| v.trim().strip_prefix("bytes=").map(str::to_owned))
        .and_then(|v| v.split('-').next().and_then(|n| n.parse::<usize>().ok()));

    let Some(route) = routes.iter().find(|r| r.path == path) else {
        return write_response(&mut stream, 404, "Not Found", &[], b"not found", true).await;
    };

    let chunked = route
        .headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.eq_ignore_ascii_case("chunked"));
    let len = route.body.len();
    match range_offset {
        Some(offset) if route.honor_range => {
            if offset >= len {
                let h = vec![("Content-Range".to_string(), format!("bytes */{len}"))];
                write_response(&mut stream, 416, "Range Not Satisfiable", &h, b"", true).await
            } else {
                let mut h = route.headers.clone();
                h.push(("Content-Range".into(), format!("bytes {offset}-{}/{len}", len - 1)));
                write_response(&mut stream, 206, "Partial Content", &h, &route.body[offset..], !chunked).await
            }
        }
        _ => write_response(&mut stream, route.status, "Status", &route.headers, &route.body, !chunked).await,
    }
}

async fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    headers: &[(String, String)],
    body: &[u8],
    with_length: bool,
) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if with_length {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes()).await?;
    if with_length {
        stream.write_all(body).await?;
    } else {
        // chunked: a single chunk then the terminator
        if !body.is_empty() {
            stream.write_all(format!("{:x}\r\n", body.len()).as_bytes()).await?;
            stream.write_all(body).await?;
            stream.write_all(b"\r\n").await?;
        }
        stream.write_all(b"0\r\n\r\n").await?;
    }
    stream.flush().await?;
    stream.shutdown().await
}
