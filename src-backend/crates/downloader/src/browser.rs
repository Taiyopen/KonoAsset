//! A hidden WebView2 window driven from Rust, for sites that only work in a real browser
//! (workupload's "Are you a human?" check, the Download button of personal OneDrive).
//!
//! The page has no IPC access. It talks back by navigating to
//! `https://konoasset.invalid/report?d=<encodeURIComponent(JSON)>` (see [`report_js`]); the
//! navigation handler decodes the report and cancels the navigation, so the page stays where it
//! is. Downloads the page starts are cancelled and handed to Rust as a [`DownloadRequest`], which
//! is then fetched with reqwest using the window's cookies; when the server refuses that, the
//! download is repeated natively by WebView2 ([`HiddenBrowser::native_download`]).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, COOKIE, REFERER};
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{AppHandle, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio::time::{timeout, timeout_at, Instant};

use crate::error::DownloadError;
use crate::http::{redact_url, safe_name, save_stream, SaveOptions, USER_AGENT};
use crate::site::{Ctx, FileProgress};

pub const REPORT_HOST: &str = "konoasset.invalid";
/// A download WebView2 performs itself may take as long as the file needs.
pub const NATIVE_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);
const BROWSER_UNAVAILABLE: &str = "browser unavailable";
/// Start of the error [`HiddenBrowser::wait_report`] gives when no report came in time.
pub(crate) const REPORT_TIMEOUT: &str = "browser page timed out";

/// Defines `window.__konoReport(kind, data)`; prepended to every init script.
const REPORT_JS: &str = r#"window.__konoReport = (kind, data) => { location.href = 'https://konoasset.invalid/report?d=' + encodeURIComponent(JSON.stringify({ kind, ...data })) };"#;

/// The shared part of every init script.
pub fn report_js() -> &'static str {
    REPORT_JS
}

/// What a page reported: `{ kind, ...data }`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Report {
    pub kind: String,
    #[serde(flatten)]
    pub data: serde_json::Map<String, serde_json::Value>,
}

impl Report {
    /// String field, `""` when missing or not a string.
    pub fn str(&self, key: &str) -> &str {
        self.data.get(key).and_then(|v| v.as_str()).unwrap_or("")
    }

    /// Boolean field, `false` when missing or not a boolean.
    pub fn bool(&self, key: &str) -> bool {
        self.data.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
    }
}

/// Decodes a report url; `None` for any other url or a malformed report.
pub fn decode_report(url: &Url) -> Option<Report> {
    if url.host_str() != Some(REPORT_HOST) || url.path() != "/report" {
        return None;
    }
    let (_, data) = url.query_pairs().find(|(key, _)| key == "d")?;
    serde_json::from_str(&data).ok()
}

/// A download the page started; it was cancelled in the browser.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadRequest {
    pub url: Url,
    /// File name WebView2 would have used; may be empty.
    pub suggested_name: String,
}

enum NativeEvent {
    Started(Url),
    Finished { path: Option<PathBuf>, success: bool },
}

#[derive(Default)]
struct NativeState {
    /// Where the next download goes. `None`: downloads are cancelled and sent to Rust.
    destination: Option<PathBuf>,
    /// Url of the download currently saved natively.
    started: Option<Url>,
}

fn lock(state: &Mutex<NativeState>) -> MutexGuard<'_, NativeState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether `err` is the timeout of [`HiddenBrowser::wait_report`].
pub(crate) fn is_report_timeout(err: &DownloadError) -> bool {
    matches!(err, DownloadError::Retryable(message) if message.starts_with(REPORT_TIMEOUT))
}

/// The app handle, or `Fatal("browser unavailable")` (tests, or no Tauri runtime).
pub(crate) fn require_app(ctx: &Ctx) -> Result<&AppHandle, DownloadError> {
    ctx.app.as_ref().ok_or_else(|| DownloadError::Fatal(BROWSER_UNAVAILABLE.into()))
}

pub struct HiddenBrowser {
    window: WebviewWindow,
    reports: UnboundedReceiver<Report>,
    downloads: UnboundedReceiver<DownloadRequest>,
    native_events: UnboundedReceiver<NativeEvent>,
    native: Arc<Mutex<NativeState>>,
}

impl HiddenBrowser {
    /// Opens an invisible window at `url`. `init_script` runs on every page (main frame only),
    /// after [`report_js`].
    pub async fn open(app: &AppHandle, url: &Url, init_script: &str) -> Result<Self, DownloadError> {
        let label = format!("dl-{}", uuid::Uuid::new_v4());
        let (report_tx, reports) = unbounded_channel();
        let (download_tx, downloads) = unbounded_channel();
        let (native_tx, native_events) = unbounded_channel();
        let native = Arc::new(Mutex::new(NativeState::default()));
        let native_state = native.clone();
        let nav_label = label.clone();
        let dl_label = label.clone();

        let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url.clone()))
            .title("KonoAsset downloader")
            .visible(false)
            .focused(false)
            .skip_taskbar(true)
            .incognito(true)
            .user_agent(USER_AGENT)
            .initialization_script(format!("{}\n{init_script}", report_js()))
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .on_navigation(move |url| match decode_report(url) {
                Some(report) => {
                    log::debug!("[{nav_label}] report url received: {}", report.kind);
                    let _ = report_tx.send(report);
                    false
                }
                None => {
                    log::debug!("[{nav_label}] navigating to {}", redact_url(url.as_str()));
                    true
                }
            })
            .on_download(move |_, event| match event {
                DownloadEvent::Requested { url, destination } => {
                    let mut state = lock(&native_state);
                    if let Some(dest) = state.destination.take() {
                        log::info!("[{dl_label}] native download of {} to {}", redact_url(url.as_str()), dest.display());
                        *destination = dest;
                        state.started = Some(url.clone());
                        let _ = native_tx.send(NativeEvent::Started(url));
                        true
                    } else {
                        let suggested_name = destination
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        log::info!(
                            "[{dl_label}] download requested from {} (suggested name: {suggested_name:?}), handing it to Rust",
                            url.host_str().unwrap_or("")
                        );
                        let _ = download_tx.send(DownloadRequest { url, suggested_name });
                        false
                    }
                }
                DownloadEvent::Finished { url, path, success } => {
                    let mut state = lock(&native_state);
                    if state.started.as_ref() == Some(&url) {
                        state.started = None;
                        log::info!("[{dl_label}] native download finished, success: {success}, path: {path:?}");
                        let _ = native_tx.send(NativeEvent::Finished { path, success });
                    } else {
                        log::debug!("[{dl_label}] ignoring end of cancelled download {}", redact_url(url.as_str()));
                    }
                    true
                }
                _ => true,
            })
            .build()
            .map_err(|e| DownloadError::Retryable(format!("cannot open browser window: {e}")))?;
        log::info!("[{label}] hidden browser opened at {}", redact_url(url.as_str()));

        Ok(Self { window, reports, downloads, native_events, native })
    }

    /// Navigates the browser in `slot` to `url`, opening one first when there is none (or the
    /// old one stopped working).
    pub(crate) async fn reuse_or_open<'a>(
        slot: &'a mut Option<HiddenBrowser>,
        app: &AppHandle,
        url: &Url,
        init_script: &str,
    ) -> Result<&'a mut HiddenBrowser, DownloadError> {
        if let Some(browser) = slot.as_mut() {
            if let Err(e) = browser.navigate(url) {
                log::warn!("[{}] cannot reuse hidden browser, opening a new one: {e}", browser.window.label());
                *slot = None;
            }
        }
        match slot {
            Some(browser) => Ok(browser),
            None => Ok(slot.insert(Self::open(app, url, init_script).await?)),
        }
    }

    /// Loads `url`. Reports and download requests of the previous page are discarded.
    pub fn navigate(&mut self, url: &Url) -> Result<(), DownloadError> {
        self.discard_pending();
        log::debug!("[{}] navigate to {}", self.window.label(), redact_url(url.as_str()));
        self.window
            .navigate(url.clone())
            .map_err(|e| DownloadError::Retryable(format!("cannot navigate to {}: {e}", redact_url(url.as_str()))))
    }

    fn discard_pending(&mut self) {
        while let Ok(report) = self.reports.try_recv() {
            log::debug!("[{}] discarding stale report {}", self.window.label(), report.kind);
        }
        while let Ok(request) = self.downloads.try_recv() {
            log::debug!("[{}] discarding stale download {}", self.window.label(), redact_url(request.url.as_str()));
        }
        while self.native_events.try_recv().is_ok() {}
    }

    /// Runs `js` in the current page.
    pub fn eval(&self, js: &str) -> Result<(), DownloadError> {
        self.window
            .eval(js)
            .map_err(|e| DownloadError::Retryable(format!("cannot run script in browser: {e}")))
    }

    pub fn current_url(&self) -> Option<Url> {
        self.window.url().ok()
    }

    /// The first report whose kind is in `kinds`; other reports are logged and skipped.
    pub async fn wait_report(&mut self, timeout: Duration, kinds: &[&str]) -> Result<Report, DownloadError> {
        let deadline = Instant::now() + timeout;
        loop {
            match timeout_at(deadline, self.reports.recv()).await {
                Ok(Some(report)) if kinds.contains(&report.kind.as_str()) => {
                    log::info!(
                        "[{}] report {} received, title: {:?}",
                        self.window.label(),
                        report.kind,
                        report.str("title")
                    );
                    return Ok(report);
                }
                Ok(Some(report)) => {
                    log::debug!("[{}] skipping report {} (waiting for {kinds:?})", self.window.label(), report.kind);
                }
                Ok(None) => return Err(DownloadError::Retryable("browser window closed".into())),
                Err(_) => {
                    let at = self.current_url().map(|u| redact_url(u.as_str())).unwrap_or_default();
                    return Err(DownloadError::Retryable(format!(
                        "{REPORT_TIMEOUT}: no {kinds:?} within {}s (at {at})",
                        timeout.as_secs()
                    )));
                }
            }
        }
    }

    /// The next download the page starts (cancelled in the browser).
    pub async fn wait_download(&mut self, timeout_after: Duration) -> Result<DownloadRequest, DownloadError> {
        match timeout(timeout_after, self.downloads.recv()).await {
            Ok(Some(request)) => Ok(request),
            Ok(None) => Err(DownloadError::Retryable("browser window closed".into())),
            Err(_) => Err(DownloadError::Retryable("download did not start".into())),
        }
    }

    /// `Cookie` header value for `url`, including HTTP-only cookies.
    ///
    /// Must run on a tokio worker thread: on Windows reading cookies from the main thread
    /// deadlocks (see the Tauri docs of `cookies_for_url`).
    pub fn cookie_header(&self, url: &Url) -> Result<String, DownloadError> {
        let cookies = self
            .window
            .cookies_for_url(url.clone())
            .map_err(|e| DownloadError::Retryable(format!("cannot read browser cookies: {e}")))?;
        Ok(cookies.iter().map(|c| format!("{}={}", c.name(), c.value())).collect::<Vec<_>>().join("; "))
    }

    /// Lets WebView2 itself save the next download to `dest`, triggered by loading
    /// `native.trigger`, and waits until it finished. The download must start within
    /// `native.start_timeout` (a report whose kind is in `native.fail_reports` gives up at once);
    /// once started it may take up to [`NATIVE_TIMEOUT`].
    pub async fn native_download(&mut self, native: &NativeFallback<'_>, dest: &Path) -> Result<(), DownloadError> {
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        *lock(&self.native) = NativeState { destination: Some(dest.to_path_buf()), started: None };
        let result = self.native_wait(native).await;
        *lock(&self.native) = NativeState::default();

        let path = result?;
        if path != dest {
            log::info!("[{}] moving native download {} to {}", self.window.label(), path.display(), dest.display());
            tokio::fs::rename(&path, dest).await?;
        }
        Ok(())
    }

    async fn native_wait(&mut self, native: &NativeFallback<'_>) -> Result<PathBuf, DownloadError> {
        self.navigate(native.trigger)?;
        let label = self.window.label().to_string();
        let mut deadline = Instant::now() + native.start_timeout;
        let mut started = false;
        loop {
            tokio::select! {
                event = self.native_events.recv() => match event {
                    Some(NativeEvent::Started(url)) => {
                        log::info!("[{label}] native download started from {}", redact_url(url.as_str()));
                        started = true;
                        deadline = Instant::now() + NATIVE_TIMEOUT;
                    }
                    Some(NativeEvent::Finished { path: Some(path), success: true }) => return Ok(path),
                    Some(NativeEvent::Finished { .. }) => {
                        return Err(DownloadError::Retryable("browser download failed".into()));
                    }
                    None => return Err(DownloadError::Retryable("browser window closed".into())),
                },
                Some(report) = self.reports.recv(), if !started => {
                    if native.fail_reports.contains(&report.kind.as_str()) {
                        log::info!("[{label}] report {} while waiting for the native download", report.kind);
                        return Err(DownloadError::Retryable(format!("download did not start ({})", report.kind)));
                    }
                    log::debug!("[{label}] report {} while waiting for the native download", report.kind);
                }
                _ = tokio::time::sleep_until(deadline) => {
                    return Err(DownloadError::Retryable(
                        if started { "browser download timed out" } else { "download did not start" }.into(),
                    ));
                }
            }
        }
    }
}

impl Drop for HiddenBrowser {
    fn drop(&mut self) {
        log::info!("[{}] closing hidden browser", self.window.label());
        if let Err(e) = self.window.destroy() {
            log::error!("[{}] cannot close hidden browser: {e}", self.window.label());
        }
    }
}

/// How to repeat a download natively when the reqwest download is refused.
pub(crate) struct NativeFallback<'a> {
    /// Loading this url starts the download again.
    pub trigger: &'a Url,
    /// The download must start within this time.
    pub start_timeout: Duration,
    /// Reports that mean the download will not start (e.g. `no-button`).
    pub fail_reports: &'a [&'a str],
}

/// Whether a refused reqwest download should be repeated by the browser: the server answered,
/// but with a page or an HTTP error status (`save_stream` maps 404 to `NotFound` and other
/// statuses to `Retryable("HTTP <code> ...")`). Transport and I/O errors are not.
pub(crate) fn should_fallback_native(err: &DownloadError) -> bool {
    match err {
        DownloadError::NotAFile(_)
        | DownloadError::NotFound(_)
        | DownloadError::LoginRequired(_)
        | DownloadError::RateLimited(_) => true,
        DownloadError::Retryable(message) => message
            .strip_prefix("HTTP ")
            .and_then(|rest| rest.get(..3))
            .and_then(|code| code.parse::<u16>().ok())
            .is_some_and(|code| (400..500).contains(&code)),
        _ => false,
    }
}

/// Saves a download the page requested to `out_dir/<safe suggested name>`: first with reqwest
/// and the window's cookies (renamed to the server's file name when it sends one), and when the
/// server refuses that ([`should_fallback_native`]), natively as described by `native`. Returns
/// right away when the file is already there.
pub(crate) async fn save_download(
    browser: &mut HiddenBrowser,
    http: &reqwest::Client,
    request: DownloadRequest,
    referer: Option<&str>,
    native: &NativeFallback<'_>,
    out_dir: &Path,
    progress: FileProgress<'_>,
) -> Result<PathBuf, DownloadError> {
    let name = if request.suggested_name.is_empty() {
        crate::sites::name_from_url(request.url.as_str()).unwrap_or_default()
    } else {
        request.suggested_name.clone()
    };
    let file_name = safe_name(&name);
    let target = out_dir.join(&file_name);
    if tokio::fs::try_exists(&target).await.unwrap_or(false) {
        log::info!("{} already exists, skipping", target.display());
        return Ok(target);
    }

    let mut headers = HeaderMap::new();
    let cookie = browser.cookie_header(&request.url)?;
    let cookie_ok = match HeaderValue::from_str(&cookie) {
        Ok(value) => {
            if !cookie.is_empty() {
                headers.insert(COOKIE, value);
            }
            true
        }
        Err(_) => false,
    };
    if let Some(referer) = referer {
        headers.insert(REFERER, HeaderValue::from_str(referer).map_err(|e| DownloadError::Fatal(e.to_string()))?);
    }

    if cookie_ok {
        log::info!(
            "downloading {file_name} from {} with {} browser cookie(s)",
            request.url.host_str().unwrap_or(""),
            cookie.split("; ").filter(|c| !c.is_empty()).count()
        );
        match save_requested(http, request.url.as_str(), headers, out_dir, &file_name, progress).await {
            Ok(path) => return Ok(path),
            Err(e) if should_fallback_native(&e) => {
                log::info!("server refused the direct download of {file_name} ({e}), letting the browser download it");
            }
            Err(e) => return Err(e),
        }
    } else {
        log::info!("browser cookies cannot be sent as a header, letting the browser download {file_name}");
    }

    progress(&file_name, 0, None);
    browser.native_download(native, &target).await?;
    Ok(target)
}

/// Saves `url` to `out_dir/<file_name>` with reqwest, then renames it to the name the server gives:
/// WebView2's suggested name may be de-duplicated against the user's Downloads (`a (1).zip`).
async fn save_requested(
    http: &reqwest::Client,
    url: &str,
    headers: HeaderMap,
    out_dir: &Path,
    file_name: &str,
    progress: FileProgress<'_>,
) -> Result<PathBuf, DownloadError> {
    let target = out_dir.join(file_name);
    let opts = SaveOptions { size: None, sha256: None, headers, reject_html: true };
    let header_name = save_stream(http, url, &target, opts, &|done, total| progress(file_name, done, total)).await?;
    match header_name.map(|name| safe_name(&name)).filter(|name| name != file_name) {
        Some(name) => {
            let renamed = out_dir.join(name);
            tokio::fs::rename(&target, &renamed).await?;
            Ok(renamed)
        }
        None => Ok(target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Url;

    #[test]
    fn decodes_report_url() {
        let url = Url::parse("https://konoasset.invalid/report?d=%7B%22kind%22%3A%22ready%22%2C%22title%22%3A%22a.zip%22%2C%22hasPassword%22%3Afalse%7D").unwrap();
        let r = decode_report(&url).unwrap();
        assert_eq!(r.kind, "ready");
        assert_eq!(r.data["title"], "a.zip");
        assert!(decode_report(&Url::parse("https://workupload.com/file/abc").unwrap()).is_none());
        assert!(decode_report(&Url::parse("https://konoasset.invalid/report?d=not-json").unwrap()).is_none());
    }

    #[test]
    fn falls_back_to_native_when_the_server_refuses() {
        use DownloadError::*;
        for err in [
            NotAFile("rejected".into()),
            NotFound("u".into()),
            LoginRequired("u".into()),
            RateLimited("u".into()),
            Retryable("HTTP 400 Bad Request for u".into()),
            Retryable("HTTP 405 Method Not Allowed for u".into()),
            Retryable("HTTP 429 Too Many Requests for u".into()),
        ] {
            assert!(should_fallback_native(&err), "{err:?}");
        }
        for err in [
            Retryable("HTTP 502 Bad Gateway for u".into()),
            Retryable("error sending request for url (u)".into()),
            Retryable("size mismatch: got 1 bytes, expected 2".into()),
            Fatal("x".into()),
            WrongPassword,
        ] {
            assert!(!should_fallback_native(&err), "{err:?}");
        }
    }

    #[tokio::test]
    async fn saved_under_the_server_name_not_the_deduplicated_one() {
        use crate::test_server::{serve, Route};
        let mut named = Route::ok("/named", b"zip".to_vec());
        named.headers.push(("Content-Disposition".into(), "attachment; filename=\"Kirke_.zip\"".into()));
        let base = serve(vec![named, Route::ok("/plain", b"zip".to_vec())]).await;
        let dir = tempfile::tempdir().unwrap();
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let save = |path: &str| {
            let url = format!("{base}{path}");
            let out_dir = dir.path().to_path_buf();
            async move { save_requested(&crate::client(), &url, HeaderMap::new(), &out_dir, "Kirke_ (1).zip", &noop).await }
        };

        assert_eq!(save("/named").await.unwrap(), dir.path().join("Kirke_.zip"));
        assert!(!dir.path().join("Kirke_ (1).zip").exists());
        // Without a name from the server the suggested one stays.
        assert_eq!(save("/plain").await.unwrap(), dir.path().join("Kirke_ (1).zip"));
    }

    #[test]
    fn report_timeouts_are_recognized() {
        assert!(is_report_timeout(&DownloadError::Retryable(format!("{REPORT_TIMEOUT}: no [\"ready\"] within 90s"))));
        assert!(!is_report_timeout(&DownloadError::Retryable("browser window closed".into())));
        assert!(!is_report_timeout(&DownloadError::Fatal(REPORT_TIMEOUT.into())));
    }
}
