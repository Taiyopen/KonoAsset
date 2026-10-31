# 從下載連結新增素材（背景下載＋匯入）實作計畫

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在新增素材對話框貼上下載連結，按「新增素材」後對話框關閉，背景下載並自動匯入成一個素材。

**Architecture:** 新的 `downloader` crate 負責連結解析、HTTP 串流下載（續傳／校驗）、各網站實作與隱藏 WebView2 驅動；`tauri/src/download_job/` 的 `DownloadJobManager` 管理背景工作佇列（號誌限制並行、取消、重試），透過可替換的 `JobBackend` 呼叫下載與現有匯入函式，並以 `DownloadJobUpdated` 事件通知前端。前端新增連結輸入分頁、狀態列的背景工作按鈕與 Popover、zustand store。

**Tech Stack:** Rust 2024（Tauri 2.11 WebviewWindow、reqwest 0.13、tokio、serde、specta rc.22、thiserror、async-trait、regex、sha2）、React 19 + zustand + react-hook-form、vitest。

**Spec:** `docs/superpowers/specs/2026-10-31-download-link-import-design.md`

## Global Constraints

- 在 `main` 分支開發；每個 Task 完成並通過檢查後立即 commit（conventional commits），訊息結尾加 `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`；不要 push。
- **與 AI 建議功能並行**：另一個 session 正在 `main` 上實作 `docs/superpowers/plans/2026-10-31-ai-asset-field-suggestion.md`。Task 1–6 只新增 `crates/downloader`（與 workspace `Cargo.toml` 的依賴），可立即進行；Task 7 起會改到 `command/mod.rs`、`lib.rs`、`bindings.ts`、新增素材對話框與語系檔，開始 Task 7 前先 `git log --oneline` 確認 AI 功能的最後一個 Task（整合確認）已 commit，否則等待。每次 commit 前 `git status` 確認只加入本 Task 的檔案（工作目錄有不屬於本計畫的 `CODE_OF_CONDUCT.md` 刪除與 `routeTree.gen.ts` 修改，不要加入）。
- Rust 測試一律在 `src-backend/` 下執行 `cargo test --workspace [過濾字串]`。
- 前端測試：專案根目錄 `pnpm test:run <路徑>`；pre-commit 會跑 `tsc --noEmit` 與 `eslint src/`。
- 共用 User-Agent（reqwest 與 WebView2 相同）：`Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36`。
- 重試：每個檔案／清單最多 4 次；預設等待 `20s × attempt`；workupload 連線錯誤（訊息含 `ERR_CONNECTION` 或 `ERR_TIMED_OUT`）等待 60／180／300 秒。
- 檔案之間暫停：workupload 10 秒、Google Drive 2 秒、其他 0。
- 同時下載的工作最多 3 個；workupload 工作另外同一時間只跑 1 個。
- 進度事件最多每 250ms 送一次；狀態改變立即送。
- 暫存根目錄：`<app_local_data_dir>/downloads/`；每個工作 `downloads/<job-id>/`，每一行連結 `downloads/<job-id>/<行號>/`（行號 1 起算，指原始輸入的行號）。
- 隱藏 WebView2 視窗 label 一律 `dl-` 開頭；**不要**修改 `capabilities/*.json`（外部網頁不得有 IPC 權限）。
- 網頁 → Rust 回報網址：`https://konoasset.invalid/report?d=<encodeURIComponent(JSON)>`。
- 錯誤訊息不在 Rust 端翻譯：以 `DownloadErrorKind` 傳到前端由語系檔顯示，細節字串（英文）作為次要說明。
- 每個新的語言鍵都要加進 `src/locales/` 下全部 5 個檔案（en-US、en-GB、ja-JP、zh-CN、zh-TW），zh-TW 使用台灣用語；`node .github/workflows/scripts/locales-test.js` 必須通過。
- `src/lib/bindings.ts` 由 tauri-specta 在 debug 執行時產生；Task 9 先手動編輯成與產生結果相同的內容，Task 12 以 `pnpm tauri dev` 重新產生並確認沒有差異。
- 不實作：重開程式後續傳、拖放／deep link 帶入連結、「加入到這裡」、其他網站。

## Review Focus

1. **貼上的文字含 `\r\n`、行首尾空白、或網址與密碼之間是全形空白「　」**（中文輸入法常見）→ 仍正確取出網址與密碼。（Task 1：`parse_handles_crlf_and_fullwidth_space`）
2. **伺服器回傳的檔名含 Windows 不允許的字元或 latin-1 亂碼**（`a:b?.zip`、UTF-8 位元組被當 latin-1）→ 存檔名合法且中文／日文正確。（Task 2：`safe_name_*`、`filename_from_headers_*`）
3. **伺服器不支援 Range 或沒有 Content-Length** → 續傳時從頭下載、不因大小未知而失敗。（Task 2：`save_stream_restarts_when_range_ignored`、`save_stream_without_content_length`）
4. **同一工作的兩行連結下載到同名檔案**（例如兩個 `Textures.zip`）→ 兩個都保留，不會被當成「已存在」而跳過。（Task 3：`same_filename_on_two_lines_kept_separately`）
5. **下載途中關閉程式，再次開啟** → 上次的暫存資料夾被清除，不會越積越多。（Task 8：`cleanup_root_removes_leftovers`）

---

### Task 1: `downloader` crate 與連結解析

**Files:**
- Create: `src-backend/crates/downloader/Cargo.toml`
- Create: `src-backend/crates/downloader/src/lib.rs`
- Create: `src-backend/crates/downloader/src/link.rs`
- Modify: `src-backend/Cargo.toml`（workspace members 已用 `crates/*`；在 `[workspace.dependencies]` 加 `downloader = { path = "./crates/downloader", version = "0.0.0" }`、`async-trait = "0.1"`、`regex = "1"`、`sha2 = "0.10"`）

**Interfaces:**
- Produces（`downloader::link`，皆 `pub` 並由 `lib.rs` 重新匯出）：
  ```rust
  #[derive(Serialize, Deserialize, specta::Type, Clone, Copy, Debug, PartialEq, Eq)]
  pub enum SiteKind { Workupload, Pixeldrain, GoogleDrive, OneDrive, Direct }
  impl SiteKind { pub fn is_exclusive(self) -> bool /* 只有 Workupload 為 true */ }

  #[derive(Serialize, Deserialize, specta::Type, Clone, Copy, Debug, PartialEq, Eq)]
  pub enum JobKind { File, Folder }

  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct Job { pub kind: JobKind, pub id: String, pub password: String } // Direct/OneDrive 的 id 是完整網址

  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct LinkLine { pub line_number: u32, pub site: SiteKind, pub job: Job }

  #[derive(Serialize, specta::Type, Clone, Debug, PartialEq, Eq)]
  #[serde(rename_all = "camelCase")]
  pub struct ParsedLine {
      pub line_number: u32, pub text: String,
      pub site: Option<SiteKind>, pub kind: Option<JobKind>,
      pub has_password: bool, pub valid: bool,
  }

  pub fn parse_line(line: &str) -> Option<(SiteKind, Job)>;
  pub fn parse_text(text: &str) -> Vec<ParsedLine>;          // 略過空行與 # 開頭
  pub fn valid_lines(text: &str) -> Result<Vec<LinkLine>, String>; // 有無效行或 0 行時 Err
  ```
- 比對順序與正規表示式照 Python 版 `sites/`：Workupload → Pixeldrain → GoogleDrive → OneDrive → Direct（Direct 接下其餘 `https?://\S+`）。
  - Workupload：`workupload\.com/(file|start|archive)/([A-Za-z0-9]+)`，`archive` → Folder；網址後第一個 token 為密碼。
  - Pixeldrain：`pixeldrain\.(?:com|net)/(u|l)/([A-Za-z0-9]+)`，`l` → Folder。
  - GoogleDrive：資料夾 `drive\.google\.com/drive/(?:u/\d+/)?folders/([\w-]+)`；檔案 `(?:drive|docs)\.google\.com/(?:file/d/|open\?id=|uc\?(?:[^ ]*&)?id=)([\w-]+)|drive\.usercontent\.google\.com/download\?(?:[^ ]*&)?id=([\w-]+)`。
  - OneDrive：`https?://(?:1drv\.ms|onedrive\.live\.com|[\w-]+\.sharepoint\.com)/\S+` → File，id = 整段網址。
  - 所有網站都支援「網址 + 空白 + 密碼」；空白包含全形空白 `\u{3000}`。

- [ ] **Step 1: 建立 crate 骨架**：`Cargo.toml` 仿 `crates/booth/Cargo.toml`（description `"Download links from file hosting sites"`），依賴 `tauri`、`reqwest`、`tokio`（workspace，另加 features `["fs", "io-util", "sync", "time", "rt", "net"]`）、`serde`、`serde_json`、`specta`、`thiserror`、`async-trait`、`regex`、`sha2`、`log`；dev-dependencies：`tokio = { workspace = true, features = ["test-util", "rt-multi-thread", "macros", "net"] }`、`tempfile`（若 workspace 沒有就加 `tempfile = "3"`）。

- [ ] **Step 2: 寫失敗的測試**（`link.rs` tests 模組）

```rust
#[test]
fn parse_each_site() {
    let cases = [
        ("https://workupload.com/file/DQTyYjAQqUJ huna", SiteKind::Workupload, JobKind::File, "DQTyYjAQqUJ", "huna"),
        ("https://workupload.com/archive/q2hv3vcBcy 12345", SiteKind::Workupload, JobKind::Folder, "q2hv3vcBcy", "12345"),
        ("https://pixeldrain.com/u/pHP3XpbB", SiteKind::Pixeldrain, JobKind::File, "pHP3XpbB", ""),
        ("https://pixeldrain.net/l/abc123", SiteKind::Pixeldrain, JobKind::Folder, "abc123", ""),
        ("https://drive.google.com/drive/folders/1AbC-_x", SiteKind::GoogleDrive, JobKind::Folder, "1AbC-_x", ""),
        ("https://drive.google.com/file/d/1XyZ/view?usp=sharing", SiteKind::GoogleDrive, JobKind::File, "1XyZ", ""),
        ("https://drive.usercontent.google.com/download?export=download&id=1Q", SiteKind::GoogleDrive, JobKind::File, "1Q", ""),
        ("https://1drv.ms/u/s!Abc", SiteKind::OneDrive, JobKind::File, "https://1drv.ms/u/s!Abc", ""),
        ("https://example.com/a.zip", SiteKind::Direct, JobKind::File, "https://example.com/a.zip", ""),
    ];
    for (line, site, kind, id, pw) in cases {
        let (s, job) = parse_line(line).unwrap_or_else(|| panic!("{line}"));
        assert_eq!((s, job.kind, job.id.as_str(), job.password.as_str()), (site, kind, id, pw), "{line}");
    }
}

#[test]
fn parse_handles_crlf_and_fullwidth_space() {
    let lines = valid_lines("  https://workupload.com/file/abc\u{3000}密碼  \r\n\r\n# comment\r\nhttps://pixeldrain.com/u/x\r\n").unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!((lines[0].line_number, lines[0].job.password.as_str()), (1, "密碼"));
    assert_eq!(lines[1].line_number, 4);
}

#[test]
fn parse_text_marks_invalid_lines() {
    let parsed = parse_text("not a link\nhttps://workupload.com/file/abc pw");
    assert!(!parsed[0].valid && parsed[0].site.is_none());
    assert!(parsed[1].valid && parsed[1].has_password);
    assert!(valid_lines("not a link\nhttps://workupload.com/file/abc").is_err());
    assert!(valid_lines("\n# only comment\n").is_err());
}
```

- [ ] **Step 3: 執行確認失敗**：`cargo test --workspace link::tests` → 編譯錯誤（函式未定義）。
- [ ] **Step 4: 實作** `link.rs`（`regex` 用 `std::sync::LazyLock`；先把 `\u{3000}` 換成半形空白再 `trim`）。
- [ ] **Step 5: 執行確認通過**：`cargo test --workspace link::tests` → 3 passed。
- [ ] **Step 6: Commit**：`feat(downloader): add crate with download link parsing`

---

### Task 2: HTTP 工具（檔名、串流下載、續傳、校驗）

**Files:**
- Create: `src-backend/crates/downloader/src/error.rs`
- Create: `src-backend/crates/downloader/src/http.rs`
- Create: `src-backend/crates/downloader/src/test_server.rs`（`#[cfg(test)]`，供本 crate 其他測試共用）

**Interfaces:**
- Produces：
  ```rust
  // error.rs
  #[derive(Serialize, Deserialize, specta::Type, Clone, Copy, Debug, PartialEq, Eq)]
  pub enum DownloadErrorKind { PasswordRequired, WrongPassword, NotFound, LoginRequired, RateLimited, NotAFile, ImportFailed, Other }

  #[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
  pub enum DownloadError {
      #[error("password required")] PasswordRequired,
      #[error("wrong password")] WrongPassword,
      #[error("not found: {0}")] NotFound(String),
      #[error("login required: {0}")] LoginRequired(String),
      #[error("rate limited: {0}")] RateLimited(String),
      #[error("not a file: {0}")] NotAFile(String),
      #[error("{0}")] Fatal(String),
      #[error("{0}")] Retryable(String),
  }
  impl DownloadError { pub fn is_retryable(&self) -> bool; pub fn kind(&self) -> DownloadErrorKind; } // Fatal/Retryable → Other
  impl From<reqwest::Error> for DownloadError   // → Retryable
  impl From<std::io::Error> for DownloadError   // → Retryable

  // http.rs
  pub const USER_AGENT: &str = /* Global Constraints 的 UA */;
  pub fn client() -> reqwest::Client;           // UA、connect_timeout 30s、read_timeout 60s、無總逾時
  pub fn safe_name(name: &str) -> String;       // 取代 <>:"/\|?* 與 \x00-\x1f 為 _，去掉頭尾空白與 .，空字串 → "unnamed"
  pub fn filename_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String>;
  pub type ProgressFn<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync); // (已下載, 總大小)
  pub struct SaveOptions<'a> { pub size: Option<u64>, pub sha256: Option<&'a str>, pub headers: reqwest::header::HeaderMap, pub reject_html: bool }
  // reject_html = true 時，回應為 403 或 content-type 含 text/html → Err(NotAFile("rejected"))（不寫檔）
  pub async fn save_stream(client: &reqwest::Client, url: &str, target: &Path, opts: SaveOptions<'_>, progress: ProgressFn<'_>) -> Result<(), DownloadError>;
  ```
- `save_stream` 行為照 Python 版 `base.save_stream`：寫到 `<target>.part`；`.part` 存在時送 `Range: bytes=<offset>-`；416 視為已完整；有 offset 但非 206 → 從頭寫；完成後大小與 `size`（或 `offset + content-length`）不符 → `Retryable`；`sha256` 不符 → 刪除 `.part` 並 `Retryable`；最後 rename 成 `target`。以 `response.chunk().await` 迴圈讀取（不需要 reqwest `stream` feature）。404 → `NotFound`。
- `test_server.rs`：`pub async fn serve(routes: Vec<Route>) -> String /* base url */`，`Route { path, status, headers, body: Vec<u8>, honor_range: bool }`，以 `tokio::net::TcpListener` 實作最小 HTTP/1.1（每個連線一個請求，回應後關閉），`honor_range` 時解析 `Range` 回 206（offset 超過長度回 416）。

- [ ] **Step 1: 寫失敗的測試**（`http.rs` tests）

```rust
#[test]
fn safe_name_replaces_windows_invalid_chars() {
    assert_eq!(safe_name("a:b?.zip"), "a_b_.zip");
    assert_eq!(safe_name(" .hidden. "), "hidden");
    assert_eq!(safe_name("【11 Avatars】 Sweet Devil.zip"), "【11 Avatars】 Sweet Devil.zip");
    assert_eq!(safe_name("..."), "unnamed");
}

#[test]
fn filename_from_headers_variants() {
    let h = |v: &[u8]| { let mut m = HeaderMap::new(); m.insert(CONTENT_DISPOSITION, HeaderValue::from_bytes(v).unwrap()); m };
    assert_eq!(filename_from_headers(&h(b"attachment; filename*=UTF-8''%E7%B4%A0%E6%9D%90.zip")).as_deref(), Some("素材.zip"));
    assert_eq!(filename_from_headers(&h(b"attachment; filename=\"a b.zip\"")).as_deref(), Some("a b.zip"));
    assert_eq!(filename_from_headers(&h("attachment; filename=\"素材.zip\"".as_bytes())).as_deref(), Some("素材.zip")); // UTF-8 位元組直接塞在 header
    assert_eq!(filename_from_headers(&HeaderMap::new()), None);
}

#[tokio::test]
async fn save_stream_resumes_from_part() { /* .part 先寫入 body 前 4 bytes，honor_range=true；完成後內容等於 body，.part 不存在 */ }

#[tokio::test]
async fn save_stream_restarts_when_range_ignored() { /* .part 有 4 bytes 垃圾，honor_range=false 回 200 → 檔案內容等於 body */ }

#[tokio::test]
async fn save_stream_without_content_length() { /* 不送 content-length、opts.size=None → 成功，progress 最後一次 total 為 None */ }

#[tokio::test]
async fn save_stream_size_mismatch_is_retryable() { /* opts.size = body.len()+1 → Err(Retryable)，.part 保留 */ }

#[tokio::test]
async fn save_stream_sha256_mismatch_deletes_part() { /* 錯誤 sha → Err(Retryable)，.part 不存在 */ }

#[tokio::test]
async fn save_stream_reject_html() { /* reject_html=true、回 text/html → Err(NotAFile)，target 與 .part 都不存在 */ }
```

- [ ] **Step 2: 執行確認失敗**：`cargo test --workspace http::tests` → 編譯錯誤。
- [ ] **Step 3: 實作** `error.rs`、`http.rs`、`test_server.rs`（`filename_from_headers` 解析順序照 Python 版：先 `filename*=UTF-8''`，再引號、再無引號；header 原始位元組先嘗試 UTF-8 解碼）。
- [ ] **Step 4: 執行確認通過**：`cargo test --workspace http::tests` → 8 passed。
- [ ] **Step 5: Commit**：`feat(downloader): add streaming download with resume and checksum`

---

### Task 3: `Site` trait 與下載流程（列出 → 下載 → 重試）

**Files:**
- Create: `src-backend/crates/downloader/src/site.rs`
- Create: `src-backend/crates/downloader/src/runner.rs`

**Interfaces:**
- Consumes：Task 1 `LinkLine`、`SiteKind`、`Job`；Task 2 `DownloadError`、`client()`。
- Produces：
  ```rust
  // site.rs
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct FileItem { pub id: String, pub subdir: String, pub name: Option<String> }
  pub struct Ctx { pub app: Option<tauri::AppHandle>, pub http: reqwest::Client }
  pub type FileProgress<'a> = &'a (dyn Fn(&str, u64, Option<u64>) + Send + Sync); // (檔名, 已下載, 總大小)

  #[async_trait::async_trait]
  pub trait Site: Send {
      /// (資料夾名稱, 檔案)；單一檔案回傳 (None, vec![FileItem { id: job.id, .. }])
      async fn list_files(&mut self, ctx: &Ctx, job: &Job) -> Result<(Option<String>, Vec<FileItem>), DownloadError>;
      /// 下載到 out_dir，回傳最終檔案路徑；已存在且大小正確時直接回傳路徑
      async fn download(&mut self, ctx: &Ctx, item: &FileItem, password: &str, out_dir: &Path, progress: FileProgress<'_>) -> Result<PathBuf, DownloadError>;
      fn delay(&self) -> Duration { Duration::ZERO }
      fn retry_wait(&self, attempt: u32, _err: &DownloadError) -> Duration { Duration::from_secs(20 * attempt as u64) }
  }
  pub type SiteFactory = dyn Fn(SiteKind) -> Box<dyn Site> + Send + Sync;

  // runner.rs
  #[derive(Clone, Debug, PartialEq)]
  pub struct Progress { pub filename: String, pub file_index: u32, pub file_count: u32, pub downloaded: u64, pub total: Option<u64> }
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct LineError { pub line_number: u32, pub error: DownloadError }
  pub const MAX_TRIES: u32 = 4;
  pub async fn download_all(ctx: &Ctx, factory: &SiteFactory, lines: &[LinkLine], out_dir: &Path, progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<Vec<PathBuf>, LineError>;
  ```
- `download_all`：
  1. 每個 `SiteKind` 只建立一個 `Box<dyn Site>`（同一工作內共用，例如同一個隱藏瀏覽器）。
  2. **列出階段**：逐行 `list_files`（套用重試），得到全部檔案後才知道 `file_count`。
  3. **下載階段**：檔案存到 `out_dir/<line_number>/<資料夾名稱?>/<subdir>/`；逐檔 `download`（套用重試）；同一網站連續兩個檔案之間 `sleep(site.delay())`，最後一個檔案之後不等。
  4. 回傳要匯入的路徑（去重、保持順序）：資料夾工作回傳 `out_dir/<n>/<資料夾名稱>`，單一檔案工作回傳檔案路徑。
  5. 重試：`is_retryable()` 為 false 的錯誤立即回傳 `LineError`；可重試的錯誤最多 `MAX_TRIES` 次，每次之間 `sleep(site.retry_wait(attempt, &err))`。

- [ ] **Step 1: 寫失敗的測試**（`runner.rs` tests，用 `FakeSite`：預先設定好 list 結果與每次 download 要回傳的錯誤序列，下載時寫一個小檔案；`#[tokio::test(start_paused = true)]` 讓 sleep 立即完成）

```rust
#[tokio::test(start_paused = true)]
async fn retries_retryable_then_succeeds() { /* 前兩次 Retryable、第三次成功 → Ok，download 被呼叫 3 次 */ }

#[tokio::test(start_paused = true)]
async fn gives_up_after_max_tries() { /* 一直 Retryable → Err(LineError { line_number: 1, error: Retryable(_) })，呼叫 4 次 */ }

#[tokio::test(start_paused = true)]
async fn fatal_error_is_not_retried() { /* WrongPassword → 呼叫 1 次，Err.error == WrongPassword */ }

#[tokio::test(start_paused = true)]
async fn folder_job_returns_folder_path_and_counts_files() {
    // 第 1 行資料夾 "pack"（2 個檔案，其中一個 subdir "sub"），第 3 行單一檔案
    // 回傳 [out/1/pack, out/3/<file>]；progress 收到的 file_count 皆為 3，file_index 依序 1..=3
}

#[tokio::test(start_paused = true)]
async fn same_filename_on_two_lines_kept_separately() {
    // 兩行都下載 "Textures.zip" → 回傳兩個不同路徑 out/1/Textures.zip、out/2/Textures.zip，兩個檔案都存在
}
```

- [ ] **Step 2: 執行確認失敗**：`cargo test --workspace runner::tests` → 編譯錯誤。
- [ ] **Step 3: 實作** `site.rs`、`runner.rs`；`lib.rs` 匯出 `Site`、`Ctx`、`FileItem`、`SiteFactory`、`download_all`、`Progress`、`LineError`。
- [ ] **Step 4: 執行確認通過**：`cargo test --workspace runner::tests` → 5 passed。
- [ ] **Step 5: Commit**：`feat(downloader): add site trait and download runner with retries`

---

### Task 4: 直接連結、pixeldrain、SharePoint

**Files:**
- Create: `src-backend/crates/downloader/src/sites/mod.rs`
- Create: `src-backend/crates/downloader/src/sites/direct.rs`
- Create: `src-backend/crates/downloader/src/sites/pixeldrain.rs`
- Create: `src-backend/crates/downloader/src/sites/onedrive.rs`（本 Task 只做 SharePoint；個人 OneDrive 在 Task 6）

**Interfaces:**
- Consumes：Task 2 `save_stream`、`filename_from_headers`、`safe_name`；Task 3 `Site`。
- Produces：`pub fn sites::create(kind: SiteKind) -> Box<dyn Site>`（本 Task 未實作的網站暫時回傳會 `Fatal("not implemented")` 的 stub，Task 5／6 換掉）。
- Direct：GET 讀標頭（不讀 body）：404 → `NotFound`；沒檔名且 `content-type` 含 `text/html` → `NotAFile`；檔名順序 `filename_from_headers` → 最終網址路徑最後一段（percent-decode）→ `"download"`；目標已存在且大小相同 → 跳過；否則 `save_stream(最終網址)`。
- Pixeldrain：API base 為建構子參數（正式 `https://pixeldrain.com/api`，測試指向 test_server）。`list/<id>` → `(safe_name(title or id), files[].id)`；`file/<id>/info` → `name`、`size`、`hash_sha256`、`availability`；`availability` 非空 → `RateLimited(availability_message 或 availability + 檔案網址)`；404 → `NotFound`；下載 `file/<id>?download` 並以 SHA-256 校驗。
- SharePoint（`*.sharepoint.com`）：網址加上 `download=1` 查詢參數；沒檔名 → `LoginRequired`。

- [ ] **Step 1: 寫失敗的測試**（用 `test_server`）

```rust
#[tokio::test] async fn direct_uses_content_disposition_name() { /* 回 filename="a.zip" → 回傳 out/a.zip，內容正確 */ }
#[tokio::test] async fn direct_html_page_is_not_a_file() { /* content-type text/html、無 CD → Err(NotAFile) */ }
#[tokio::test] async fn direct_404_is_not_found() {}
#[tokio::test] async fn pixeldrain_list_and_download_with_checksum() { /* list 兩個檔案、info 帶正確 sha → 兩個檔案下載完成 */ }
#[tokio::test] async fn pixeldrain_availability_is_rate_limited() {}
#[test] fn sharepoint_adds_download_param() { assert_eq!(with_download_param("https://x.sharepoint.com/:u:/g/a?e=1"), "https://x.sharepoint.com/:u:/g/a?e=1&download=1"); }
```

- [ ] **Step 2: 執行確認失敗**：`cargo test --workspace sites::` → 編譯錯誤。
- [ ] **Step 3: 實作**三個網站與 `sites::create`。
- [ ] **Step 4: 執行確認通過**：`cargo test --workspace sites::` → 6 passed。
- [ ] **Step 5: Commit**：`feat(downloader): support direct links, pixeldrain and SharePoint`

---

### Task 5: Google Drive（檔案確認頁、資料夾）

**Files:**
- Create: `src-backend/crates/downloader/src/sites/gdrive.rs`
- Create: `src-backend/crates/downloader/test/gdrive_confirm.html`、`test/gdrive_folder.html`、`test/gdrive_quota.html`（fixture）

**Interfaces:**
- Produces（皆 `pub(crate)`，可單元測試）：
  ```rust
  fn confirm_form_url(html: &str) -> Option<String>;            // 解析 <form id="download-form" action=...> 的 hidden input，組成 GET 網址
  fn is_quota_page(html: &str) -> bool;                        // 含 "Too many users" 或 "quota"（不分大小寫）
  struct FolderEntry { id: String, name: String, is_folder: bool }
  fn parse_folder_listing(html: &str) -> (Option<String> /* <title> 去掉「 - Google 雲端硬碟/Google Drive」 */, Vec<FolderEntry>);
  ```
- 檔案：GET `https://drive.usercontent.google.com/download?id=<id>&export=download&confirm=t`；回應為 HTML 時：`is_quota_page` → `RateLimited`；`confirm_form_url` 有值 → 改用該網址；網址被導到 `accounts.google.com` → `LoginRequired`；都不是 → `NotAFile`。其餘照 Direct 的檔名與 `save_stream` 流程。
- 資料夾：GET `https://drive.google.com/embeddedfolderview?id=<id>`，以 `parse_folder_listing` 取得項目；子資料夾遞迴，`FileItem.subdir` 為以 `\` 連接的相對路徑；`FileItem.name = Some(name)`；資料夾名稱經 `safe_name`。404 或無項目且頁面不是資料夾 → `LoginRequired("folder is not shared publicly")`。
- fixture 依實際頁面結構撰寫：資料夾頁每個項目為 `<div class="flip-entry" id="entry-<ID>">…<a href="https://drive.google.com/file/d/<ID>/view?usp=drive_web" …><div class="flip-entry-title">名稱</div>`，子資料夾的 `href` 為 `https://drive.google.com/drive/folders/<ID>`。可行時以一個公開資料夾實際抓一次確認結構後再寫 fixture。

- [ ] **Step 1: 寫失敗的測試**

```rust
#[test] fn parses_confirm_form() { assert_eq!(confirm_form_url(include_str!("../../test/gdrive_confirm.html")).unwrap(), "https://drive.usercontent.google.com/download?id=1AbC&export=download&confirm=t&uuid=u-1"); }
#[test] fn detects_quota_page() { assert!(is_quota_page(include_str!("../../test/gdrive_quota.html"))); assert!(!is_quota_page(include_str!("../../test/gdrive_confirm.html"))); }
#[test] fn parses_folder_listing() {
    let (title, entries) = parse_folder_listing(include_str!("../../test/gdrive_folder.html"));
    assert_eq!(title.as_deref(), Some("素材パック"));
    assert_eq!(entries, vec![
        FolderEntry { id: "1file".into(), name: "a.zip".into(), is_folder: false },
        FolderEntry { id: "1sub".into(), name: "Textures".into(), is_folder: true },
    ]);
}
```

- [ ] **Step 2: 執行確認失敗**：`cargo test --workspace gdrive::tests` → 編譯錯誤。
- [ ] **Step 3: 實作** `gdrive.rs`，`sites::create(GoogleDrive)` 改回傳它；`delay()` 2 秒。
- [ ] **Step 4: 執行確認通過**：`cargo test --workspace gdrive::tests` → 3 passed。
- [ ] **Step 5: Commit**：`feat(downloader): support Google Drive files and folders`

---

### Task 6: 隱藏 WebView2 驅動、workupload、個人 OneDrive

**Files:**
- Create: `src-backend/crates/downloader/src/browser.rs`
- Create: `src-backend/crates/downloader/src/sites/workupload.rs`
- Modify: `src-backend/crates/downloader/src/sites/onedrive.rs`

**Interfaces:**
- Produces：
  ```rust
  // browser.rs
  #[derive(Debug, Clone, PartialEq, serde::Deserialize)]
  pub struct Report { pub kind: String, #[serde(flatten)] pub data: serde_json::Map<String, serde_json::Value> }
  pub fn decode_report(url: &tauri::Url) -> Option<Report>;    // host == "konoasset.invalid" 且 path == "/report"
  pub struct DownloadRequest { pub url: tauri::Url, pub suggested_name: String }
  pub struct HiddenBrowser { /* WebviewWindow、reports/downloads 的 mpsc receiver、native_destination: Arc<Mutex<Option<PathBuf>>> */ }
  impl HiddenBrowser {
      pub async fn open(app: &AppHandle, url: &tauri::Url, init_script: &str) -> Result<Self, DownloadError>;
      pub fn navigate(&self, url: &tauri::Url) -> Result<(), DownloadError>;
      pub fn eval(&self, js: &str) -> Result<(), DownloadError>;
      pub fn current_url(&self) -> Option<tauri::Url>;
      pub async fn wait_report(&mut self, timeout: Duration, kind: &str) -> Result<Report, DownloadError>; // 逾時 → Retryable
      pub async fn wait_download(&mut self, timeout: Duration) -> Result<DownloadRequest, DownloadError>;
      pub fn cookie_header(&self, url: &tauri::Url) -> Result<String, DownloadError>;
      /// 讓 WebView2 自己把下一個下載存到 dest，等待完成
      pub async fn native_download(&mut self, trigger: &tauri::Url, dest: &Path, timeout: Duration) -> Result<(), DownloadError>;
  }
  impl Drop for HiddenBrowser { /* window.destroy()，錯誤只記 log */ }
  ```
- `open`：`WebviewWindowBuilder::new(app, format!("dl-{}", Uuid::new_v4()), WebviewUrl::External(url))`，`.visible(false)`、`.incognito(true)`、`.user_agent(USER_AGENT)`、`.initialization_script(init_script)`、`.on_new_window(|_, _| NewWindowResponse::Deny)`、`.on_navigation(...)`（`decode_report` 成功 → 送進 reports channel 並回傳 `false`；其他 → `true`）、`.on_download(...)`（`Requested`：`native_destination` 有值時設定 `destination` 並回傳 `true`；否則送 `DownloadRequest { url, suggested_name: destination.file_name() }` 並回傳 `false`。`Finished` 送進 native 完成 channel）。
- `cookie_header`：`cookies_for_url` 必須在 tokio 執行緒呼叫（Windows 在主執行緒呼叫會死結，見 Tauri 文件 Known issues）。
- 共用 init script 工具：`fn report_js() -> &'static str`，定義 `window.__konoReport = (kind, data) => { location.href = 'https://konoasset.invalid/report?d=' + encodeURIComponent(JSON.stringify({ kind, ...data })) }`。
- **workupload**（`Workupload { browser: Option<HiddenBrowser> }`，`delay()` 10 秒，`retry_wait` 依 Global Constraints）：
  - init script：hostname 為 `workupload.com` 時，每 500ms 檢查一次，直到 `document.readyState !== 'loading'` 且標題不含 `human`，回報 `ready`：`{ title, hasPassword: !!document.querySelector("input[type=password][id^=passwordprotected_]"), fileIds: /* a[href^='/file/'] 的 ID，去重保持順序 */ }`。
  - `open_unlocked(url, password)`：第一次 `HiddenBrowser::open`，之後 `navigate`；`wait_report(90s, "ready")`；`hasPassword` 時：密碼空 → `PasswordRequired`；否則 `eval` 填入密碼（以 `serde_json::to_string` 跳脫）並按 `button[type=submit][id^=passwordprotected_]`，再 `wait_report(90s, "ready")`，仍 `hasPassword` → `WrongPassword`。
  - `list_files`：File → `(None, [id])`；Folder → 開 `/archive/<id>` 解鎖，回傳 `(Some(id), fileIds)`。
  - `download`：開 `/file/<id>` 解鎖；`title` 非空且不是 `workupload`、`out_dir/title` 已存在 → 跳過；`navigate(/start/<id>)` → `wait_download(60s)`（逾時 → `Retryable("download did not start")`）；以 `cookie_header` + `Referer: https://workupload.com/` 呼叫 `save_stream(request.url, out_dir/safe_name(suggested_name))`。`SaveOptions.reject_html = true`；回傳 `NotAFile(_)` 時改用 `native_download(/start/<id>, 目標, 6 小時)`，進度回報 `(0, None)`。
- **個人 OneDrive**：init script（hostname 含 `onedrive` 或 `1drv`）在載入完成後每秒依序檢查 Python 版的 5 個下載按鈕選擇器，找到可見的就 `click()` 並回報 `clicked`；45 秒找不到回報 `no-button`。Rust 端：`open` → `wait_report(60s, "clicked")`；收到 `no-button` 時 `current_url` 含 `login.live.com` 或 `login.microsoftonline.com` → `LoginRequired`，否則 `Retryable`；`clicked` 後 `wait_download(180s)`，接著同 workupload 的 cookie + `save_stream`／`native_download` 流程。
- 本 Task 的 WebView2 部分無法單元測試；只測 `decode_report`，實際網站在 Task 12 手動驗證。

- [ ] **Step 1: 寫失敗的測試**（`browser.rs` tests）

```rust
#[test]
fn decodes_report_url() {
    let url = Url::parse("https://konoasset.invalid/report?d=%7B%22kind%22%3A%22ready%22%2C%22title%22%3A%22a.zip%22%2C%22hasPassword%22%3Afalse%7D").unwrap();
    let r = decode_report(&url).unwrap();
    assert_eq!(r.kind, "ready");
    assert_eq!(r.data["title"], "a.zip");
    assert!(decode_report(&Url::parse("https://workupload.com/file/abc").unwrap()).is_none());
    assert!(decode_report(&Url::parse("https://konoasset.invalid/report?d=not-json").unwrap()).is_none());
}
```

- [ ] **Step 2: 執行確認失敗**：`cargo test --workspace browser::tests` → 編譯錯誤。
- [ ] **Step 3: 實作** `browser.rs`、`workupload.rs`、`onedrive.rs` 個人帳號分支；`sites::create` 改用實作（`Ctx.app` 為 `None` 時 workupload／個人 OneDrive 回傳 `Fatal("browser unavailable")`）。
- [ ] **Step 4: 執行確認通過**：`cargo test --workspace browser::tests` → 1 passed；`cargo build --workspace` 無警告。
- [ ] **Step 5: Commit**：`feat(downloader): drive workupload and OneDrive through a hidden WebView2`

---

### Task 7: 匯入函式改為可不送進度事件

**Files:**
- Modify: `src-backend/tauri/src/importer/import_wrapper.rs:118-220`（`import_avatar`、`import_avatar_wearable`、`import_world_object`、`import_other_asset`）
- Modify: `src-backend/tauri/src/command/asset/create.rs`（4 個呼叫端）

**Interfaces:**
- Produces：四個函式的 `app_handle: &AppHandle` 改為 `app_handle: Option<&AppHandle>`，直接傳給內部 `import_asset`。現有呼叫端改傳 `Some(&cloned_app_handle)`，行為不變。

- [ ] **Step 1: 修改簽章與呼叫端。**
- [ ] **Step 2: 確認**：`cargo test --workspace` 全部通過；`cargo build --workspace` 無警告。
- [ ] **Step 3: Commit**：`refactor(importer): make import progress events optional`

---

### Task 8: `DownloadJobManager`（背景工作佇列）

**Files:**
- Create: `src-backend/tauri/src/download_job/mod.rs`
- Create: `src-backend/tauri/src/download_job/definitions.rs`
- Create: `src-backend/tauri/src/download_job/manager.rs`
- Modify: `src-backend/tauri/src/lib.rs`（`mod download_job;`）
- Modify: `src-backend/tauri/Cargo.toml`（加 `downloader.workspace = true`、`async-trait.workspace = true`）

**Interfaces:**
- Consumes：Task 1 `valid_lines`、`LinkLine`；Task 2 `DownloadErrorKind`；Task 3 `Progress`、`LineError`。
- Produces：
  ```rust
  // definitions.rs（specta::Type，serde camelCase）
  pub enum DownloadJobStatus { Queued, Downloading, Importing, Completed, Failed, Cancelled }
  pub struct DownloadJob {
      pub id: Uuid, pub asset_name: String, pub status: DownloadJobStatus, pub site: SiteKind,
      pub current_file: Option<String>, pub file_index: u32, pub file_count: Option<u32>,
      pub downloaded_bytes: u64, pub total_bytes: Option<u64>,
      pub error: Option<String>, pub error_kind: Option<DownloadErrorKind>, pub failed_line: Option<u32>,
      pub created_at: i64,
  }
  pub enum DownloadPreAsset { Avatar(PreAvatar), AvatarWearable(PreAvatarWearable), WorldObject(PreWorldObject), OtherAsset(PreOtherAsset) } // Deserialize、Clone
  pub struct DownloadImportRequest { pub links_text: String, pub pre_asset: DownloadPreAsset }
  #[derive(Serialize, Clone, specta::Type, tauri_specta::Event)] pub struct DownloadJobUpdated(pub DownloadJob);

  // manager.rs
  #[async_trait]
  pub trait JobBackend: Send + Sync + 'static {
      async fn download(&self, lines: Vec<LinkLine>, out_dir: PathBuf, progress: Arc<dyn Fn(Progress) + Send + Sync>) -> Result<Vec<PathBuf>, LineError>;
      async fn import(&self, pre_asset: DownloadPreAsset, paths: Vec<PathBuf>) -> Result<(), String>;
  }
  pub type EmitFn = Arc<dyn Fn(DownloadJob) + Send + Sync>;
  pub struct DownloadJobManager { /* root, backend, emit, jobs: Vec<Entry>, download_slots: Arc<Semaphore>(3), exclusive_slot: Arc<Semaphore>(1) */ }
  impl DownloadJobManager {
      pub fn new(root: PathBuf, backend: Arc<dyn JobBackend>, emit: EmitFn) -> Self;
      pub fn cleanup_root(root: &Path);                                  // 刪除整個 root 後重建空資料夾，錯誤只記 log
      pub fn enqueue(&mut self, request: DownloadImportRequest) -> Result<Uuid, String>; // valid_lines 失敗 → Err
      pub fn list(&self) -> Vec<DownloadJob>;                            // created_at 由新到舊
      pub async fn cancel(&mut self, id: Uuid) -> Result<(), String>;    // 只有 Queued/Downloading/Importing 可取消
      pub fn retry(&mut self, id: Uuid, password: Option<String>) -> Result<(), String>; // 只有 Failed 可重試
      pub async fn dismiss(&mut self, id: Uuid) -> Result<(), String>;   // 只有 Completed/Failed/Cancelled 可移除
  }
  ```
- 工作執行（`tokio::spawn`，保存 `AbortHandle`）：`Queued` → 取得 `download_slots`（任一行 `site.is_exclusive()` 時再取得 `exclusive_slot`）→ `Downloading` → `backend.download(lines, root/<id>, progress)` → 釋放號誌 → `Importing` → `backend.import` → `Completed` 並刪除 `root/<id>`。下載錯誤 → `Failed`，填 `error`（`LineError.error` 的字串）、`error_kind`、`failed_line`，保留資料夾；匯入錯誤 → `Failed`、`error_kind = ImportFailed`。`site` 取第一行的 `SiteKind`；`asset_name` 取 `pre_asset` 的 `description.name`。
- `retry(id, Some(pw))`：把 `failed_line` 那一行的密碼換成 `pw` 後重新執行（同一個 `id` 與資料夾，已完成的檔案由各網站的「已存在就跳過」接續）；`failed_line` 為匯入失敗時也是從下載開始（各檔案都會被跳過）。
- `cancel`：abort 任務、狀態 `Cancelled`、刪除 `root/<id>`。
- 進度事件節流 250ms（狀態改變不節流）；`emit` 每次送出完整 `DownloadJob`。

- [ ] **Step 1: 寫失敗的測試**（`manager.rs` tests；`FakeBackend` 可設定 download／import 的結果、可用 `tokio::sync::Notify` 卡住、記錄呼叫次數與收到的密碼；`emit` 收集到 `Vec<DownloadJob>`）

```rust
#[tokio::test] async fn job_goes_through_all_stages() { /* emit 的 status 依序含 Queued, Downloading, Importing, Completed；root/<id> 不存在 */ }
#[tokio::test] async fn invalid_links_are_rejected() { /* links_text "not a link" → enqueue Err，list 為空 */ }
#[tokio::test] async fn download_failure_keeps_dir_and_reports_kind() { /* LineError{line 2, WrongPassword} → Failed, error_kind WrongPassword, failed_line Some(2)，root/<id> 存在 */ }
#[tokio::test] async fn retry_with_password_replaces_failed_line_password() { /* 第二次 download 收到的第 2 行 job.password == "new" → Completed */ }
#[tokio::test] async fn cancel_removes_dir() { /* download 卡住時 cancel → Cancelled，root/<id> 不存在 */ }
#[tokio::test] async fn exclusive_jobs_run_one_at_a_time() { /* 兩個 workupload 工作，download 卡住 → 同時只有 1 個在 Downloading，另一個 Queued */ }
#[tokio::test] async fn import_failure_is_import_failed() {}
#[test] fn cleanup_root_removes_leftovers() { /* root/old-job/1/a.zip 存在 → cleanup 後 root 存在且為空 */ }
```

- [ ] **Step 2: 執行確認失敗**：`cargo test --workspace download_job::` → 編譯錯誤。
- [ ] **Step 3: 實作** `definitions.rs`、`manager.rs`。
- [ ] **Step 4: 執行確認通過**：`cargo test --workspace download_job::` → 8 passed。
- [ ] **Step 5: Commit**：`feat(download_job): add background download job manager`

---

### Task 9: 實際 backend、Tauri 指令與事件註冊、bindings

**Files:**
- Create: `src-backend/tauri/src/download_job/backend.rs`
- Create: `src-backend/tauri/src/download_job/commands.rs`
- Modify: `src-backend/tauri/src/command/mod.rs`（註冊指令，加一段 `// ダウンロード関連` 註解）
- Modify: `src-backend/tauri/src/lib.rs`（`collect_events!` 加 `DownloadJobUpdated`；setup 中 `cleanup_root` 並 `manage(arc_mutex(DownloadJobManager::new(...)))`）
- Modify: `src/lib/bindings.ts`（手動加入與產生結果相同的型別、指令、事件）

**Interfaces:**
- Consumes：Task 7 的 `import_*`（傳 `None`）；Task 8 `JobBackend`、`DownloadJobManager`。
- Produces：
  ```rust
  pub struct TauriJobBackend { app: AppHandle } // download: downloader::download_all(Ctx { app: Some, http: client() }, &|k| sites::create(k), ...)
                                                 // import: 鎖 AssetStorage、讀 preference.zip_extraction，依 DownloadPreAsset 呼叫 import_*(…, None, zip_extraction, false)，
                                                 //         AssetImportRequest { pre_asset, absolute_paths: 路徑字串, delete_source: true }
  #[tauri::command] #[specta::specta] pub fn parse_download_links(text: String) -> Vec<ParsedLine>;
  #[tauri::command] #[specta::specta] pub async fn request_download_import(manager: State<'_, Arc<Mutex<DownloadJobManager>>>, request: DownloadImportRequest) -> Result<Uuid, String>;
  #[tauri::command] #[specta::specta] pub async fn list_download_jobs(manager: …) -> Result<Vec<DownloadJob>, String>;
  #[tauri::command] #[specta::specta] pub async fn cancel_download_job(manager: …, id: Uuid) -> Result<(), String>;
  #[tauri::command] #[specta::specta] pub async fn retry_download_job(manager: …, id: Uuid, password: Option<String>) -> Result<(), String>;
  #[tauri::command] #[specta::specta] pub async fn dismiss_download_job(manager: …, id: Uuid) -> Result<(), String>;
  ```
- 前端 bindings 名稱：`commands.parseDownloadLinks`、`requestDownloadImport`、`listDownloadJobs`、`cancelDownloadJob`、`retryDownloadJob`、`dismissDownloadJob`；`events.downloadJobUpdated`；型別 `ParsedLine`、`SiteKind`、`JobKind`、`DownloadJob`、`DownloadJobStatus`、`DownloadErrorKind`、`DownloadPreAsset`、`DownloadImportRequest`。
- `DownloadJobManager` 在 setup 中於 `load_store_provider` 之前建立（root 取自 `app_local_data_dir`），`emit` 以 `DownloadJobUpdated(job).emit(&app_handle)`，失敗只記 log。

- [ ] **Step 1: 實作** backend、commands、註冊與 setup。
- [ ] **Step 2: 手動更新** `src/lib/bindings.ts`（仿既有 `requestAvatarImport` 與 `taskStatusChanged` 的產生格式）。
- [ ] **Step 3: 確認**：`cargo test --workspace` 全部通過；專案根目錄 `pnpm type-check` 通過。
- [ ] **Step 4: Commit**：`feat(download_job): expose download job commands and events`

---

### Task 10: 新增素材對話框的連結輸入

**Files:**
- Create: `src/components/model-legacy/asset-dialogs/components/tabs/AssetPathSelectorTab/selector/SelectLinkCard/index.tsx`
- Create: `src/components/model-legacy/asset-dialogs/components/tabs/LinkInputTab/index.tsx`
- Create: `src/components/model-legacy/asset-dialogs/components/tabs/LinkInputTab/logic.ts`
- Test: `src/components/model-legacy/asset-dialogs/components/tabs/LinkInputTab/logic.test.ts`
- Test: `src/components/model-legacy/asset-dialogs/AddAssetDialog/logic/index.test.ts`（新增）
- Modify: `AssetPathSelectorTab/index.tsx`（第三張卡片；三張各 `w-[200px]`，容器 `w-[650px]`；選檔案／資料夾時 `setDownloadLinks(undefined)`）
- Modify: `AddAssetDialog/index.tsx`（context 加 `downloadLinks?: string`、`setDownloadLinks`；新增 `link-input` TabsContent；連結模式時 `AdditionalInputTab` 傳 `hideDeleteSourceCheckbox`）
- Modify: `AddAssetDialog/hook/index.ts`（state、`clearForm` 一併清空連結、`submit` 分支）
- Modify: `AddAssetDialog/logic/index.ts`（新增 `sendDownloadImportRequest`）
- Modify: `components/tabs/BoothInputTab/add/hook/index.ts:105-111`（連結模式的代表名稱與數量）
- Modify: `components/tabs/DuplicateWarningTab/index.tsx`（連結模式隱藏「加入到這裡」）
- Modify: `src/locales/*.json`（5 個檔案）

**Interfaces:**
- Consumes：Task 9 的 `commands.parseDownloadLinks`、`commands.requestDownloadImport`、`ParsedLine`、`SiteKind`、`DownloadPreAsset`。
- Produces：
  ```ts
  // LinkInputTab/logic.ts
  export const canProceed = (parsed: ParsedLine[]): boolean // 至少一行且全部 valid
  export const siteDisplayName = (site: SiteKind, t: (key: string) => string): string
  // Workupload → 'workupload'、Pixeldrain → 'pixeldrain'、GoogleDrive → 'Google Drive'、OneDrive → 'OneDrive'、Direct → t('addasset:link-input:site:direct')
  export const representativeLinkLabel = (parsed: ParsedLine[], t: (key: string) => string): { label: string; count: number }
  // label = `${siteDisplayName}：${第一個有效行網址路徑的最後一段}`；count = 有效行數
  // AddAssetDialog/logic/index.ts
  export const buildDownloadPreAsset = (assetType: AssetType, preAsset: PreAvatar | PreAvatarWearable | PreWorldObject | PreOtherAsset): Result<DownloadPreAsset, string>
  export const sendDownloadImportRequest = (assetType, linksText: string, preAsset) => Promise<Result<string, string>>
  ```
- `LinkInputTab`：`Textarea`（`rows={6}`、`font-mono`）、輸入後 300ms debounce 呼叫 `parseDownloadLinks`；每行顯示行號、網站名稱 `Badge`、`kind`（檔案／資料夾）、有密碼時 `KeyRound` 圖示；無效行文字 `text-destructive` 並顯示 `addasset:link-input:invalid`。下方顯示 `addasset:link-input:supported-sites`。按鈕：返回（`selector`）、下一步（`canProceed` 為 false 時 disabled，按下 `setDownloadLinks(text)`、`setAssetPaths([])`、`setTab('booth-input')`）。標題 `(1/5) {t('addasset:link-input')}`。
- `submit`：`downloadLinks` 有值時略過 `validatePaths`，`sendDownloadImportRequest` 成功 → toast `addasset:link-input:queued-toast`（description `…:description`），`setTab('empty')`、`setDialogOpen(false)`；失敗 → toast `addasset:import-start-error-toast` 並留在對話框。
- 語言鍵（zh-TW ／ en-US；ja-JP、zh-CN 自行翻譯，en-GB 同 en-US）：

| key | zh-TW | en-US |
|---|---|---|
| `addasset:select-link` | 下載連結 | Download link |
| `addasset:link-input` | 輸入下載連結 | Enter download links |
| `addasset:link-input:explanation-text` | 一行一個連結，需要密碼的在後面空一格寫密碼 | One link per line. Add a space and the password after links that need one |
| `addasset:link-input:placeholder` | `https://workupload.com/file/XXXX 密碼` 換行 `https://pixeldrain.com/u/YYYY` | `https://workupload.com/file/XXXX password` newline `https://pixeldrain.com/u/YYYY` |
| `addasset:link-input:supported-sites` | 支援 workupload、pixeldrain、Google Drive、OneDrive 及直接下載連結 | Supports workupload, pixeldrain, Google Drive, OneDrive and direct download links |
| `addasset:link-input:kind:file` | 檔案 | File |
| `addasset:link-input:kind:folder` | 資料夾 | Folder |
| `addasset:link-input:invalid` | 無法辨識的連結 | Unrecognized link |
| `addasset:link-input:site:direct` | 直接連結 | Direct link |
| `addasset:link-input:queued-toast` | 已加入背景下載 | Added to background downloads |
| `addasset:link-input:queued-toast:description` | 可在狀態列查看進度 | You can check progress in the status bar |

- [ ] **Step 1: 寫失敗的測試**

```ts
// LinkInputTab/logic.test.ts
const line = (p: Partial<ParsedLine>): ParsedLine => ({ lineNumber: 1, text: '', site: null, kind: null, hasPassword: false, valid: false, ...p })
test('canProceed requires at least one line and all valid', () => {
  expect(canProceed([])).toBe(false)
  expect(canProceed([line({ valid: true, site: 'Workupload' }), line({ valid: false })])).toBe(false)
  expect(canProceed([line({ valid: true, site: 'Workupload' })])).toBe(true)
})
test('representativeLinkLabel uses first valid line', () => {
  expect(representativeLinkLabel([
    line({ valid: true, site: 'Workupload', text: 'https://workupload.com/archive/q2hv3vcBcy 12345' }),
    line({ lineNumber: 2, valid: true, site: 'Pixeldrain', text: 'https://pixeldrain.com/u/pHP3XpbB' }),
  ], (k) => k)).toEqual({ label: 'workupload：q2hv3vcBcy', count: 2 })
})
// AddAssetDialog/logic/index.test.ts
test('buildDownloadPreAsset wraps by asset type', () => {
  const r = buildDownloadPreAsset('WorldObject', { description, category: 'Furniture' })
  expect(r).toEqual({ status: 'ok', data: { WorldObject: { description, category: 'Furniture' } } })
})
```

- [ ] **Step 2: 執行確認失敗**：`pnpm test:run src/components/model-legacy/asset-dialogs` → 新測試失敗（模組不存在）。
- [ ] **Step 3: 實作**上列元件、hook、logic 與語系。
- [ ] **Step 4: 確認**：`pnpm test:run src/components/model-legacy/asset-dialogs` 全部通過；`node .github/workflows/scripts/locales-test.js` 通過；`pnpm type-check`、`pnpm lint` 通過。
- [ ] **Step 5: Commit**：`feat(add-asset): add download link input to the add asset dialog`

---

### Task 11: 背景工作 store 與狀態列 Popover

**Files:**
- Create: `src/stores/DownloadJobStore/index.ts`
- Create: `src/stores/DownloadJobStore/logic.ts`
- Test: `src/stores/DownloadJobStore/logic.test.ts`
- Create: `src/components/models/download-jobs/DownloadJobsButton/DownloadJobsButton.tsx`、`index.ts`
- Create: `src/components/models/download-jobs/DownloadJobsPopover/DownloadJobsPopover.tsx`、`index.ts`
- Create: `src/components/models/download-jobs/DownloadJobRow/DownloadJobRow.tsx`、`index.ts`
- Create: `src/components/models/download-jobs/DownloadJobRow/DownloadJobRow.stories.tsx`（Queued／Downloading 有大小／Downloading 未知大小／Importing／Failed 需要密碼／Completed）
- Modify: `src/components/models/status-bar/StatusBar/internal/index.tsx`（右側加 `<DownloadJobsButton />`）
- Modify: `src/locales/*.json`

**Interfaces:**
- Consumes：Task 9 bindings。
- Produces：
  ```ts
  // logic.ts
  export const upsertJob = (jobs: DownloadJob[], job: DownloadJob): DownloadJob[] // 依 id 取代或加到最前面
  export const detectTransitions = (prev: DownloadJob | undefined, next: DownloadJob): 'completed' | 'failed' | null // 只有狀態「變成」Completed／Failed 時回傳
  export const progressPercent = (job: DownloadJob): number | null // totalBytes 為 null 或 0 → null；否則 0–100
  export const activeCount = (jobs: DownloadJob[]): number          // Queued + Downloading + Importing
  // index.ts（zustand）
  type State = { jobs: DownloadJob[]; initialized: boolean; initialize: (onTransition: (job: DownloadJob, t: 'completed' | 'failed') => void) => Promise<void> }
  ```
- `initialize` 只執行一次：`listDownloadJobs` 填入 → `events.downloadJobUpdated.listen` 以 `upsertJob` 更新並在 `detectTransitions` 非 null 時呼叫 `onTransition`。
- `DownloadJobsButton`：在 `useEffect` 中 `initialize`，`onTransition` 為：completed → toast `addasset:success-toast`（description 素材名稱）＋ `refreshAssetSummaries()` ＋ `refreshFilteredIds()`；failed → toast `download-jobs:failed-toast`（description 素材名稱）。`jobs` 為空時不顯示；否則 ghost `Button` + `Download` 圖示，`activeCount > 0` 時右上角徽章；點擊開啟 `Popover`（`w-[420px]`，`ScrollArea` 最高 `400px`）。
- `DownloadJobRow`：素材名稱、網站、狀態文字；`Progress`（`progressPercent` 為 null 且 Downloading 時用不確定動畫 `animate-pulse`）；`currentFile (fileIndex/fileCount)`；Failed 時顯示 `download-jobs:error:<kind>` ＋ 細節（`text-muted-foreground text-xs`）；按鈕：進行中 → 取消；Failed → 重試、移除，`errorKind` 為 PasswordRequired／WrongPassword 時重試按鈕前出現密碼 `Input`（重試時帶入）；Completed／Cancelled → 移除。
- 語言鍵（zh-TW ／ en-US）：

| key | zh-TW | en-US |
|---|---|---|
| `download-jobs:title` | 背景下載 | Background downloads |
| `download-jobs:status:queued` | 等待中 | Queued |
| `download-jobs:status:downloading` | 下載中 | Downloading |
| `download-jobs:status:importing` | 匯入中 | Importing |
| `download-jobs:status:completed` | 完成 | Completed |
| `download-jobs:status:failed` | 失敗 | Failed |
| `download-jobs:status:cancelled` | 已取消 | Cancelled |
| `download-jobs:button:cancel` | 取消 | Cancel |
| `download-jobs:button:retry` | 重試 | Retry |
| `download-jobs:button:dismiss` | 移除 | Remove |
| `download-jobs:password-placeholder` | 輸入密碼 | Enter password |
| `download-jobs:failed-toast` | 下載失敗 | Download failed |
| `download-jobs:error:password-required` | 需要密碼 | Password required |
| `download-jobs:error:wrong-password` | 密碼錯誤 | Wrong password |
| `download-jobs:error:not-found` | 找不到檔案（可能已被刪除） | File not found (it may have been deleted) |
| `download-jobs:error:login-required` | 需要登入，或連結沒有公開分享 | Login required, or the link is not shared publicly |
| `download-jobs:error:rate-limited` | 網站限制下載，請稍後再試或用瀏覽器手動下載 | The site is limiting downloads. Try again later or download it in your browser |
| `download-jobs:error:not-a-file` | 這個連結打開是網頁，不是檔案 | This link opens a web page, not a file |
| `download-jobs:error:import-failed` | 匯入失敗 | Import failed |
| `download-jobs:error:other` | 下載失敗 | Download failed |

- [ ] **Step 1: 寫失敗的測試**（`logic.test.ts`）

```ts
test('upsertJob replaces by id or prepends', () => { /* 既有 id → 取代同位置；新 id → index 0 */ })
test('detectTransitions only fires on change', () => {
  expect(detectTransitions(job({ status: 'Importing' }), job({ status: 'Completed' }))).toBe('completed')
  expect(detectTransitions(job({ status: 'Completed' }), job({ status: 'Completed' }))).toBeNull()
  expect(detectTransitions(undefined, job({ status: 'Failed' }))).toBe('failed')
  expect(detectTransitions(job({ status: 'Queued' }), job({ status: 'Downloading' }))).toBeNull()
})
test('progressPercent', () => {
  expect(progressPercent(job({ downloadedBytes: 50, totalBytes: 200 }))).toBe(25)
  expect(progressPercent(job({ downloadedBytes: 50, totalBytes: null }))).toBeNull()
  expect(progressPercent(job({ downloadedBytes: 0, totalBytes: 0 }))).toBeNull()
})
test('activeCount', () => { /* Queued/Downloading/Importing 各一、Completed/Failed 各一 → 3 */ })
```

- [ ] **Step 2: 執行確認失敗**：`pnpm test:run src/stores/DownloadJobStore` → 失敗。
- [ ] **Step 3: 實作** store、元件、stories、語系。
- [ ] **Step 4: 確認**：`pnpm test:run src/stores/DownloadJobStore` 通過；`locales-test.js`、`pnpm type-check`、`pnpm lint` 通過。
- [ ] **Step 5: Commit**：`feat(download-jobs): show background download jobs in the status bar`

---

### Task 12: 整合確認、bindings 重新產生、更新紀錄

**Files:**
- Modify: `src/lib/bindings.ts`（以產生結果覆蓋）
- Modify: `changelog.json`（在最新版本的 `features` 加一條，含 `ja-JP` 與 `zh-TW`）

- [ ] **Step 1**：`pnpm tauri dev` 啟動；`git diff src/lib/bindings.ts` 確認與 Task 9 手動內容一致（有差異以產生結果為準）。
- [ ] **Step 2: 手動端對端**（使用 `C:\Users\Taiyo\Desktop\workupload下載器\links.txt` 的實際連結），每項確認後打勾：
  - workupload 單一檔案＋密碼（`file/DQTyYjAQqUJ huna`）→ 素材建立、內容正確、`downloads/` 已清空。
  - workupload 資料夾＋密碼（`archive/q2hv3vcBcy 12345`）→ 17 個 zip 匯入成一個素材，進度顯示 `(n/17)`。
  - pixeldrain 檔案（`u/pHP3XpbB`）→ 有百分比進度。
  - 第一個工作下載中時再新增第二個素材 → 兩個工作同時出現在 Popover，workupload 第二個顯示「等待中」。
  - 密碼留空的 workupload 連結 → 失敗並顯示「需要密碼」與密碼輸入框，輸入後重試成功。
  - 下載中取消 → 狀態「已取消」、沒有殘留的隱藏視窗（工作管理員中 WebView2 程序數回到原本）、`downloads/<id>` 已刪除。
  - 前景同時用「選擇檔案」匯入另一個素材 → 前景進度條不受背景工作影響。
  - workupload 的 cookie＋reqwest 路徑是否成功；若伺服器拒絕，確認 `native_download` 備援可完成（記錄實際走哪一條）。
  - Google Drive 與 OneDrive 若手邊有公開連結則各測一個；沒有則在回報中註明未測。
- [ ] **Step 3**：`changelog.json` 新增 feature：`"text": "Assets can now be added directly from download links (workupload, pixeldrain, Google Drive, OneDrive, direct links) and are downloaded and imported in the background"`，ja-JP `"ダウンロードリンクから直接アセットを追加できるように（バックグラウンドでダウンロード・インポート）"`，zh-TW `"可直接從下載連結新增素材（支援 workupload、pixeldrain、Google Drive、OneDrive 與直接連結），並在背景下載與匯入"`。
- [ ] **Step 4: 確認**：`cargo test --workspace`、`pnpm test:run`、`pnpm type-check`、`pnpm lint`、`locales-test.js` 全部通過。
- [ ] **Step 5: Commit**：`chore: regenerate bindings and add changelog for download link import`
