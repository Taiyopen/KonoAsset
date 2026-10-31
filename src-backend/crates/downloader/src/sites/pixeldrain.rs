//! pixeldrain, through its public API (`/u/<id>` files and `/l/<id>` lists).

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::DownloadError;
use crate::http::{safe_name, SaveOptions};
use crate::link::{Job, JobKind};
use crate::site::{Ctx, FileItem, FileProgress, Site};

use super::{check_status, save_named};

pub struct Pixeldrain {
    /// `https://pixeldrain.com/api` in production.
    api: String,
}

impl Pixeldrain {
    pub fn new(api: String) -> Self {
        Self { api: api.trim_end_matches('/').to_string() }
    }

    async fn api_json(&self, ctx: &Ctx, path: &str) -> Result<Value, DownloadError> {
        let url = format!("{}/{path}", self.api);
        let response = ctx.http.get(&url).send().await?;
        check_status(response.status(), &url)?;
        Ok(response.json().await?)
    }
}

fn file_page(id: &str) -> String {
    format!("https://pixeldrain.com/u/{id}")
}

#[async_trait::async_trait]
impl Site for Pixeldrain {
    async fn list_files(&mut self, ctx: &Ctx, job: &Job) -> Result<(Option<String>, Vec<FileItem>), DownloadError> {
        if job.kind == JobKind::File {
            return Ok((None, vec![FileItem { id: job.id.clone(), subdir: String::new(), name: None }]));
        }
        let list = self.api_json(ctx, &format!("list/{}", job.id)).await?;
        let title = list["title"].as_str().filter(|t| !t.is_empty()).unwrap_or(&job.id);
        let files = list["files"]
            .as_array()
            .ok_or_else(|| DownloadError::Fatal("unexpected pixeldrain list response".into()))?
            .iter()
            .filter_map(|f| {
                Some(FileItem {
                    id: f["id"].as_str()?.to_string(),
                    subdir: String::new(),
                    name: f["name"].as_str().map(str::to_string),
                })
            })
            .collect();
        Ok((Some(safe_name(title)), files))
    }

    async fn download(
        &mut self,
        ctx: &Ctx,
        item: &FileItem,
        _password: &str,
        out_dir: &Path,
        progress: FileProgress<'_>,
    ) -> Result<PathBuf, DownloadError> {
        // List entries carry neither the checksum nor the availability, so ask per file.
        let info = self.api_json(ctx, &format!("file/{}/info", item.id)).await?;
        let name = info["name"]
            .as_str()
            .ok_or_else(|| DownloadError::Fatal("pixeldrain info has no name".into()))?;
        if let Some(availability) = info["availability"].as_str().filter(|a| !a.is_empty()) {
            // e.g. a captcha is required: the user has to download it in a browser.
            let reason = info["availability_message"].as_str().filter(|m| !m.is_empty()).unwrap_or(availability);
            return Err(DownloadError::RateLimited(format!("{reason} ({})", file_page(&item.id))));
        }
        let opts = SaveOptions {
            size: info["size"].as_u64(),
            sha256: info["hash_sha256"].as_str().filter(|h| !h.is_empty()),
            ..Default::default()
        };
        let url = format!("{}/file/{}?download", self.api, item.id);
        save_named(&ctx.http, &url, name, out_dir, progress, opts).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{Job, JobKind};
    use crate::site::{Ctx, FileItem, Site};
    use crate::test_server::{serve, Route};
    use crate::{client, DownloadError};
    use sha2::{Digest, Sha256};

    fn ctx() -> Ctx {
        Ctx { app: None, http: client() }
    }

    fn sha_hex(data: &[u8]) -> String {
        Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn json(path: &str, value: serde_json::Value) -> Route {
        Route::ok(path, value.to_string().into_bytes())
    }

    fn data(seed: u8) -> Vec<u8> {
        (0..3_000u32).map(|i| (i as u8).wrapping_add(seed)).collect()
    }

    fn item(id: &str) -> FileItem {
        FileItem { id: id.into(), subdir: String::new(), name: None }
    }

    #[tokio::test]
    async fn pixeldrain_list_and_download_with_checksum() {
        let (a, b) = (data(1), data(2));
        let base = serve(vec![
            json(
                "/list/L1",
                serde_json::json!({
                    "title": "My: Pack",
                    "files": [{"id": "f1", "name": "one.bin"}, {"id": "f2", "name": "two.bin"}],
                }),
            ),
            json(
                "/file/f1/info",
                serde_json::json!({
                    "name": "one.bin", "size": a.len(), "hash_sha256": sha_hex(&a), "availability": "",
                }),
            ),
            json(
                "/file/f2/info",
                serde_json::json!({
                    "name": "two.bin", "size": b.len(), "hash_sha256": sha_hex(&b), "availability": "",
                }),
            ),
            Route::ok("/file/f1", a.clone()),
            Route::ok("/file/f2", b.clone()),
        ])
        .await;
        let ctx = ctx();
        let mut site = Pixeldrain::new(base);
        let job = Job { kind: JobKind::Folder, id: "L1".into(), password: String::new() };
        let (folder, files) = site.list_files(&ctx, &job).await.unwrap();
        assert_eq!(folder.as_deref(), Some("My_ Pack"));
        assert_eq!(files.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["f1", "f2"]);

        let dir = tempfile::tempdir().unwrap();
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let p1 = site.download(&ctx, &files[0], "", dir.path(), &noop).await.unwrap();
        let p2 = site.download(&ctx, &files[1], "", dir.path(), &noop).await.unwrap();
        assert_eq!(std::fs::read(p1).unwrap(), a);
        assert_eq!(std::fs::read(p2).unwrap(), b);
    }

    #[tokio::test]
    async fn pixeldrain_single_file_job() {
        let ctx = ctx();
        let mut site = Pixeldrain::new("http://unused.invalid".into());
        let job = Job { kind: JobKind::File, id: "abc".into(), password: String::new() };
        let (folder, files) = site.list_files(&ctx, &job).await.unwrap();
        assert_eq!(folder, None);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id, "abc");
    }

    #[tokio::test]
    async fn pixeldrain_checksum_mismatch_is_retryable() {
        let a = data(1);
        let base = serve(vec![
            json(
                "/file/f1/info",
                serde_json::json!({
                    "name": "one.bin", "size": a.len(), "hash_sha256": sha_hex(b"other"), "availability": "",
                }),
            ),
            Route::ok("/file/f1", a),
        ])
        .await;
        let mut site = Pixeldrain::new(base);
        let dir = tempfile::tempdir().unwrap();
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let err = site.download(&ctx(), &item("f1"), "", dir.path(), &noop).await.unwrap_err();
        assert!(err.is_retryable(), "{err:?}");
    }

    #[tokio::test]
    async fn pixeldrain_availability_is_rate_limited() {
        let base = serve(vec![json(
            "/file/f1/info",
            serde_json::json!({
                "name": "one.bin", "size": 10, "hash_sha256": "",
                "availability": "file_rate_limited_captcha_required",
                "availability_message": "Download limit reached",
            }),
        )])
        .await;
        let mut site = Pixeldrain::new(base);
        let dir = tempfile::tempdir().unwrap();
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let err = site.download(&ctx(), &item("f1"), "", dir.path(), &noop).await.unwrap_err();
        match err {
            DownloadError::RateLimited(msg) => {
                assert!(msg.contains("Download limit reached"), "{msg}");
                assert!(msg.contains("pixeldrain.com/u/f1"), "{msg}");
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn pixeldrain_missing_file_is_not_found() {
        let base = serve(vec![]).await;
        let mut site = Pixeldrain::new(base);
        let dir = tempfile::tempdir().unwrap();
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let err = site.download(&ctx(), &item("nope"), "", dir.path(), &noop).await.unwrap_err();
        assert!(matches!(err, DownloadError::NotFound(_)), "{err:?}");
    }
}
