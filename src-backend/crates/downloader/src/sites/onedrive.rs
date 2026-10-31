//! OneDrive. This file handles company/school SharePoint links (`*.sharepoint.com`), where
//! adding `download=1` to the url makes the server answer with the file itself. Personal
//! OneDrive links need a webview and are not implemented yet.

use std::path::{Path, PathBuf};

use crate::error::DownloadError;
use crate::link::Job;
use crate::site::{Ctx, FileItem, FileProgress, Site};

use super::{probe, save_named};

#[derive(Default)]
pub struct OneDrive;

impl OneDrive {
    pub fn new() -> Self {
        Self
    }
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
    async fn list_files(&mut self, _ctx: &Ctx, job: &Job) -> Result<(Option<String>, Vec<FileItem>), DownloadError> {
        if !is_sharepoint(&job.id) {
            return Err(DownloadError::Fatal("not implemented".into()));
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
            return Err(DownloadError::Fatal("not implemented".into()));
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
    async fn non_sharepoint_hosts_are_not_implemented_yet() {
        let ctx = Ctx { app: None, http: client() };
        let job = Job { kind: JobKind::File, id: "https://1drv.ms/u/s!abc".into(), password: String::new() };
        let err = OneDrive::new().list_files(&ctx, &job).await.unwrap_err();
        assert_eq!(err, DownloadError::Fatal("not implemented".into()));
    }
}
