//! HTTP helpers shared by all sites: file names, streaming download with resume and checksum.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use reqwest::header::{HeaderMap, CONTENT_DISPOSITION, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::StatusCode;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::DownloadError;

/// Shared by reqwest and the WebView2 windows so that sites see the same browser.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

/// No total timeout: large files may legitimately take hours.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .build()
        .expect("failed to build reqwest client")
}

/// Replaces characters Windows does not allow in file names.
pub fn safe_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c <= '\u{1f}' { '_' } else { c })
        .collect();
    let trimmed = replaced.trim_matches(|c| c == ' ' || c == '.');
    if trimmed.is_empty() { "unnamed".to_string() } else { trimmed.to_string() }
}

/// `host/path` of `url`, for logs and error messages: the query and fragment may hold tokens.
pub(crate) fn redact_url(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(u) => format!("{}{}", u.host_str().unwrap_or(""), u.path()),
        Err(_) => url.split(['?', '#']).next().unwrap_or_default().to_string(),
    }
}

pub(crate) fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        (b as char).to_digit(16).map(|d| d as u8)
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

static RE_EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)filename\*\s*=\s*utf-8''([^;]+)"#).unwrap());
static RE_QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"filename\s*=\s*"([^"]+)""#).unwrap());
static RE_PLAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"filename\s*=\s*([^;]+)"#).unwrap());

/// Extracts the file name from `Content-Disposition`.
/// Order: `filename*=UTF-8''...`, quoted `filename="..."`, unquoted `filename=...`.
/// Raw header bytes are decoded as UTF-8 first (many servers put UTF-8 straight in the header),
/// falling back to latin-1.
pub fn filename_from_headers(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(CONTENT_DISPOSITION)?.as_bytes();
    let cd = match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => raw.iter().map(|&b| b as char).collect(),
    };
    let name = if let Some(m) = RE_EXT.captures(&cd) {
        percent_decode(m[1].trim().trim_matches('"'))
    } else if let Some(m) = RE_QUOTED.captures(&cd).or_else(|| RE_PLAIN.captures(&cd)) {
        m[1].trim().to_string()
    } else {
        return None;
    };
    if name.is_empty() { None } else { Some(name) }
}

/// `(downloaded, total)`
pub type ProgressFn<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync);

pub struct SaveOptions<'a> {
    pub size: Option<u64>,
    pub sha256: Option<&'a str>,
    pub headers: HeaderMap,
    /// When true, a 403 or `text/html` response is rejected with `NotAFile("rejected")` before
    /// anything is written (sites answer with a login/captcha page instead of the file).
    pub reject_html: bool,
}

fn part_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

async fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Streams `url` into `<target>.part`, resuming an existing `.part` with a `Range` request, then
/// verifies the size / SHA-256 and renames to `target`. Returns the file name the response's
/// `Content-Disposition` gives, if any.
pub async fn save_stream(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
    opts: SaveOptions<'_>,
    progress: ProgressFn<'_>,
) -> Result<Option<String>, DownloadError> {
    let part = part_path(target);
    let mut offset = match tokio::fs::metadata(&part).await {
        Ok(m) => m.len(),
        Err(_) => 0,
    };

    let mut request = client.get(url).headers(opts.headers.clone());
    if offset > 0 {
        request = request.header(RANGE, format!("bytes={offset}-"));
    }
    let mut response = request.send().await?;
    let status = response.status();
    let header_name = filename_from_headers(response.headers());

    if opts.reject_html {
        let is_html = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.to_ascii_lowercase().contains("text/html"));
        if status == StatusCode::FORBIDDEN || is_html {
            return Err(DownloadError::NotAFile("rejected".into()));
        }
    }

    let mut size = opts.size;
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        // The .part should already be the complete file; `Content-Range: bytes */<len>` tells
        // the real length so an oversized stale .part is detected below.
        if size.is_none() {
            size = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|n| n.trim().parse::<u64>().ok());
        }
    } else {
        if status == StatusCode::NOT_FOUND {
            return Err(DownloadError::NotFound(redact_url(url)));
        }
        if !status.is_success() {
            return Err(DownloadError::Retryable(format!("HTTP {status} for {}", redact_url(url))));
        }
        if offset > 0 && status != StatusCode::PARTIAL_CONTENT {
            offset = 0; // the server ignored Range: start over
        }
        if size.is_none() {
            size = response.content_length().map(|len| offset + len);
        }
        if let Some(parent) = part.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = if offset > 0 {
            tokio::fs::OpenOptions::new().append(true).open(&part).await?
        } else {
            tokio::fs::File::create(&part).await?
        };
        let mut got = offset;
        progress(got, size);
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
            got += chunk.len() as u64;
            progress(got, size);
        }
        file.flush().await?;
    }

    let actual = tokio::fs::metadata(&part).await?.len();
    if let Some(expected) = size {
        if actual > expected {
            // A stale/oversized .part can never become correct by resuming: start clean next time.
            let _ = tokio::fs::remove_file(&part).await;
        }
        if actual != expected {
            return Err(DownloadError::Retryable(format!("size mismatch: got {actual} bytes, expected {expected}")));
        }
    }
    if let Some(expected) = opts.sha256 {
        if !sha256_of(&part).await?.eq_ignore_ascii_case(expected.trim()) {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(DownloadError::Retryable("checksum mismatch (SHA-256)".into()));
        }
    }
    tokio::fs::rename(&part, target).await?;
    Ok(header_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_server::{serve, serve_logged, Route};
    use reqwest::header::{HeaderMap, HeaderValue, CONTENT_DISPOSITION};
    use std::sync::Mutex;

    #[test]
    fn safe_name_replaces_windows_invalid_chars() {
        assert_eq!(safe_name("a:b?.zip"), "a_b_.zip");
        assert_eq!(safe_name(" .hidden. "), "hidden");
        assert_eq!(safe_name("【11 Avatars】 Sweet Devil.zip"), "【11 Avatars】 Sweet Devil.zip");
        assert_eq!(safe_name("..."), "unnamed");
    }

    #[test]
    fn redact_url_keeps_host_and_path_only() {
        assert_eq!(redact_url("https://h.com/a/b.zip?sig=secret#frag"), "h.com/a/b.zip");
        assert_eq!(redact_url("http://127.0.0.1:8080/x?token=1"), "127.0.0.1/x");
        assert_eq!(redact_url("not a url?token=1"), "not a url");
    }

    #[tokio::test]
    async fn errors_do_not_contain_the_query() {
        let base = serve(vec![]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        let url = format!("{base}/nope?sig=secret");
        let err = save_stream(&client(), &url, &target, opts(), &noop).await.unwrap_err();
        assert!(!err.to_string().contains("secret"), "{err}");
        // A connection error from reqwest names the url too.
        let err = save_stream(&client(), "http://127.0.0.1:9/x?sig=secret", &target, opts(), &noop).await.unwrap_err();
        assert!(!err.to_string().contains("secret"), "{err}");
    }

    #[test]
    fn filename_from_headers_variants() {
        let h = |v: &[u8]| {
            let mut m = HeaderMap::new();
            m.insert(CONTENT_DISPOSITION, HeaderValue::from_bytes(v).unwrap());
            m
        };
        assert_eq!(
            filename_from_headers(&h(b"attachment; filename*=UTF-8''%E7%B4%A0%E6%9D%90.zip")).as_deref(),
            Some("素材.zip")
        );
        assert_eq!(filename_from_headers(&h(b"attachment; filename=\"a b.zip\"")).as_deref(), Some("a b.zip"));
        assert_eq!(
            filename_from_headers(&h("attachment; filename=\"素材.zip\"".as_bytes())).as_deref(),
            Some("素材.zip")
        ); // UTF-8 bytes put directly in the header
        assert_eq!(filename_from_headers(&HeaderMap::new()), None);
    }

    fn body() -> Vec<u8> {
        (0..10_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn opts<'a>() -> SaveOptions<'a> {
        SaveOptions { size: None, sha256: None, headers: HeaderMap::new(), reject_html: false }
    }

    fn noop(_: u64, _: Option<u64>) {}

    fn part_of(target: &std::path::Path) -> std::path::PathBuf {
        let mut s = target.as_os_str().to_owned();
        s.push(".part");
        s.into()
    }

    #[tokio::test]
    async fn save_stream_resumes_from_part() {
        let b = body();
        let mut route = Route::ok("/f.bin", b.clone());
        route.honor_range = true;
        let (base, log) = serve_logged(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        std::fs::write(part_of(&target), &b[..4]).unwrap();
        let calls = Mutex::new(Vec::new());
        let progress = |done: u64, total: Option<u64>| calls.lock().unwrap().push((done, total));
        save_stream(&client(), &format!("{base}/f.bin"), &target, opts(), &progress).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b);
        assert!(!part_of(&target).exists());
        assert_eq!(*log.lock().unwrap(), vec![Some("bytes=4-".to_string())]);
        let calls = calls.lock().unwrap();
        // starts from the resumed offset, with the total derived from offset + content-length
        assert_eq!(calls[0], (4, Some(b.len() as u64)));
        assert_eq!(*calls.last().unwrap(), (b.len() as u64, Some(b.len() as u64)));
    }

    #[tokio::test]
    async fn save_stream_restarts_when_range_ignored() {
        let b = body();
        let base = serve(vec![Route::ok("/f.bin", b.clone())]).await; // honor_range = false
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        std::fs::write(part_of(&target), b"junk").unwrap();
        save_stream(&client(), &format!("{base}/f.bin"), &target, opts(), &noop).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b);
        assert!(!part_of(&target).exists());
    }

    #[tokio::test]
    async fn save_stream_treats_416_as_complete() {
        let b = body();
        let mut route = Route::ok("/f.bin", b.clone());
        route.honor_range = true;
        let (base, log) = serve_logged(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        std::fs::write(part_of(&target), &b).unwrap();
        let o = SaveOptions { size: Some(b.len() as u64), ..opts() };
        let calls = Mutex::new(Vec::new());
        let progress = |done: u64, total: Option<u64>| calls.lock().unwrap().push((done, total));
        save_stream(&client(), &format!("{base}/f.bin"), &target, o, &progress).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b);
        assert!(!part_of(&target).exists());
        assert_eq!(*log.lock().unwrap(), vec![Some(format!("bytes={}-", b.len()))]);
        assert!(calls.lock().unwrap().is_empty(), "no body should have been streamed");
    }

    #[tokio::test]
    async fn save_stream_oversized_part_is_discarded() {
        let b = body();
        let mut route = Route::ok("/f.bin", b.clone());
        route.honor_range = true;
        let base = serve(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        std::fs::write(part_of(&target), vec![7u8; b.len() + 50]).unwrap();
        let url = format!("{base}/f.bin");
        // 416 with Content-Range tells the real length, so the stale .part is detected
        let err = save_stream(&client(), &url, &target, opts(), &noop).await.unwrap_err();
        assert!(matches!(err, DownloadError::Retryable(_)), "{err:?}");
        assert!(!part_of(&target).exists());
        assert!(!target.exists());
        // the retry starts clean
        save_stream(&client(), &url, &target, opts(), &noop).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b);
        assert!(!part_of(&target).exists());
    }

    #[tokio::test]
    async fn save_stream_without_content_length() {
        let b = body();
        let mut route = Route::ok("/f.bin", b.clone());
        route.headers.push(("Transfer-Encoding".into(), "chunked".into()));
        let base = serve(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        let last = Mutex::new(None);
        let progress = |done: u64, total: Option<u64>| {
            *last.lock().unwrap() = Some((done, total));
        };
        save_stream(&client(), &format!("{base}/f.bin"), &target, opts(), &progress).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b);
        assert_eq!(*last.lock().unwrap(), Some((b.len() as u64, None)));
    }

    #[tokio::test]
    async fn save_stream_size_mismatch_is_retryable() {
        let b = body();
        let base = serve(vec![Route::ok("/f.bin", b.clone())]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        let o = SaveOptions { size: Some(b.len() as u64 + 1), ..opts() };
        let err = save_stream(&client(), &format!("{base}/f.bin"), &target, o, &noop).await.unwrap_err();
        assert!(matches!(err, DownloadError::Retryable(_)), "{err:?}");
        assert!(part_of(&target).exists());
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn save_stream_sha256_mismatch_deletes_part() {
        let b = body();
        let base = serve(vec![Route::ok("/f.bin", b.clone())]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        let wrong = "00".repeat(32);
        let o = SaveOptions { sha256: Some(&wrong), ..opts() };
        let err = save_stream(&client(), &format!("{base}/f.bin"), &target, o, &noop).await.unwrap_err();
        assert!(matches!(err, DownloadError::Retryable(_)), "{err:?}");
        assert!(!part_of(&target).exists());
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn save_stream_sha256_match_succeeds() {
        use sha2::{Digest, Sha256};
        let b = body();
        let sha: String = Sha256::digest(&b).iter().map(|x| format!("{x:02X}")).collect();
        let base = serve(vec![Route::ok("/f.bin", b.clone())]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        let o = SaveOptions { sha256: Some(&sha), ..opts() };
        save_stream(&client(), &format!("{base}/f.bin"), &target, o, &noop).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b);
    }

    #[tokio::test]
    async fn save_stream_reject_html() {
        let mut route = Route::ok("/f.bin", b"<html>login</html>".to_vec());
        route.headers.push(("Content-Type".into(), "text/html; charset=utf-8".into()));
        let base = serve(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        let o = SaveOptions { reject_html: true, ..opts() };
        let err = save_stream(&client(), &format!("{base}/f.bin"), &target, o, &noop).await.unwrap_err();
        assert_eq!(err, DownloadError::NotAFile("rejected".into()));
        assert!(!target.exists());
        assert!(!part_of(&target).exists());
    }

    #[tokio::test]
    async fn save_stream_not_found() {
        let base = serve(vec![]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.bin");
        let err = save_stream(&client(), &format!("{base}/nope"), &target, opts(), &noop).await.unwrap_err();
        assert!(matches!(err, DownloadError::NotFound(_)), "{err:?}");
        assert!(!part_of(&target).exists());
    }
}
