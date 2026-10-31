//! Direct links: the url itself is the file.

use std::path::{Path, PathBuf};

use crate::error::DownloadError;
use crate::link::Job;
use crate::site::{Ctx, FileItem, FileProgress, Site};

use super::{name_from_url, probe, save_named};

pub struct Direct;

#[async_trait::async_trait]
impl Site for Direct {
    async fn list_files(&mut self, _ctx: &Ctx, job: &Job) -> Result<(Option<String>, Vec<FileItem>), DownloadError> {
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
        // Read only the headers first: make sure it is a file and learn its name and size.
        let head = probe(&ctx.http, &item.id).await?;
        let name = match head.name {
            Some(name) => name,
            None if head.is_html => {
                return Err(DownloadError::NotAFile(format!("{} is a web page, not a file", item.id)));
            }
            None => name_from_url(&head.final_url).unwrap_or_else(|| "download".to_string()),
        };
        save_named(&ctx.http, &head.final_url, &name, head.size, out_dir, progress, false).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{Job, JobKind};
    use crate::site::{Ctx, Site};
    use crate::test_server::{serve, Route};
    use crate::{client, DownloadError};

    fn ctx() -> Ctx {
        Ctx { app: None, http: client() }
    }

    fn job(url: String) -> Job {
        Job { kind: JobKind::File, id: url, password: String::new() }
    }

    fn payload() -> Vec<u8> {
        (0..5_000u32).map(|i| (i % 251) as u8).collect()
    }

    async fn run(url: String, dir: &std::path::Path) -> Result<std::path::PathBuf, DownloadError> {
        let ctx = ctx();
        let mut site = Direct;
        let (_, files) = site.list_files(&ctx, &job(url)).await?;
        assert_eq!(files.len(), 1);
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        site.download(&ctx, &files[0], "", dir, &noop).await
    }

    #[tokio::test]
    async fn direct_uses_content_disposition_name() {
        let mut route = Route::ok("/dl", payload());
        route.headers.push(("Content-Disposition".into(), "attachment; filename=\"a.zip\"".into()));
        let base = serve(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let path = run(format!("{base}/dl"), dir.path()).await.unwrap();
        assert_eq!(path, dir.path().join("a.zip"));
        assert_eq!(std::fs::read(&path).unwrap(), payload());
    }

    #[tokio::test]
    async fn direct_html_page_is_not_a_file() {
        let mut route = Route::ok("/page", b"<html></html>".to_vec());
        route.headers.push(("Content-Type".into(), "text/html; charset=utf-8".into()));
        let base = serve(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let err = run(format!("{base}/page"), dir.path()).await.unwrap_err();
        assert!(matches!(err, DownloadError::NotAFile(_)), "{err:?}");
    }

    #[tokio::test]
    async fn direct_404_is_not_found() {
        let base = serve(vec![]).await;
        let dir = tempfile::tempdir().unwrap();
        let err = run(format!("{base}/missing.zip"), dir.path()).await.unwrap_err();
        assert!(matches!(err, DownloadError::NotFound(_)), "{err:?}");
    }

    #[tokio::test]
    async fn direct_falls_back_to_url_path_name() {
        let base = serve(vec![Route::ok("/files/my%20pack.bin", payload())]).await;
        let dir = tempfile::tempdir().unwrap();
        let path = run(format!("{base}/files/my%20pack.bin"), dir.path()).await.unwrap();
        assert_eq!(path, dir.path().join("my pack.bin"));
    }

    #[tokio::test]
    async fn direct_skips_existing_file_of_same_size() {
        let base = serve(vec![Route::ok("/x.bin", payload())]).await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.bin"), payload()).unwrap();
        let before = std::fs::metadata(dir.path().join("x.bin")).unwrap().modified().unwrap();
        let path = run(format!("{base}/x.bin"), dir.path()).await.unwrap();
        assert_eq!(path, dir.path().join("x.bin"));
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before);
        assert!(!dir.path().join("x.bin.part").exists());
    }
}
