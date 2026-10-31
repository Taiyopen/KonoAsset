//! The interface every file hosting site implements.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::DownloadError;
use crate::link::{Job, SiteKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileItem {
    pub id: String,
    /// Sub-folder inside the job's folder; empty for files at the top level.
    pub subdir: String,
    pub name: Option<String>,
}

pub struct Ctx {
    /// `None` in tests and for sites that do not need a webview.
    pub app: Option<tauri::AppHandle>,
    pub http: reqwest::Client,
}

/// `(file name, downloaded bytes, total bytes)`
pub type FileProgress<'a> = &'a (dyn Fn(&str, u64, Option<u64>) + Send + Sync);

#[async_trait::async_trait]
pub trait Site: Send {
    /// Returns `(folder name, files)`. A single file job returns
    /// `(None, vec![FileItem { id: job.id, .. }])`.
    async fn list_files(
        &mut self,
        ctx: &Ctx,
        job: &Job,
    ) -> Result<(Option<String>, Vec<FileItem>), DownloadError>;

    /// Downloads into `out_dir` and returns the final file path.
    /// Returns the path right away when the file already exists with the right size.
    async fn download(
        &mut self,
        ctx: &Ctx,
        item: &FileItem,
        password: &str,
        out_dir: &Path,
        progress: FileProgress<'_>,
    ) -> Result<PathBuf, DownloadError>;

    /// Pause between two consecutive files of this site.
    fn delay(&self) -> Duration {
        Duration::ZERO
    }

    /// Wait before retry number `attempt + 1` (`attempt` starts at 1).
    fn retry_wait(&self, attempt: u32, _err: &DownloadError) -> Duration {
        Duration::from_secs(20 * attempt as u64)
    }
}

pub type SiteFactory = dyn Fn(SiteKind) -> Box<dyn Site> + Send + Sync;
