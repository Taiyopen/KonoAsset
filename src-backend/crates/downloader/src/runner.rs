//! Runs a download job: list every line, then download every file, retrying on transient errors.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::time::{Instant, sleep};

use crate::error::DownloadError;
use crate::http::safe_name;
use crate::link::{LinkLine, SiteKind};
use crate::site::{Ctx, FileItem, Site, SiteFactory};

#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub filename: String,
    /// 1-based index over all files of the whole job.
    pub file_index: u32,
    pub file_count: u32,
    pub downloaded: u64,
    pub total: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineError {
    pub line_number: u32,
    pub error: DownloadError,
}

pub const MAX_TRIES: u32 = 4;

/// Runs `$op` (an expression on `$s`, the site) up to `MAX_TRIES` times. Only retryable
/// errors are retried.
///
/// A macro because a function taking an async closure makes the future of `download_all`
/// not `Send` ("implementation of `Send` is not general enough"), and the job manager spawns it.
macro_rules! with_retry {
    ($site:expr, |$s:ident| $op:expr) => {{
        let $s: &mut dyn Site = $site;
        let mut attempt = 1;
        loop {
            let result: Result<_, DownloadError> = $op.await;
            match result {
                Ok(value) => break Ok(value),
                Err(err) if err.is_retryable() && attempt < MAX_TRIES => {
                    let wait = $s.retry_wait(attempt, &err);
                    log::warn!("attempt {attempt}/{MAX_TRIES} failed, retrying in {wait:?}: {err}");
                    sleep(wait).await;
                    attempt += 1;
                }
                Err(err) => break Err(err),
            }
        }
    }};
}

struct Listed {
    line_number: u32,
    site: SiteKind,
    password: String,
    /// Already sanitized.
    folder: Option<String>,
    items: Vec<FileItem>,
}

/// Components of `subdir` are sanitized one by one so a site can never escape the job folder.
fn subdir_path(subdir: &str) -> PathBuf {
    subdir
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .map(safe_name)
        .collect()
}

/// Lists and downloads every line into `out_dir/<line_number>/[<folder>/][<subdir>/]`.
///
/// Returns the paths to import (deduplicated, in order): the folder for folder jobs,
/// the file for single file jobs. Stops at the first line that fails for good.
pub async fn download_all(
    ctx: &Ctx,
    factory: &SiteFactory,
    lines: &[LinkLine],
    out_dir: &Path,
    progress: &(dyn Fn(Progress) + Send + Sync),
) -> Result<Vec<PathBuf>, LineError> {
    // One `Site` per kind, shared by the whole job (e.g. one hidden browser).
    let mut sites: HashMap<SiteKind, Box<dyn Site>> = HashMap::new();

    let mut listed = Vec::with_capacity(lines.len());
    for line in lines {
        let site = sites.entry(line.site).or_insert_with(|| factory(line.site));
        let (folder, items) =
            with_retry!(site.as_mut(), |s| s.list_files(ctx, &line.job))
                .map_err(|error| LineError {
                    line_number: line.line_number,
                    error,
                })?;
        listed.push(Listed {
            line_number: line.line_number,
            site: line.site,
            password: line.job.password.clone(),
            folder: folder.as_deref().map(safe_name),
            items,
        });
    }

    let file_count = listed.iter().map(|l| l.items.len()).sum::<usize>() as u32;
    let mut file_index = 0u32;
    let mut results: Vec<PathBuf> = Vec::new();
    // When the last file of each site finished; the next file waits until `delay()` has passed.
    let mut last_done: HashMap<SiteKind, Instant> = HashMap::new();

    for entry in &listed {
        let line_dir = out_dir.join(entry.line_number.to_string());
        let base_dir = match &entry.folder {
            Some(folder) => line_dir.join(folder),
            None => line_dir,
        };
        let site = sites
            .get_mut(&entry.site)
            .expect("site was created while listing");
        let mut downloaded_any = false;

        for item in &entry.items {
            file_index += 1;
            let line_error = |error| LineError {
                line_number: entry.line_number,
                error,
            };

            if let Some(done_at) = last_done.get(&entry.site) {
                sleep((*done_at + site.delay()).saturating_duration_since(Instant::now())).await;
            }

            let target_dir = base_dir.join(subdir_path(&item.subdir));
            tokio::fs::create_dir_all(&target_dir)
                .await
                .map_err(|e| line_error(e.into()))?;

            let label = item.name.clone().unwrap_or_else(|| item.id.clone());
            let report = |downloaded: u64, total: Option<u64>| {
                progress(Progress {
                    filename: label.clone(),
                    file_index,
                    file_count,
                    downloaded,
                    total,
                })
            };
            report(0, None);
            let file_progress = |name: &str, downloaded: u64, total: Option<u64>| {
                progress(Progress {
                    filename: name.to_string(),
                    file_index,
                    file_count,
                    downloaded,
                    total,
                })
            };

            let path = with_retry!(site.as_mut(), |s| {
                s.download(ctx, item, &entry.password, &target_dir, &file_progress)
            })
            .map_err(line_error)?;

            last_done.insert(entry.site, Instant::now());
            downloaded_any = true;
            if entry.folder.is_none() && !results.contains(&path) {
                results.push(path);
            }
        }

        if let Some(folder) = &entry.folder {
            let path = out_dir.join(entry.line_number.to_string()).join(folder);
            if downloaded_any && !results.contains(&path) {
                results.push(path);
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::link::{Job, JobKind};
    use crate::site::FileProgress;

    type Listing = (Option<String>, Vec<FileItem>);

    #[derive(Default)]
    struct Shared {
        /// Keyed by job id.
        listings: HashMap<String, Listing>,
        /// Popped by each `download` call; an empty queue means success.
        download_errors: VecDeque<DownloadError>,
        download_calls: u32,
        list_calls: u32,
    }

    struct FakeSite {
        shared: Arc<Mutex<Shared>>,
        delay: Duration,
    }

    #[async_trait::async_trait]
    impl Site for FakeSite {
        async fn list_files(&mut self, _ctx: &Ctx, job: &Job) -> Result<Listing, DownloadError> {
            let mut shared = self.shared.lock().unwrap();
            shared.list_calls += 1;
            shared
                .listings
                .get(&job.id)
                .cloned()
                .ok_or_else(|| DownloadError::NotFound(job.id.clone()))
        }

        async fn download(
            &mut self,
            _ctx: &Ctx,
            item: &FileItem,
            _password: &str,
            out_dir: &Path,
            progress: FileProgress<'_>,
        ) -> Result<PathBuf, DownloadError> {
            {
                let mut shared = self.shared.lock().unwrap();
                shared.download_calls += 1;
                if let Some(err) = shared.download_errors.pop_front() {
                    return Err(err);
                }
            }
            let name = item.name.clone().unwrap_or_else(|| item.id.clone());
            let path = out_dir.join(&name);
            std::fs::write(&path, b"data")?;
            progress(&name, 4, Some(4));
            Ok(path)
        }

        fn delay(&self) -> Duration {
            self.delay
        }
    }

    fn file(id: &str, subdir: &str) -> FileItem {
        FileItem {
            id: id.to_string(),
            subdir: subdir.to_string(),
            name: Some(id.to_string()),
        }
    }

    fn line(line_number: u32, kind: JobKind, id: &str) -> LinkLine {
        LinkLine {
            line_number,
            site: SiteKind::Direct,
            job: Job {
                kind,
                id: id.to_string(),
                password: String::new(),
            },
        }
    }

    fn ctx() -> Ctx {
        Ctx {
            app: None,
            http: reqwest::Client::new(),
        }
    }

    fn setup(delay: Duration) -> (Arc<Mutex<Shared>>, impl Fn(SiteKind) -> Box<dyn Site>) {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let handle = shared.clone();
        let factory = move |_kind| -> Box<dyn Site> {
            Box::new(FakeSite {
                shared: handle.clone(),
                delay,
            })
        };
        (shared, factory)
    }

    fn single(shared: &Arc<Mutex<Shared>>, id: &str) {
        shared
            .lock()
            .unwrap()
            .listings
            .insert(id.to_string(), (None, vec![file(id, "")]));
    }

    fn no_progress(_: Progress) {}

    #[tokio::test(start_paused = true)]
    async fn retries_retryable_then_succeeds() {
        let (shared, factory) = setup(Duration::ZERO);
        single(&shared, "a.zip");
        shared.lock().unwrap().download_errors = VecDeque::from([
            DownloadError::Retryable("1".into()),
            DownloadError::Retryable("2".into()),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();

        let paths = download_all(
            &ctx(),
            &factory,
            &[line(1, JobKind::File, "a.zip")],
            dir.path(),
            &no_progress,
        )
        .await
        .unwrap();

        assert_eq!(paths, vec![dir.path().join("1").join("a.zip")]);
        assert_eq!(shared.lock().unwrap().download_calls, 3);
        // default wait is 20s * attempt: 20s + 40s
        assert_eq!(started.elapsed(), Duration::from_secs(60));
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_max_tries() {
        let (shared, factory) = setup(Duration::ZERO);
        single(&shared, "a.zip");
        shared.lock().unwrap().download_errors = (0..10)
            .map(|_| DownloadError::Retryable("down".into()))
            .collect();
        let dir = tempfile::tempdir().unwrap();

        let err = download_all(
            &ctx(),
            &factory,
            &[line(1, JobKind::File, "a.zip")],
            dir.path(),
            &no_progress,
        )
        .await
        .unwrap_err();

        assert_eq!(
            err,
            LineError {
                line_number: 1,
                error: DownloadError::Retryable("down".into())
            }
        );
        assert_eq!(shared.lock().unwrap().download_calls, MAX_TRIES);
    }

    #[tokio::test(start_paused = true)]
    async fn fatal_error_is_not_retried() {
        let (shared, factory) = setup(Duration::ZERO);
        single(&shared, "a.zip");
        shared.lock().unwrap().download_errors = VecDeque::from([DownloadError::WrongPassword]);
        let dir = tempfile::tempdir().unwrap();

        let err = download_all(
            &ctx(),
            &factory,
            &[line(1, JobKind::File, "a.zip")],
            dir.path(),
            &no_progress,
        )
        .await
        .unwrap_err();

        assert_eq!(err.error, DownloadError::WrongPassword);
        assert_eq!(shared.lock().unwrap().download_calls, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn fatal_list_error_reports_its_line_and_downloads_nothing() {
        let (shared, factory) = setup(Duration::ZERO);
        // unknown id -> NotFound (fatal)
        let dir = tempfile::tempdir().unwrap();

        let err = download_all(
            &ctx(),
            &factory,
            &[line(7, JobKind::File, "missing")],
            dir.path(),
            &no_progress,
        )
        .await
        .unwrap_err();

        assert_eq!(err.line_number, 7);
        assert!(matches!(err.error, DownloadError::NotFound(_)));
        let shared = shared.lock().unwrap();
        assert_eq!((shared.list_calls, shared.download_calls), (1, 0));
    }

    #[tokio::test(start_paused = true)]
    async fn folder_job_returns_folder_path_and_counts_files() {
        let (shared, factory) = setup(Duration::ZERO);
        shared.lock().unwrap().listings.insert(
            "pack-id".into(),
            (
                Some("pack".into()),
                vec![file("a.zip", ""), file("b.png", "sub")],
            ),
        );
        single(&shared, "c.zip");
        let dir = tempfile::tempdir().unwrap();
        let events = Mutex::new(Vec::new());

        let paths = download_all(
            &ctx(),
            &factory,
            &[
                line(1, JobKind::Folder, "pack-id"),
                line(3, JobKind::File, "c.zip"),
            ],
            dir.path(),
            &|p| events.lock().unwrap().push(p),
        )
        .await
        .unwrap();

        let out = dir.path();
        assert_eq!(
            paths,
            vec![out.join("1").join("pack"), out.join("3").join("c.zip")]
        );
        assert!(out.join("1/pack/a.zip").is_file());
        assert!(out.join("1/pack/sub/b.png").is_file());

        let events = events.lock().unwrap();
        assert!(events.iter().all(|p| p.file_count == 3));
        let mut indices: Vec<u32> = events.iter().map(|p| p.file_index).collect();
        assert!(indices.windows(2).all(|w| w[0] <= w[1]));
        indices.dedup();
        assert_eq!(indices, vec![1, 2, 3]);
        assert!(
            events
                .iter()
                .any(|p| p.filename == "b.png" && p.downloaded == 4 && p.total == Some(4))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn same_filename_on_two_lines_kept_separately() {
        let (shared, factory) = setup(Duration::ZERO);
        single(&shared, "Textures.zip");
        let dir = tempfile::tempdir().unwrap();

        let paths = download_all(
            &ctx(),
            &factory,
            &[
                line(1, JobKind::File, "Textures.zip"),
                line(2, JobKind::File, "Textures.zip"),
            ],
            dir.path(),
            &no_progress,
        )
        .await
        .unwrap();

        let out = dir.path();
        assert_eq!(
            paths,
            vec![
                out.join("1").join("Textures.zip"),
                out.join("2").join("Textures.zip")
            ]
        );
        assert!(paths.iter().all(|p| p.is_file()));
    }

    #[tokio::test(start_paused = true)]
    async fn delay_is_applied_between_files_but_not_after_the_last() {
        let (shared, factory) = setup(Duration::from_secs(10));
        single(&shared, "a.zip");
        single(&shared, "b.zip");
        single(&shared, "c.zip");
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();

        download_all(
            &ctx(),
            &factory,
            &[
                line(1, JobKind::File, "a.zip"),
                line(2, JobKind::File, "b.zip"),
                line(3, JobKind::File, "c.zip"),
            ],
            dir.path(),
            &no_progress,
        )
        .await
        .unwrap();

        assert_eq!(started.elapsed(), Duration::from_secs(20));
    }

    #[tokio::test(start_paused = true)]
    async fn subdir_cannot_escape_the_job_folder() {
        let (shared, factory) = setup(Duration::ZERO);
        shared.lock().unwrap().listings.insert(
            "x".into(),
            (Some("pack".into()), vec![file("a.zip", "../../evil")]),
        );
        let dir = tempfile::tempdir().unwrap();

        download_all(
            &ctx(),
            &factory,
            &[line(1, JobKind::Folder, "x")],
            dir.path(),
            &no_progress,
        )
        .await
        .unwrap();

        assert!(
            dir.path()
                .join("1/pack/unnamed/unnamed/evil/a.zip")
                .is_file()
        );
    }

    /// The job manager runs `download_all` on a spawned tokio task, so its future must be `Send`.
    #[test]
    fn download_all_future_is_send() {
        fn assert_send<T: Send>(_: &T) {}
        let ctx = ctx();
        let dir = PathBuf::from("unused");
        let future = download_all(&ctx, &crate::sites::create, &[], &dir, &no_progress);
        assert_send(&future);
    }
}
