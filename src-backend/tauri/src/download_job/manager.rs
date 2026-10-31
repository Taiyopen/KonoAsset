use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use downloader::{DownloadErrorKind, LineError, LinkLine, Progress, valid_lines};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, Semaphore};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::definitions::{DownloadImportRequest, DownloadJob, DownloadJobStatus, DownloadPreAsset};

const MAX_CONCURRENT_DOWNLOADS: usize = 3;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// Pause between two exclusive (workupload) jobs, like workupload's pause between two files.
const EXCLUSIVE_GAP: Duration = Duration::from_secs(10);

/// When the last exclusive job finished downloading; locked while one downloads.
type ExclusiveSlot = Arc<AsyncMutex<Option<tokio::time::Instant>>>;

/// Does the actual work of a job; the real one wraps `downloader` and the importer.
#[async_trait]
pub trait JobBackend: Send + Sync + 'static {
    async fn download(
        &self,
        lines: Vec<LinkLine>,
        out_dir: PathBuf,
        progress: Arc<dyn Fn(Progress) + Send + Sync>,
    ) -> Result<Vec<PathBuf>, LineError>;

    async fn import(&self, pre_asset: DownloadPreAsset, paths: Vec<PathBuf>) -> Result<(), String>;
}

/// Receives the full job on every change.
pub type EmitFn = Arc<dyn Fn(DownloadJob) + Send + Sync>;

struct Entry {
    job: Arc<StdMutex<DownloadJob>>,
    lines: Vec<LinkLine>,
    pre_asset: DownloadPreAsset,
    handle: Option<JoinHandle<()>>,
}

pub struct DownloadJobManager {
    root: PathBuf,
    backend: Arc<dyn JobBackend>,
    emit: EmitFn,
    jobs: Vec<Entry>,
    download_slots: Arc<Semaphore>,
    exclusive_slot: ExclusiveSlot,
}

impl DownloadJobManager {
    pub fn new(root: PathBuf, backend: Arc<dyn JobBackend>, emit: EmitFn) -> Self {
        Self {
            root,
            backend,
            emit,
            jobs: Vec::new(),
            download_slots: Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS)),
            exclusive_slot: ExclusiveSlot::default(),
        }
    }

    /// Removes everything left by a previous run and recreates `root` empty.
    pub fn cleanup_root(root: &Path) {
        match std::fs::remove_dir_all(root) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => log::error!(
                "Failed to clean up download directory {}: {e}",
                root.display()
            ),
            _ => {}
        }
        if let Err(e) = std::fs::create_dir_all(root) {
            log::error!(
                "Failed to create download directory {}: {e}",
                root.display()
            );
        }
    }

    pub fn enqueue(&mut self, request: DownloadImportRequest) -> Result<Uuid, String> {
        let lines = valid_lines(&request.links_text)?;
        let job = DownloadJob {
            id: Uuid::new_v4(),
            asset_name: request.pre_asset.name().to_string(),
            status: DownloadJobStatus::Queued,
            site: lines[0].site,
            current_file: None,
            file_index: 0,
            file_count: None,
            downloaded_bytes: 0,
            total_bytes: None,
            error: None,
            error_kind: None,
            failed_line: None,
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        let id = job.id;
        (self.emit)(job.clone());

        self.jobs.push(Entry {
            job: Arc::new(StdMutex::new(job)),
            lines,
            pre_asset: request.pre_asset,
            handle: None,
        });
        self.start(self.jobs.len() - 1);
        Ok(id)
    }

    /// Newest first.
    pub fn list(&self) -> Vec<DownloadJob> {
        let mut jobs: Vec<DownloadJob> = self
            .jobs
            .iter()
            .rev()
            .map(|entry| lock(&entry.job).clone())
            .collect();
        jobs.sort_by_key(|job| std::cmp::Reverse(job.created_at));
        jobs
    }

    /// Only queued or downloading jobs can be cancelled: aborting an import half-way could
    /// leave a partially written asset behind.
    pub async fn cancel(&mut self, id: Uuid) -> Result<(), String> {
        let index = self.position(id)?;
        let entry = &mut self.jobs[index];
        {
            let mut job = lock(&entry.job);
            match job.status {
                DownloadJobStatus::Queued | DownloadJobStatus::Downloading => {}
                DownloadJobStatus::Importing => {
                    return Err(format!(
                        "Download job {id} cannot be cancelled while importing"
                    ));
                }
                _ => return Err(format!("Download job {id} is not running")),
            }
            // Decided under the job lock, which the task also holds when it switches to
            // Importing: either this wins and the task stops before importing, or the job is
            // already Importing and the cancel is refused. The abort never hits an import.
            job.status = DownloadJobStatus::Cancelled;
            (self.emit)(job.clone());
        }
        if let Some(handle) = entry.handle.take() {
            handle.abort();
            // Wait until the task is gone so it cannot write into the removed directory.
            let _ = handle.await;
        }
        remove_job_dir(&self.job_dir(id)).await;
        Ok(())
    }

    /// Re-runs a failed job in the same directory; already downloaded files are skipped.
    /// `password` replaces the password of the failed line, and is ignored for import failures.
    pub fn retry(&mut self, id: Uuid, password: Option<String>) -> Result<(), String> {
        let index = self.position(id)?;
        let entry = &mut self.jobs[index];
        {
            let mut job = lock(&entry.job);
            if job.status != DownloadJobStatus::Failed {
                return Err(format!("Download job {id} has not failed"));
            }
            let failed_line = job.failed_line.and_then(|failed_line| {
                entry
                    .lines
                    .iter_mut()
                    .find(|line| line.line_number == failed_line)
            });
            if let (Some(password), Some(line)) = (password, failed_line) {
                line.job.password = password;
            }
            job.status = DownloadJobStatus::Queued;
            job.current_file = None;
            job.file_index = 0;
            job.file_count = None;
            job.downloaded_bytes = 0;
            job.total_bytes = None;
            job.error = None;
            job.error_kind = None;
            job.failed_line = None;
            (self.emit)(job.clone());
        }
        self.start(index);
        Ok(())
    }

    pub async fn dismiss(&mut self, id: Uuid) -> Result<(), String> {
        let index = self.position(id)?;
        if lock(&self.jobs[index].job).status.is_active() {
            return Err(format!("Download job {id} is still running"));
        }
        self.jobs.remove(index);
        remove_job_dir(&self.job_dir(id)).await;
        Ok(())
    }

    fn position(&self, id: Uuid) -> Result<usize, String> {
        self.jobs
            .iter()
            .position(|entry| lock(&entry.job).id == id)
            .ok_or_else(|| format!("Download job {id} not found"))
    }

    fn job_dir(&self, id: Uuid) -> PathBuf {
        self.root.join(id.to_string())
    }

    fn start(&mut self, index: usize) {
        let entry = &self.jobs[index];
        let id = lock(&entry.job).id;
        let run = Run {
            job: entry.job.clone(),
            emit: self.emit.clone(),
            backend: self.backend.clone(),
            lines: entry.lines.clone(),
            pre_asset: entry.pre_asset.clone(),
            out_dir: self.job_dir(id),
            download_slots: self.download_slots.clone(),
            exclusive_slot: self.exclusive_slot.clone(),
        };
        self.jobs[index].handle = Some(tokio::spawn(run.execute()));
    }
}

/// Everything one execution of a job needs, owned by its task (never the manager lock).
struct Run {
    job: Arc<StdMutex<DownloadJob>>,
    emit: EmitFn,
    backend: Arc<dyn JobBackend>,
    lines: Vec<LinkLine>,
    pre_asset: DownloadPreAsset,
    out_dir: PathBuf,
    download_slots: Arc<Semaphore>,
    exclusive_slot: ExclusiveSlot,
}

impl Run {
    async fn execute(self) {
        let downloaded = {
            // Take the exclusive slot first so a waiting workupload job does not hold a shared slot.
            let _exclusive = if self.lines.iter().any(|line| line.site.is_exclusive()) {
                let last_finished = self.exclusive_slot.clone().lock_owned().await;
                if let Some(at) = *last_finished {
                    tokio::time::sleep_until(at + EXCLUSIVE_GAP).await;
                }
                Some(ExclusiveTurn(last_finished))
            } else {
                None
            };
            let _slot = self
                .download_slots
                .clone()
                .acquire_owned()
                .await
                .expect("semaphore is never closed");

            if !self.update(|job| job.status = DownloadJobStatus::Downloading) {
                return;
            }
            if let Err(e) = tokio::fs::create_dir_all(&self.out_dir).await {
                self.fail(e.to_string(), DownloadErrorKind::Other, None);
                return;
            }
            self.backend
                .download(
                    self.lines.clone(),
                    self.out_dir.clone(),
                    self.progress_callback(),
                )
                .await
        };

        let paths = match downloaded {
            Ok(paths) if paths.is_empty() => {
                self.fail(
                    "nothing to import".to_string(),
                    DownloadErrorKind::NotFound,
                    None,
                );
                return;
            }
            Ok(paths) => paths,
            Err(e) => {
                self.fail(e.error.to_string(), e.error.kind(), Some(e.line_number));
                return;
            }
        };

        // Refused once cancelled; after this succeeds `cancel` is refused (see `cancel`).
        if !self.update(|job| job.status = DownloadJobStatus::Importing) {
            return;
        }
        if let Err(e) = self.backend.import(self.pre_asset.clone(), paths).await {
            self.fail(e, DownloadErrorKind::ImportFailed, None);
            return;
        }

        remove_job_dir(&self.out_dir).await;
        self.update(|job| job.status = DownloadJobStatus::Completed);
    }

    /// Applies `change` and emits, unless the job is no longer active (e.g. cancelled).
    fn update(&self, change: impl FnOnce(&mut DownloadJob)) -> bool {
        let mut job = lock(&self.job);
        if !job.status.is_active() {
            return false;
        }
        change(&mut job);
        (self.emit)(job.clone());
        true
    }

    fn fail(&self, error: String, kind: DownloadErrorKind, failed_line: Option<u32>) {
        self.update(|job| {
            job.status = DownloadJobStatus::Failed;
            job.error = Some(error);
            job.error_kind = Some(kind);
            job.failed_line = failed_line;
        });
    }

    /// Updates the job on every report but emits at most once per `PROGRESS_INTERVAL`.
    fn progress_callback(&self) -> Arc<dyn Fn(Progress) + Send + Sync> {
        let job = self.job.clone();
        let emit = self.emit.clone();
        let last_emit: StdMutex<Option<Instant>> = StdMutex::new(None);

        Arc::new(move |progress: Progress| {
            let mut job = lock(&job);
            if job.status != DownloadJobStatus::Downloading {
                return;
            }
            job.current_file = Some(progress.filename);
            job.file_index = progress.file_index;
            job.file_count = Some(progress.file_count);
            job.downloaded_bytes = progress.downloaded;
            job.total_bytes = progress.total;

            let now = Instant::now();
            let mut last = lock(&last_emit);
            if last.is_none_or(|last| now.duration_since(last) >= PROGRESS_INTERVAL) {
                *last = Some(now);
                emit(job.clone());
            }
        })
    }
}

/// The exclusive slot of a running job; records when it is given back.
struct ExclusiveTurn(OwnedMutexGuard<Option<tokio::time::Instant>>);

impl Drop for ExclusiveTurn {
    fn drop(&mut self) {
        *self.0 = Some(tokio::time::Instant::now());
    }
}

/// A panic elsewhere must not make every later job operation panic too.
fn lock<T>(mutex: &StdMutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

async fn remove_job_dir(dir: &Path) {
    match tokio::fs::remove_dir_all(dir).await {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            log::warn!("Failed to remove download directory {}: {e}", dir.display())
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use downloader::{DownloadError, DownloadErrorKind, SiteKind};
    use model::AssetDescription;
    use tokio::sync::Semaphore;

    use crate::definitions::import_request::PreAvatar;
    use crate::download_job::definitions::{
        DownloadImportRequest, DownloadJobStatus, DownloadPreAsset,
    };

    const PIXELDRAIN: &str = "https://pixeldrain.com/u/abc";
    const TWO_LINES: &str = "https://pixeldrain.com/u/abc\nhttps://workupload.com/file/XYZ old";

    #[derive(Default)]
    struct FakeBackend {
        /// Popped per call; empty means "download one file successfully".
        download_results: StdMutex<VecDeque<Result<Vec<PathBuf>, LineError>>>,
        /// Popped per call; empty means `Ok(())`.
        import_results: StdMutex<VecDeque<Result<(), String>>>,
        /// When set, every download waits for one permit.
        gate: Option<Arc<Semaphore>>,
        /// When set, every import waits for one permit.
        import_gate: Option<Arc<Semaphore>>,
        /// `(sleep before in ms, progress)` reported by every download.
        progress_steps: Vec<(u64, Progress)>,
        download_calls: AtomicU32,
        import_calls: AtomicU32,
        /// Passwords of every line, per download call.
        passwords: StdMutex<Vec<Vec<String>>>,
        /// When every download call started.
        started_at: StdMutex<Vec<tokio::time::Instant>>,
    }

    #[async_trait]
    impl JobBackend for FakeBackend {
        async fn download(
            &self,
            lines: Vec<LinkLine>,
            out_dir: PathBuf,
            progress: Arc<dyn Fn(Progress) + Send + Sync>,
        ) -> Result<Vec<PathBuf>, LineError> {
            self.passwords
                .lock()
                .unwrap()
                .push(lines.iter().map(|l| l.job.password.clone()).collect());
            self.download_calls.fetch_add(1, Ordering::SeqCst);
            self.started_at
                .lock()
                .unwrap()
                .push(tokio::time::Instant::now());

            if let Some(gate) = &self.gate {
                gate.acquire().await.unwrap().forget();
            }
            for (delay, step) in &self.progress_steps {
                if *delay > 0 {
                    tokio::time::sleep(Duration::from_millis(*delay)).await;
                }
                progress(step.clone());
            }

            let scripted = self.download_results.lock().unwrap().pop_front();
            if let Some(result) = scripted {
                return result;
            }
            let file = out_dir.join("1").join("a.zip");
            tokio::fs::create_dir_all(file.parent().unwrap())
                .await
                .unwrap();
            tokio::fs::write(&file, b"zip").await.unwrap();
            Ok(vec![file])
        }

        async fn import(
            &self,
            _pre_asset: DownloadPreAsset,
            _paths: Vec<PathBuf>,
        ) -> Result<(), String> {
            self.import_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.import_gate {
                gate.acquire().await.unwrap().forget();
            }
            self.import_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
        }
    }

    struct Harness {
        manager: DownloadJobManager,
        backend: Arc<FakeBackend>,
        events: Arc<StdMutex<Vec<DownloadJob>>>,
        root: PathBuf,
    }

    fn harness(name: &str, backend: FakeBackend) -> Harness {
        let root = PathBuf::from("test/temp/download_job").join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let events: Arc<StdMutex<Vec<DownloadJob>>> = Arc::default();
        let sink = events.clone();
        let emit: EmitFn = Arc::new(move |job| sink.lock().unwrap().push(job));
        let backend = Arc::new(backend);
        let manager = DownloadJobManager::new(root.clone(), backend.clone(), emit);

        Harness {
            manager,
            backend,
            events,
            root,
        }
    }

    fn request(links: &str) -> DownloadImportRequest {
        DownloadImportRequest {
            links_text: links.to_string(),
            pre_asset: DownloadPreAsset::Avatar(PreAvatar {
                description: AssetDescription {
                    name: "Test Avatar".to_string(),
                    creator: "Creator".to_string(),
                    image_filename: None,
                    tags: vec![],
                    memo: None,
                    booth_item_id: None,
                    dependencies: vec![],
                    created_at: 0,
                    published_at: None,
                },
            }),
        }
    }

    fn job(manager: &DownloadJobManager, id: Uuid) -> DownloadJob {
        manager
            .list()
            .into_iter()
            .find(|job| job.id == id)
            .expect("job not listed")
    }

    /// Polls for 20 s (virtual time in paused tests, where exclusive jobs wait `EXCLUSIVE_GAP`).
    async fn wait_until(mut condition: impl FnMut() -> bool) {
        for _ in 0..2000 {
            if condition() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("condition not met within 20s");
    }

    async fn wait_for_status(manager: &DownloadJobManager, id: Uuid, status: DownloadJobStatus) {
        wait_until(|| job(manager, id).status == status).await;
    }

    fn line_error(line_number: u32, error: DownloadError) -> LineError {
        LineError { line_number, error }
    }

    fn progress(file_index: u32, downloaded: u64) -> Progress {
        Progress {
            filename: format!("file{file_index}.zip"),
            file_index,
            file_count: 3,
            downloaded,
            total: Some(1000),
        }
    }

    fn count_status(
        manager: &DownloadJobManager,
        ids: &[Uuid],
        status: DownloadJobStatus,
    ) -> usize {
        ids.iter()
            .filter(|id| job(manager, **id).status == status)
            .count()
    }

    #[tokio::test]
    async fn cancel_during_import_is_rejected() {
        let import_gate = Arc::new(Semaphore::new(0));
        let backend = FakeBackend {
            import_gate: Some(import_gate.clone()),
            ..Default::default()
        };
        let mut h = harness("cancel_import", backend);

        let id = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        wait_until(|| h.backend.import_calls.load(Ordering::SeqCst) == 1).await;
        assert_eq!(job(&h.manager, id).status, DownloadJobStatus::Importing);

        assert!(h.manager.cancel(id).await.is_err());
        assert_eq!(job(&h.manager, id).status, DownloadJobStatus::Importing);

        import_gate.add_permits(1);
        wait_for_status(&h.manager, id, DownloadJobStatus::Completed).await;
        assert_eq!(h.backend.import_calls.load(Ordering::SeqCst), 1);
        assert!(
            !h.events
                .lock()
                .unwrap()
                .iter()
                .any(|job| job.status == DownloadJobStatus::Cancelled)
        );
        assert!(!h.root.join(id.to_string()).exists());
    }

    #[tokio::test]
    async fn at_most_three_jobs_download_at_once() {
        let gate = Arc::new(Semaphore::new(0));
        let backend = FakeBackend {
            gate: Some(gate.clone()),
            ..Default::default()
        };
        let mut h = harness("three_at_once", backend);

        let ids: Vec<Uuid> = (0..4)
            .map(|_| h.manager.enqueue(request(PIXELDRAIN)).unwrap())
            .collect();
        wait_until(|| h.backend.download_calls.load(Ordering::SeqCst) == 3).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(h.backend.download_calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            count_status(&h.manager, &ids, DownloadJobStatus::Downloading),
            3
        );
        assert_eq!(count_status(&h.manager, &ids, DownloadJobStatus::Queued), 1);

        gate.add_permits(1);
        wait_until(|| h.backend.download_calls.load(Ordering::SeqCst) == 4).await;
        assert_eq!(count_status(&h.manager, &ids, DownloadJobStatus::Queued), 0);

        gate.add_permits(3);
        wait_until(|| count_status(&h.manager, &ids, DownloadJobStatus::Completed) == 4).await;
    }

    #[tokio::test]
    async fn cancel_queued_job_frees_queue() {
        let gate = Arc::new(Semaphore::new(0));
        let backend = FakeBackend {
            gate: Some(gate.clone()),
            ..Default::default()
        };
        let mut h = harness("cancel_queued", backend);

        let running: Vec<Uuid> = (0..3)
            .map(|_| h.manager.enqueue(request(PIXELDRAIN)).unwrap())
            .collect();
        let queued = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        let last = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        wait_until(|| h.backend.download_calls.load(Ordering::SeqCst) == 3).await;
        assert_eq!(job(&h.manager, queued).status, DownloadJobStatus::Queued);

        // Like a retried job, a queued job may already have a folder from an earlier run.
        let dir = h.root.join(queued.to_string());
        std::fs::create_dir_all(dir.join("1")).unwrap();
        h.manager.cancel(queued).await.unwrap();
        assert_eq!(job(&h.manager, queued).status, DownloadJobStatus::Cancelled);
        assert!(!dir.exists());

        // The freed queue position goes to the next job.
        gate.add_permits(1);
        wait_for_status(&h.manager, last, DownloadJobStatus::Downloading).await;
        gate.add_permits(3);
        for id in running.iter().chain([&last]) {
            wait_for_status(&h.manager, *id, DownloadJobStatus::Completed).await;
        }
        assert_eq!(h.backend.download_calls.load(Ordering::SeqCst), 4);
        assert_eq!(job(&h.manager, queued).status, DownloadJobStatus::Cancelled);
    }

    #[tokio::test]
    async fn job_goes_through_all_stages() {
        let mut h = harness("all_stages", FakeBackend::default());

        let id = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        wait_for_status(&h.manager, id, DownloadJobStatus::Completed).await;

        let mut statuses: Vec<DownloadJobStatus> = h
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|job| job.status)
            .collect();
        statuses.dedup();
        assert_eq!(
            statuses,
            vec![
                DownloadJobStatus::Queued,
                DownloadJobStatus::Downloading,
                DownloadJobStatus::Importing,
                DownloadJobStatus::Completed,
            ]
        );
        let job = job(&h.manager, id);
        assert_eq!(job.asset_name, "Test Avatar");
        assert_eq!(job.site, SiteKind::Pixeldrain);
        assert_eq!(h.backend.import_calls.load(Ordering::SeqCst), 1);
        assert!(!h.root.join(id.to_string()).exists());
    }

    #[tokio::test]
    async fn invalid_links_are_rejected() {
        let mut h = harness("invalid_links", FakeBackend::default());

        assert!(h.manager.enqueue(request("not a link")).is_err());
        assert!(h.manager.list().is_empty());
        assert!(h.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn download_failure_keeps_dir_and_reports_kind() {
        let backend = FakeBackend::default();
        backend
            .download_results
            .lock()
            .unwrap()
            .push_back(Err(line_error(2, DownloadError::WrongPassword)));
        let mut h = harness("download_failure", backend);

        let id = h.manager.enqueue(request(TWO_LINES)).unwrap();
        wait_for_status(&h.manager, id, DownloadJobStatus::Failed).await;

        let job = job(&h.manager, id);
        assert_eq!(job.error_kind, Some(DownloadErrorKind::WrongPassword));
        assert_eq!(job.failed_line, Some(2));
        assert_eq!(job.error.as_deref(), Some("wrong password"));
        assert_eq!(h.backend.import_calls.load(Ordering::SeqCst), 0);
        assert!(h.root.join(id.to_string()).exists());
    }

    #[tokio::test(start_paused = true)]
    async fn retry_with_password_replaces_failed_line_password() {
        let backend = FakeBackend::default();
        backend
            .download_results
            .lock()
            .unwrap()
            .push_back(Err(line_error(2, DownloadError::WrongPassword)));
        let mut h = harness("retry_password", backend);

        let id = h.manager.enqueue(request(TWO_LINES)).unwrap();
        wait_for_status(&h.manager, id, DownloadJobStatus::Failed).await;

        assert!(h.manager.retry(id, Some("new".to_string())).is_ok());
        wait_for_status(&h.manager, id, DownloadJobStatus::Completed).await;

        let passwords = h.backend.passwords.lock().unwrap().clone();
        assert_eq!(passwords.len(), 2);
        assert_eq!(passwords[0], vec!["".to_string(), "old".to_string()]);
        assert_eq!(passwords[1], vec!["".to_string(), "new".to_string()]);
        let job = job(&h.manager, id);
        assert_eq!(job.error, None);
        assert_eq!(job.error_kind, None);
        assert_eq!(job.failed_line, None);
        assert!(
            h.manager.retry(id, None).is_err(),
            "only failed jobs can be retried"
        );
    }

    #[tokio::test]
    async fn cancel_removes_dir() {
        let backend = FakeBackend {
            gate: Some(Arc::new(Semaphore::new(0))),
            ..Default::default()
        };
        let mut h = harness("cancel", backend);

        let id = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        wait_until(|| h.backend.download_calls.load(Ordering::SeqCst) == 1).await;
        assert_eq!(job(&h.manager, id).status, DownloadJobStatus::Downloading);
        assert!(h.root.join(id.to_string()).exists());

        h.manager.cancel(id).await.unwrap();

        assert_eq!(job(&h.manager, id).status, DownloadJobStatus::Cancelled);
        assert_eq!(
            h.events.lock().unwrap().last().unwrap().status,
            DownloadJobStatus::Cancelled
        );
        assert!(!h.root.join(id.to_string()).exists());
        assert!(h.manager.cancel(id).await.is_err(), "already cancelled");
    }

    #[tokio::test(start_paused = true)]
    async fn exclusive_jobs_run_one_at_a_time() {
        let gate = Arc::new(Semaphore::new(0));
        let backend = FakeBackend {
            gate: Some(gate.clone()),
            ..Default::default()
        };
        let mut h = harness("exclusive", backend);

        let first = h
            .manager
            .enqueue(request("https://workupload.com/file/AAAA"))
            .unwrap();
        let second = h
            .manager
            .enqueue(request("https://workupload.com/file/BBBB"))
            .unwrap();
        wait_until(|| h.backend.download_calls.load(Ordering::SeqCst) == 1).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(h.backend.download_calls.load(Ordering::SeqCst), 1);

        let mut statuses = vec![
            job(&h.manager, first).status,
            job(&h.manager, second).status,
        ];
        statuses.sort_by_key(|status| *status as u8);
        assert_eq!(
            statuses,
            vec![DownloadJobStatus::Queued, DownloadJobStatus::Downloading]
        );

        // A waiting workupload job must not block other sites.
        let other = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        wait_for_status(&h.manager, other, DownloadJobStatus::Downloading).await;

        gate.add_permits(3);
        wait_for_status(&h.manager, first, DownloadJobStatus::Completed).await;
        wait_for_status(&h.manager, second, DownloadJobStatus::Completed).await;
        wait_for_status(&h.manager, other, DownloadJobStatus::Completed).await;
    }

    #[tokio::test(start_paused = true)]
    async fn exclusive_jobs_wait_ten_seconds_after_the_previous_one() {
        let mut h = harness("exclusive_gap", FakeBackend::default());

        let first = h
            .manager
            .enqueue(request("https://workupload.com/file/AAAA"))
            .unwrap();
        wait_for_status(&h.manager, first, DownloadJobStatus::Completed).await;
        let second = h
            .manager
            .enqueue(request("https://workupload.com/file/BBBB"))
            .unwrap();
        let other = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        wait_for_status(&h.manager, other, DownloadJobStatus::Completed).await;
        wait_for_status(&h.manager, second, DownloadJobStatus::Completed).await;

        let started = h.backend.started_at.lock().unwrap().clone();
        assert_eq!(started.len(), 3);
        // The pixeldrain job does not wait; the second workupload job waits for the gap.
        assert!(started[1] - started[0] < Duration::from_secs(1));
        let gap = started[2] - started[0];
        assert!(
            (EXCLUSIVE_GAP..EXCLUSIVE_GAP + Duration::from_secs(1)).contains(&gap),
            "{gap:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn import_failure_is_import_failed() {
        let backend = FakeBackend::default();
        backend
            .import_results
            .lock()
            .unwrap()
            .push_back(Err("boom".to_string()));
        let mut h = harness("import_failure", backend);

        let id = h.manager.enqueue(request(TWO_LINES)).unwrap();
        wait_for_status(&h.manager, id, DownloadJobStatus::Failed).await;

        let failed = job(&h.manager, id);
        assert_eq!(failed.error_kind, Some(DownloadErrorKind::ImportFailed));
        assert_eq!(failed.error.as_deref(), Some("boom"));
        assert_eq!(failed.failed_line, None);
        assert!(h.root.join(id.to_string()).exists());

        // No failed line: the password is ignored and the job simply re-runs.
        h.manager.retry(id, Some("ignored".to_string())).unwrap();
        wait_for_status(&h.manager, id, DownloadJobStatus::Completed).await;
        let passwords = h.backend.passwords.lock().unwrap().clone();
        assert_eq!(passwords[1], vec!["".to_string(), "old".to_string()]);
        assert_eq!(h.backend.import_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn empty_download_result_fails() {
        let backend = FakeBackend::default();
        backend
            .download_results
            .lock()
            .unwrap()
            .push_back(Ok(vec![]));
        let mut h = harness("empty_result", backend);

        let id = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        wait_for_status(&h.manager, id, DownloadJobStatus::Failed).await;

        let job = job(&h.manager, id);
        assert_eq!(job.error_kind, Some(DownloadErrorKind::NotFound));
        assert_eq!(job.error.as_deref(), Some("nothing to import"));
        assert_eq!(job.failed_line, None);
        assert_eq!(h.backend.import_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn progress_events_are_throttled() {
        let mut steps: Vec<(u64, Progress)> = (1..=10).map(|n| (0, progress(1, n * 10))).collect();
        steps.push((300, progress(2, 500)));
        let backend = FakeBackend {
            progress_steps: steps,
            ..Default::default()
        };
        let mut h = harness("throttle", backend);

        let id = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        wait_for_status(&h.manager, id, DownloadJobStatus::Completed).await;

        let events = h.events.lock().unwrap().clone();
        let progress_events: Vec<&DownloadJob> = events
            .iter()
            .filter(|job| job.status == DownloadJobStatus::Downloading && job.downloaded_bytes > 0)
            .collect();
        assert_eq!(
            progress_events.len(),
            2,
            "first burst is throttled to one event"
        );
        assert_eq!(progress_events[0].downloaded_bytes, 10);
        let last = progress_events[1];
        assert_eq!(last.current_file.as_deref(), Some("file2.zip"));
        assert_eq!(last.file_index, 2);
        assert_eq!(last.file_count, Some(3));
        assert_eq!(last.downloaded_bytes, 500);
        assert_eq!(last.total_bytes, Some(1000));
    }

    #[tokio::test]
    async fn dismiss_only_removes_finished_jobs() {
        let gate = Arc::new(Semaphore::new(0));
        let backend = FakeBackend {
            gate: Some(gate.clone()),
            ..Default::default()
        };
        backend
            .download_results
            .lock()
            .unwrap()
            .push_back(Err(line_error(1, DownloadError::NotFound("gone".into()))));
        let mut h = harness("dismiss", backend);

        let id = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        wait_until(|| h.backend.download_calls.load(Ordering::SeqCst) == 1).await;
        assert!(
            h.manager.dismiss(id).await.is_err(),
            "active jobs cannot be dismissed"
        );

        gate.add_permits(1);
        wait_for_status(&h.manager, id, DownloadJobStatus::Failed).await;
        assert!(h.root.join(id.to_string()).exists());

        h.manager.dismiss(id).await.unwrap();
        assert!(h.manager.list().is_empty());
        assert!(!h.root.join(id.to_string()).exists());
    }

    #[tokio::test]
    async fn list_is_newest_first() {
        let backend = FakeBackend {
            gate: Some(Arc::new(Semaphore::new(0))),
            ..Default::default()
        };
        let mut h = harness("list_order", backend);

        let first = h.manager.enqueue(request(PIXELDRAIN)).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        let second = h.manager.enqueue(request(PIXELDRAIN)).unwrap();

        let ids: Vec<Uuid> = h.manager.list().iter().map(|job| job.id).collect();
        assert_eq!(ids, vec![second, first]);
    }

    #[test]
    fn cleanup_root_removes_leftovers() {
        let root = PathBuf::from("test/temp/download_job/cleanup");
        let _ = std::fs::remove_dir_all(&root);
        let leftover = root.join("old-job").join("1");
        std::fs::create_dir_all(&leftover).unwrap();
        std::fs::write(leftover.join("a.zip"), b"zip").unwrap();

        DownloadJobManager::cleanup_root(&root);

        assert!(root.is_dir());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }
}
