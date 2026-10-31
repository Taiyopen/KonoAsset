//! workupload: the pages run an "Are you a human?" check in JavaScript, so everything goes
//! through a hidden browser that lets the check finish.
//!
//! * `/file/<id>`: one file. `/archive/<id>`: a folder, its files are the `/file/` links.
//! * Both may ask for a password first.
//! * Loading `/start/<id>` makes the page start the download by itself.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::Url;

use crate::browser::{is_report_timeout, require_app, save_download, HiddenBrowser, NativeFallback, Report};
use crate::error::DownloadError;
use crate::http::safe_name;
use crate::link::{Job, JobKind};
use crate::site::{Ctx, FileItem, FileProgress, Site};

const BASE: &str = "https://workupload.com";
const READY_TIMEOUT: Duration = Duration::from_secs(90);
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// Reports `ready` once the human check is over. `/start/` pages are left alone: they navigate
/// to the download by themselves and a report would get in the way.
const INIT_JS: &str = r#"(() => {
  if (location.hostname !== 'workupload.com' || location.pathname.startsWith('/start/')) return;
  window.__konoReady = () => {
    const fileIds = [];
    for (const a of document.querySelectorAll("a[href^='/file/']")) {
      const m = /^\/file\/([A-Za-z0-9]+)/.exec(a.getAttribute('href'));
      if (m && !fileIds.includes(m[1])) fileIds.push(m[1]);
    }
    window.__konoReport('ready', {
      title: document.title.trim(),
      hasPassword: !!document.querySelector("input[type=password][id^=passwordprotected_]"),
      fileIds,
    });
  };
  const timer = setInterval(() => {
    if (document.readyState !== 'loading' && !document.title.includes('human')) {
      clearInterval(timer);
      window.__konoReady();
    }
  }, 500);
})();"#;

/// Fills in the password and submits. A submit that reloads the page gets a fresh `ready` from
/// the init script; when the page stays (answered in place), `ready` is reported after 5 s.
const PASSWORD_JS: &str = r#"(() => {
  const input = document.querySelector("input[type=password][id^=passwordprotected_]");
  const button = document.querySelector("button[type=submit][id^=passwordprotected_]");
  if (!input || !button) { window.__konoReady(); return; }
  input.value = __PASSWORD__;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  input.dispatchEvent(new Event('change', { bubbles: true }));
  const timer = setTimeout(() => window.__konoReady(), 5000);
  addEventListener('beforeunload', () => clearTimeout(timer));
  button.click();
})();"#;

/// Seconds to wait before retry 2, 3 and 4 after a connection error.
const CONNECTION_WAITS: [u64; 3] = [60, 180, 300];

#[derive(Default)]
pub struct Workupload {
    browser: Option<HiddenBrowser>,
}

impl Workupload {
    pub fn new() -> Self {
        Self::default()
    }
}

fn page_url(path: &str, id: &str) -> Result<Url, DownloadError> {
    Url::parse(&format!("{BASE}/{path}/{id}")).map_err(|e| DownloadError::Fatal(format!("bad workupload id {id}: {e}")))
}

/// [`PASSWORD_JS`] with `password` as a JS string literal.
fn password_js(password: &str) -> Result<String, DownloadError> {
    let literal = serde_json::to_string(password).map_err(|e| DownloadError::Fatal(e.to_string()))?;
    Ok(PASSWORD_JS.replace("__PASSWORD__", &literal))
}

/// Waits for the page to be ready and enters the password when it asks for one.
async fn unlock(browser: &mut HiddenBrowser, password: &str) -> Result<Report, DownloadError> {
    let report = browser.wait_report(READY_TIMEOUT, &["ready"]).await?;
    if !report.bool("hasPassword") {
        return Ok(report);
    }
    if password.is_empty() {
        return Err(DownloadError::PasswordRequired);
    }
    browser.eval(&password_js(password)?)?;
    log::info!("workupload: password submitted");
    let report = browser.wait_report(READY_TIMEOUT, &["ready"]).await?;
    if report.bool("hasPassword") {
        return Err(DownloadError::WrongPassword);
    }
    Ok(report)
}

impl Workupload {
    /// Opens `url` (first time: a new hidden browser) and unlocks it.
    async fn open_unlocked(&mut self, ctx: &Ctx, url: &Url, password: &str) -> Result<Report, DownloadError> {
        let app = require_app(ctx)?;
        let browser = HiddenBrowser::reuse_or_open(&mut self.browser, app, url, INIT_JS).await?;
        unlock(browser, password).await
    }
}

#[async_trait::async_trait]
impl Site for Workupload {
    async fn list_files(&mut self, ctx: &Ctx, job: &Job) -> Result<(Option<String>, Vec<FileItem>), DownloadError> {
        require_app(ctx)?;
        if job.kind == JobKind::File {
            return Ok((None, vec![FileItem { id: job.id.clone(), subdir: String::new(), name: None }]));
        }
        let report = self.open_unlocked(ctx, &page_url("archive", &job.id)?, &job.password).await?;
        let ids: Vec<String> = report
            .data
            .get("fileIds")
            .and_then(|v| v.as_array())
            .map(|ids| ids.iter().filter_map(|id| id.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        log::info!("workupload: archive {} lists {} file(s)", job.id, ids.len());
        let items = ids.into_iter().map(|id| FileItem { id, subdir: String::new(), name: None }).collect();
        Ok((Some(job.id.clone()), items))
    }

    async fn download(
        &mut self,
        ctx: &Ctx,
        item: &FileItem,
        password: &str,
        out_dir: &Path,
        progress: FileProgress<'_>,
    ) -> Result<PathBuf, DownloadError> {
        let report = self.open_unlocked(ctx, &page_url("file", &item.id)?, password).await?;
        let title = report.str("title").trim();
        if !title.is_empty() && title != "workupload" {
            let existing = out_dir.join(safe_name(title));
            if tokio::fs::try_exists(&existing).await.unwrap_or(false) {
                log::info!("workupload: {} already exists, skipping", existing.display());
                return Ok(existing);
            }
        }

        let browser = self.browser.as_mut().ok_or_else(|| DownloadError::Retryable("browser window closed".into()))?;
        let start = page_url("start", &item.id)?;
        browser.navigate(&start)?;
        let request = browser.wait_download(START_TIMEOUT).await?;
        let native = NativeFallback { trigger: &start, start_timeout: START_TIMEOUT, fail_reports: &[] };
        save_download(browser, &ctx.http, request, Some("https://workupload.com/"), &native, out_dir, progress).await
    }

    /// Too many requests in a row get the IP blocked for a while.
    fn delay(&self) -> Duration {
        Duration::from_secs(10)
    }

    /// Connection errors usually mean the site blocked us for a while: wait longer. WebView2
    /// shows its own error page instead of telling us about them, so a page that never gets
    /// `ready` counts as one too.
    fn retry_wait(&self, attempt: u32, err: &DownloadError) -> Duration {
        let message = err.to_string();
        if message.contains("ERR_CONNECTION") || message.contains("ERR_TIMED_OUT") || is_report_timeout(err) {
            return Duration::from_secs(CONNECTION_WAITS[(attempt.clamp(1, 3) - 1) as usize]);
        }
        Duration::from_secs(20 * attempt as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client;

    #[test]
    fn waits_between_files_and_longer_after_connection_errors() {
        let site = Workupload::new();
        assert_eq!(site.delay(), Duration::from_secs(10));
        let blocked = DownloadError::Retryable("net::ERR_CONNECTION_RESET".into());
        assert_eq!(site.retry_wait(1, &blocked), Duration::from_secs(60));
        assert_eq!(site.retry_wait(2, &DownloadError::Retryable("ERR_TIMED_OUT".into())), Duration::from_secs(180));
        assert_eq!(site.retry_wait(5, &blocked), Duration::from_secs(300));
        assert_eq!(site.retry_wait(2, &DownloadError::Retryable("HTTP 502".into())), Duration::from_secs(40));
    }

    #[test]
    fn page_that_never_gets_ready_waits_like_a_connection_error() {
        let site = Workupload::new();
        let timeout = DownloadError::Retryable(format!(
            "{}: no [\"ready\"] within 90s (at workupload.com/file/abc)",
            crate::browser::REPORT_TIMEOUT
        ));
        assert_eq!(site.retry_wait(1, &timeout), Duration::from_secs(60));
        assert_eq!(site.retry_wait(2, &timeout), Duration::from_secs(180));
        assert_eq!(site.retry_wait(3, &timeout), Duration::from_secs(300));
        let closed = DownloadError::Retryable("browser window closed".into());
        assert_eq!(site.retry_wait(1, &closed), Duration::from_secs(20));
    }

    #[test]
    fn password_is_injected_as_an_escaped_literal() {
        let password = "a\"b\\c\nd</script>";
        let js = password_js(password).unwrap();
        assert!(js.contains(r#"input.value = "a\"b\\c\nd</script>";"#), "{js}");
        // The literal reads back as the same password.
        let start = js.find("input.value = ").unwrap() + "input.value = ".len();
        let end = start + js[start..].find(";\n").unwrap();
        assert_eq!(serde_json::from_str::<String>(&js[start..end]).unwrap(), password);
        assert!(!js.contains("__PASSWORD__"));
    }

    #[tokio::test]
    async fn needs_a_browser() {
        let ctx = Ctx { app: None, http: client() };
        let unavailable = DownloadError::Fatal("browser unavailable".into());
        for kind in [JobKind::File, JobKind::Folder] {
            let job = Job { kind, id: "abc".into(), password: String::new() };
            assert_eq!(Workupload::new().list_files(&ctx, &job).await.unwrap_err(), unavailable);
        }
        let item = FileItem { id: "abc".into(), subdir: String::new(), name: None };
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let dir = tempfile::tempdir().unwrap();
        let err = Workupload::new().download(&ctx, &item, "", dir.path(), &noop).await.unwrap_err();
        assert_eq!(err, unavailable);
    }
}
