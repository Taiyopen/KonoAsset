//! The file hosting sites and the factory that picks one for a [`SiteKind`].

mod direct;
mod gdrive;
mod onedrive;
mod pixeldrain;

use std::path::{Path, PathBuf};

use reqwest::header::{HeaderMap, CONTENT_TYPE};
use reqwest::StatusCode;

use crate::error::DownloadError;
use crate::http::{filename_from_headers, percent_decode, safe_name, save_stream, SaveOptions};
use crate::link::{Job, SiteKind};
use crate::site::{Ctx, FileItem, FileProgress, Site};

pub const PIXELDRAIN_API: &str = "https://pixeldrain.com/api";

/// Creates the [`Site`] implementation for `kind`.
pub fn create(kind: SiteKind) -> Box<dyn Site> {
    match kind {
        SiteKind::Direct => Box::new(direct::Direct),
        SiteKind::Pixeldrain => Box::new(pixeldrain::Pixeldrain::new(PIXELDRAIN_API.to_string())),
        SiteKind::OneDrive => Box::new(onedrive::OneDrive::new()),
        SiteKind::GoogleDrive => Box::new(gdrive::GoogleDrive::new()),
        // Replaced by a real implementation in a later task.
        SiteKind::Workupload => Box::new(NotImplemented),
    }
}

/// Placeholder for sites that are not implemented yet.
struct NotImplemented;

#[async_trait::async_trait]
impl Site for NotImplemented {
    async fn list_files(&mut self, _ctx: &Ctx, _job: &Job) -> Result<(Option<String>, Vec<FileItem>), DownloadError> {
        Err(DownloadError::Fatal("not implemented".into()))
    }

    async fn download(
        &mut self,
        _ctx: &Ctx,
        _item: &FileItem,
        _password: &str,
        _out_dir: &Path,
        _progress: FileProgress<'_>,
    ) -> Result<PathBuf, DownloadError> {
        Err(DownloadError::Fatal("not implemented".into()))
    }
}

/// Maps the status of a header/API request to the matching error. `Ok` for success statuses.
/// (`save_stream` itself only knows 404; everything else is `Retryable` there.)
pub(crate) fn check_status(status: StatusCode, url: &str) -> Result<(), DownloadError> {
    match status {
        s if s.is_success() => Ok(()),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(DownloadError::LoginRequired(url.to_string())),
        StatusCode::NOT_FOUND | StatusCode::GONE => Err(DownloadError::NotFound(url.to_string())),
        StatusCode::TOO_MANY_REQUESTS => Err(DownloadError::RateLimited(url.to_string())),
        s => Err(DownloadError::Retryable(format!("HTTP {s} for {url}"))),
    }
}

/// What the response headers of a file url say; the body is never read.
pub(crate) struct Probe {
    /// Url after redirects.
    pub final_url: String,
    /// From `Content-Disposition`.
    pub name: Option<String>,
    pub size: Option<u64>,
    pub is_html: bool,
}

pub(crate) async fn probe(client: &reqwest::Client, url: &str) -> Result<Probe, DownloadError> {
    let response = client.get(url).send().await?;
    check_status(response.status(), url)?;
    Ok(Probe {
        final_url: response.url().to_string(),
        name: filename_from_headers(response.headers()),
        size: response.content_length(),
        is_html: response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.to_ascii_lowercase().contains("text/html")),
    })
}

/// Last non-empty path segment of `url`, percent-decoded.
pub(crate) fn name_from_url(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    let segment = url.path_segments()?.rfind(|s| !s.is_empty())?;
    Some(percent_decode(segment))
}

/// Downloads `url` to `out_dir/<safe name>`; returns right away when a file of the same size is
/// already there. With `reject_html`, an HTML answer fails with `NotAFile` instead of being saved.
pub(crate) async fn save_named(
    client: &reqwest::Client,
    url: &str,
    name: &str,
    size: Option<u64>,
    out_dir: &Path,
    progress: FileProgress<'_>,
    reject_html: bool,
) -> Result<PathBuf, DownloadError> {
    let file_name = safe_name(name);
    let target = out_dir.join(&file_name);
    if let (Some(size), Ok(meta)) = (size, tokio::fs::metadata(&target).await) {
        if meta.len() == size {
            return Ok(target);
        }
    }
    let opts = SaveOptions { size, sha256: None, headers: HeaderMap::new(), reject_html };
    save_stream(client, url, &target, opts, &|done, total| progress(&file_name, done, total)).await?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_status_maps_codes() {
        assert!(check_status(StatusCode::OK, "u").is_ok());
        assert!(matches!(check_status(StatusCode::FORBIDDEN, "u"), Err(DownloadError::LoginRequired(_))));
        assert!(matches!(check_status(StatusCode::UNAUTHORIZED, "u"), Err(DownloadError::LoginRequired(_))));
        assert!(matches!(check_status(StatusCode::GONE, "u"), Err(DownloadError::NotFound(_))));
        assert!(matches!(check_status(StatusCode::TOO_MANY_REQUESTS, "u"), Err(DownloadError::RateLimited(_))));
        assert!(check_status(StatusCode::BAD_GATEWAY, "u").unwrap_err().is_retryable());
    }

    #[test]
    fn name_from_url_decodes_last_segment() {
        assert_eq!(name_from_url("https://h/a/b%20c.zip?x=1").as_deref(), Some("b c.zip"));
        assert_eq!(name_from_url("https://h/").as_deref(), None);
    }

    #[tokio::test]
    async fn stub_sites_are_not_implemented() {
        let ctx = Ctx { app: None, http: crate::client() };
        let job = Job { kind: crate::link::JobKind::File, id: "x".into(), password: String::new() };
        for kind in [SiteKind::Workupload] {
            let err = create(kind).list_files(&ctx, &job).await.unwrap_err();
            assert_eq!(err, DownloadError::Fatal("not implemented".into()));
        }
    }
}
