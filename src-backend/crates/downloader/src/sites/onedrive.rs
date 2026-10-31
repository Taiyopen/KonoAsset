//! OneDrive.
//!
//! * Company/school SharePoint links (`*.sharepoint.com`): adding `download=1` to the url makes
//!   the server answer with the file itself.
//! * Personal OneDrive (`1drv.ms`, `onedrive.live.com`): the share page is opened in a hidden
//!   browser and its Download button clicked. A shared folder comes as one zip.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::Url;

use crate::browser::{require_app, save_download, HiddenBrowser, NativeFallback};
use crate::error::DownloadError;
use crate::link::Job;
use crate::site::{Ctx, FileItem, FileProgress, Site};

use super::{probe, save_named};

const BUTTON_TIMEOUT: Duration = Duration::from_secs(60);
/// A folder is zipped first, which may take a while.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);

/// Once the page has loaded, looks for a visible Download button every second (the selectors
/// differ between OneDrive versions), reports `clicked` and clicks it. The report goes first: it
/// is a navigation and would cancel a download the click starts by navigating. Reports
/// `no-button` after 45 s. Login pages get no script: public links may pass through a silent
/// login page, so a login is only assumed when the page is still there at the end.
const INIT_JS: &str = r#"(() => {
  const host = location.hostname;
  if (!host.includes('onedrive') && !host.includes('1drv')) return;
  const selectors = [
    "button[data-automationid='download']",
    "button[data-automationid='downloadCommand']",
    "[role='menuitem'][name='Download']",
    "button[name='Download']",
    "button[aria-label^='Download']",
  ];
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== 'hidden';
  };
  const start = () => {
    let waited = 0;
    const timer = setInterval(() => {
      for (const selector of selectors) {
        const el = document.querySelector(selector);
        if (el && visible(el)) {
          clearInterval(timer);
          window.__konoReport('clicked', { title: document.title, selector });
          setTimeout(() => el.click(), 1000);
          return;
        }
      }
      waited += 1000;
      if (waited >= 45000) {
        clearInterval(timer);
        window.__konoReport('no-button', { title: document.title });
      }
    }, 1000);
  };
  if (document.readyState === 'complete') start(); else addEventListener('load', start);
})();"#;

#[derive(Default)]
pub struct OneDrive {
    browser: Option<HiddenBrowser>,
}

impl OneDrive {
    pub fn new() -> Self {
        Self::default()
    }

    async fn download_personal(
        &mut self,
        ctx: &Ctx,
        url: &str,
        out_dir: &Path,
        progress: FileProgress<'_>,
    ) -> Result<PathBuf, DownloadError> {
        let app = require_app(ctx)?;
        let share = Url::parse(url).map_err(|e| DownloadError::Fatal(format!("bad OneDrive url {url}: {e}")))?;
        let browser = HiddenBrowser::reuse_or_open(&mut self.browser, app, &share, INIT_JS).await?;
        let report = match browser.wait_report(BUTTON_TIMEOUT, &["clicked", "no-button"]).await {
            Ok(report) => report,
            Err(_) if is_login(browser.current_url()) => return Err(login_required(url)),
            Err(e) => return Err(e),
        };
        if report.kind == "no-button" {
            if is_login(browser.current_url()) {
                return Err(login_required(url));
            }
            return Err(DownloadError::Retryable("no Download button on the OneDrive page".into()));
        }
        log::info!("onedrive: Download button clicked ({})", report.str("selector"));
        let request = browser.wait_download(DOWNLOAD_TIMEOUT).await?;
        let native = NativeFallback { trigger: &share, start_timeout: DOWNLOAD_TIMEOUT, fail_reports: &["no-button"] };
        save_download(browser, &ctx.http, request, None, &native, out_dir, progress).await
    }
}

fn is_login(url: Option<Url>) -> bool {
    url.is_some_and(|u| u.as_str().contains("login.live.com") || u.as_str().contains("login.microsoftonline.com"))
}

fn login_required(url: &str) -> DownloadError {
    DownloadError::LoginRequired(format!("{url} needs a login or is not shared publicly"))
}

fn is_sharepoint(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .is_some_and(|h| h.ends_with(".sharepoint.com"))
}

/// Adds (or overrides) the `download=1` query parameter, keeping the rest of the url untouched.
pub(crate) fn with_download_param(url: &str) -> String {
    let (rest, fragment) = match url.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (url, None),
    };
    let (base, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut params: Vec<&str> = query
        .split('&')
        .filter(|p| !p.is_empty() && !p.starts_with("download=") && *p != "download")
        .collect();
    params.push("download=1");
    let mut out = format!("{base}?{}", params.join("&"));
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

pub(crate) async fn download_sharepoint(
    client: &reqwest::Client,
    url: &str,
    out_dir: &Path,
    progress: FileProgress<'_>,
) -> Result<PathBuf, DownloadError> {
    let download_url = with_download_param(url);
    let head = probe(client, &download_url).await?;
    // Without a file name the server sent a login page instead of the file.
    let name = head
        .name
        .ok_or_else(|| DownloadError::LoginRequired(format!("{url} needs a login or is not shared publicly")))?;
    save_named(client, &head.final_url, &name, head.size, out_dir, progress, false).await
}

#[async_trait::async_trait]
impl Site for OneDrive {
    async fn list_files(&mut self, ctx: &Ctx, job: &Job) -> Result<(Option<String>, Vec<FileItem>), DownloadError> {
        if !is_sharepoint(&job.id) {
            require_app(ctx)?;
        }
        Ok((None, vec![FileItem { id: job.id.clone(), subdir: String::new(), name: None }]))
    }

    async fn download(
        &mut self,
        ctx: &Ctx,
        item: &FileItem,
        _password: &str,
        out_dir: &Path,
        progress: FileProgress<'_>,
    ) -> Result<PathBuf, DownloadError> {
        if !is_sharepoint(&item.id) {
            return self.download_personal(ctx, &item.id, out_dir, progress).await;
        }
        download_sharepoint(&ctx.http, &item.id, out_dir, progress).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{Job, JobKind};
    use crate::site::{Ctx, Site};
    use crate::test_server::{serve, Route};
    use crate::{client, DownloadError};

    #[test]
    fn sharepoint_adds_download_param() {
        assert_eq!(
            with_download_param("https://x.sharepoint.com/:u:/g/a?e=1"),
            "https://x.sharepoint.com/:u:/g/a?e=1&download=1"
        );
        assert_eq!(with_download_param("https://x.sharepoint.com/a"), "https://x.sharepoint.com/a?download=1");
        assert_eq!(
            with_download_param("https://x.sharepoint.com/a?download=0&e=1#frag"),
            "https://x.sharepoint.com/a?e=1&download=1#frag"
        );
    }

    #[tokio::test]
    async fn sharepoint_downloads_with_download_param() {
        let mut route = Route::ok("/s/file", vec![7u8; 2_000]);
        route
            .headers
            .push(("Content-Disposition".into(), "attachment; filename*=UTF-8''%E7%B4%A0%E6%9D%90.zip".into()));
        let base = serve(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let path = download_sharepoint(&client(), &format!("{base}/s/file?e=1"), dir.path(), &noop).await.unwrap();
        assert_eq!(path, dir.path().join("素材.zip"));
        assert_eq!(std::fs::read(path).unwrap().len(), 2_000);
    }

    #[tokio::test]
    async fn sharepoint_without_filename_needs_login() {
        let mut route = Route::ok("/s/file", b"<html>login</html>".to_vec());
        route.headers.push(("Content-Type".into(), "text/html".into()));
        let base = serve(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let err = download_sharepoint(&client(), &format!("{base}/s/file"), dir.path(), &noop).await.unwrap_err();
        assert!(matches!(err, DownloadError::LoginRequired(_)), "{err:?}");
    }

    #[tokio::test]
    async fn personal_onedrive_needs_a_browser() {
        let ctx = Ctx { app: None, http: client() };
        let unavailable = DownloadError::Fatal("browser unavailable".into());
        let url = "https://1drv.ms/u/s!abc";
        let job = Job { kind: JobKind::File, id: url.into(), password: String::new() };
        assert_eq!(OneDrive::new().list_files(&ctx, &job).await.unwrap_err(), unavailable);
        let item = FileItem { id: url.into(), subdir: String::new(), name: None };
        let dir = tempfile::tempdir().unwrap();
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let err = OneDrive::new().download(&ctx, &item, "", dir.path(), &noop).await.unwrap_err();
        assert_eq!(err, unavailable);
    }

    #[test]
    fn login_pages_are_detected() {
        assert!(is_login(Url::parse("https://login.live.com/oauth20_authorize.srf?x=1").ok()));
        assert!(is_login(Url::parse("https://login.microsoftonline.com/common").ok()));
        assert!(!is_login(Url::parse("https://onedrive.live.com/?id=abc").ok()));
        assert!(!is_login(None));
    }
}
