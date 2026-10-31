# 從下載連結新增素材（背景下載＋匯入）— 設計規格

- 日期：2026-10-31
- 狀態：待審查
- 範圍：Feature B（貼上下載連結 → 背景下載 → 自動匯入）。參考實作：`C:\Users\Taiyo\Desktop\workupload下載器`（Python + Playwright）。

## 1. 目的

現在新增素材前，使用者必須先另外下載檔案、再拖進 KonoAsset，等待時間長且無法同時處理多個。
本功能讓使用者在「新增素材」的第一步直接貼上下載連結，填完素材資訊後按下「新增素材」，
對話框立刻關閉，下載與匯入在背景執行，使用者可以馬上開始新增下一個素材。

### 成功條件

- 第一步多出「下載連結」選項，可貼上一行或多行連結（可附密碼）。
- 按下「新增素材」後對話框立即關閉，背景工作出現在狀態列的「下載中」清單。
- 可同時排入多個工作；每個工作顯示階段（等待中／下載中／匯入中）、進度、目前檔名。
- 完成時跳出通知並重新整理素材列表；失敗時顯示原因，可重試（續傳）或移除。
- 不需要安裝 Python、Playwright 或 Chrome；只用 KonoAsset 本身（Rust＋WebView2）。
- 不使用下載連結時，現有流程完全不變。

### 已確定的決策

| 項目 | 決定 |
|---|---|
| 背景範圍 | 下載＋匯入都在背景；對話框按下「新增素材」後立即關閉 |
| 支援網站 | workupload、pixeldrain、Google Drive、OneDrive、直接下載連結（共 5 種） |
| 實作方式 | Rust 重寫；需要真瀏覽器的網站用隱藏的 Tauri WebView2 視窗 |
| 連結與素材的關係 | 同一次輸入的所有連結、資料夾內所有檔案 → 合併為**一個**素材 |
| 輸入格式 | 與 `links.txt` 相同：一行一個連結，後面空一格可接密碼，`#` 開頭忽略 |
| 下載位置 | 暫存資料夾，匯入成功後自動刪除 |
| 工作保存 | 只存在記憶體；關閉程式後未完成的工作會消失 |

## 2. 使用者流程

1. 開啟「新增素材」→ 第一步有三張卡片：「選擇檔案」「選擇資料夾」「下載連結」。
2. 點「下載連結」→ 進入 `link-input` 分頁：
   - 多行輸入框（placeholder 示範格式）。
   - 輸入時即時解析（呼叫後端 `parse_download_links`），每行顯示辨識結果：
     網站名稱徽章、類型（檔案／資料夾）、是否有密碼；無法辨識的行標紅並說明。
   - 至少一行有效、沒有無效行時「下一步」才可按。
3. 之後的 BOOTH 資訊、重複確認、素材類型、填寫資訊、附加資訊分頁照舊。
   - BOOTH 分頁的「檔案或資料夾名稱」改顯示第一個連結（例：`workupload：q2hv3vcBcy 及其他 2 個項目`）。
   - 重複確認分頁的「加入到這裡」（把檔案加到既有素材）在連結模式下隱藏，只保留「直接繼續」。
   - 附加資訊分頁的「匯入後刪除原始檔案」在連結模式下隱藏（下載的暫存檔一律刪除）。
4. 按「新增素材」→ 呼叫 `request_download_import`，跳出「已加入背景下載」通知，對話框關閉並重置。
5. 狀態列右側出現「下載」圖示，有進行中的工作時顯示數量徽章；點開為工作清單 Popover。
6. 工作完成 → 通知「已新增素材！（素材名稱）」＋重新整理列表；失敗 → 通知「下載失敗（素材名稱）」。

## 3. 後端架構

### 3.1 新 crate：`crates/downloader`

依賴：`tauri`、`reqwest`（stream）、`tokio`、`serde`、`specta`、`regex`、`sha2`、`url`、`log`。
一個網站一個模組，對應 Python 版的 `sites/`。

```
crates/downloader/src/
  lib.rs            // 對外介面
  link.rs           // 連結解析：parse_lines(text) -> Vec<ParsedLine>
  site.rs           // Site trait、Job、FileItem、DownloadError
  http.rs           // save_stream（.part、續傳、進度、SHA-256）、filename_from_headers、safe_name
  browser.rs        // 隱藏 WebView2 視窗的共用工具
  sites/
    workupload.rs
    pixeldrain.rs
    gdrive.rs
    onedrive.rs
    direct.rs       // 一定排在最後
```

#### `Site` trait

```rust
#[async_trait]
trait Site: Send + Sync {
    fn name(&self) -> &'static str;           // 顯示名稱
    fn parse(&self, line: &str) -> Option<Job>;
    async fn list_files(&self, ctx: &Ctx, job: &Job) -> Result<(Option<String>, Vec<FileItem>), DownloadError>;
    async fn download(&self, ctx: &Ctx, item: &FileItem, password: &str, out_dir: &Path) -> Result<PathBuf, DownloadError>;
    fn delay(&self) -> Duration { Duration::ZERO }          // 檔案之間的暫停
    fn retry_wait(&self, attempt: u32, err: &DownloadError) -> Duration { 20s * attempt }
    fn exclusive(&self) -> bool { false }                   // true = 同一時間只跑一個工作（workupload）
}
```

- `Ctx` 帶有 `AppHandle`、進度回呼 `Fn(downloaded, total, filename)`、取消權杖。
- `DownloadError` 分為 `Fatal(String)`（不重試，例如檔案已刪除）、`PasswordRequired`、
  `WrongPassword`、`Retryable(String)`。重試邏輯與 Python 版 `retry()` 相同（最多 4 次）。

#### 各網站

| 網站 | 方式 |
|---|---|
| 直接連結 | reqwest 先讀標頭確認不是 `text/html`，取檔名與大小，再 `save_stream` |
| pixeldrain | 官方 API：`/api/list/<id>`、`/api/file/<id>/info`、`/api/file/<id>?download`，以 SHA-256 校驗；`availability` 非空時 Fatal |
| Google Drive 檔案 | `drive.usercontent.google.com/download?id=<id>&export=download&confirm=t`；若回傳 HTML 確認頁，解析表單欄位（`confirm`、`uuid`）後再送一次；配額錯誤 → Fatal |
| Google Drive 資料夾 | 解析 `drive.google.com/embeddedfolderview?id=<id>` 的檔案／子資料夾連結，遞迴列出並保留子資料夾結構；資料夾名稱取自 `<title>` |
| OneDrive（SharePoint） | 網址加 `download=1` 直接下載 |
| OneDrive（個人） | 隱藏 WebView2：開分享頁 → 依序嘗試 Python 版的下載按鈕選擇器並點擊 → 攔截下載 |
| workupload | 隱藏 WebView2，詳見 3.2 |

### 3.2 隱藏 WebView2（workupload／個人 OneDrive）

以 `WebviewWindowBuilder::new(app, "dl-<job-id>", WebviewUrl::External(url))` 建立 `visible(false)` 的視窗：

- **User-Agent** 固定為與 reqwest 相同的 Chrome UA。
- **安全性**：capabilities 只授權 `main` 視窗，外部網頁沒有任何 IPC 權限。
- **擋彈出視窗**：`on_new_window` 一律拒絕（廣告）。
- **網頁 → Rust 回報**：注入的 JS 以 `location.href = "https://konoasset.invalid/report?d=<JSON>"` 回報狀態
  （例：人機檢查完成、需要密碼、資料夾內的檔案 ID 清單、頁面標題）；
  `on_navigation` 攔截 `konoasset.invalid` 的導航、解析資料並回傳 `false` 取消導航。
- **Rust → 網頁**：`webview.eval(js)`（填密碼、按按鈕、蒐集連結）。
- **下載**：`on_download` 收到 `Requested` 時記下實際檔案網址與建議檔名，回傳 `false` 取消內建下載，
  改用 `cookies_for_url` 取得 cookie 後交給 reqwest `save_stream`（可顯示進度、可續傳）。
  若實測 workupload 下載伺服器不接受（403／回傳 HTML），改為讓 WebView2 自己下載
  （`Requested` 設定 `destination`，等 `Finished`），此時進度顯示為不確定。
- 工作結束、取消或失敗時關閉視窗。

workupload 流程（對應 Python 版 `_open_unlocked` / `download`）：

1. 開 `/file/<id>` 或 `/archive/<id>`，等待標題不再包含 `human`（最多 90 秒）。
2. 若有 `input[type=password][id^=passwordprotected_]`：沒有密碼 → `PasswordRequired`；
   有就填入並送出，送出後仍有密碼欄 → `WrongPassword`。
3. 資料夾：蒐集 `a[href^='/file/']` 的檔案 ID（去重），逐一處理。
4. 檔案：導向 `/start/<id>` 觸發下載。
5. `exclusive() = true`、`delay() = 10 秒`；連線錯誤（`ERR_CONNECTION`／`ERR_TIMED_OUT`）時等待 60／180／300 秒再重試。

### 3.3 工作管理：`tauri/src/download_job/`

```
download_job/
  mod.rs
  manager.rs     // DownloadJobManager：工作清單、排程、取消、重試
  definitions.rs // DownloadJob、DownloadJobStatus、DownloadImportRequest、事件
  commands.rs    // Tauri commands
```

- `DownloadJobManager` 以 `Arc<Mutex<_>>` 註冊為 Tauri state。不重用現有 `TaskContainer`，
  因為需要多階段狀態、進度、重試，而 `TaskContainer` 只有單純的成功／失敗。
- 排程：同時最多 3 個工作在下載；`exclusive` 網站（workupload）另有一個 1 的號誌，排隊執行。
- 每個工作的暫存資料夾：`<app_local_data_dir>/downloads/<job-id>/`；資料夾連結會在裡面建子資料夾。
- 程式啟動時清空 `downloads/`（上次未完成的工作已不存在）。

#### 工作狀態

```rust
enum DownloadJobStatus { Queued, Downloading, Importing, Completed, Failed, Cancelled }

struct DownloadJob {
    id: Uuid,
    asset_name: String,
    status: DownloadJobStatus,
    site: String,                 // 第一個連結的網站名稱
    current_file: Option<String>,
    file_index: u32,              // 第幾個檔案（1 起算）
    file_count: Option<u32>,      // 列出全部檔案後才知道
    downloaded_bytes: u64,
    total_bytes: Option<u64>,     // 目前檔案的大小，不知道時為 None
    error: Option<String>,
    error_kind: Option<DownloadErrorKind>, // PasswordRequired / WrongPassword / Other
    created_at: i64,
}
```

事件 `DownloadJobUpdated(DownloadJob)`：狀態改變時立即送出，下載進度最多每 250ms 送一次。

#### 工作流程

1. **Queued** → 取得號誌。
2. **Downloading**：依序處理每一行連結：`list_files` → 逐檔 `download`（套用 `delay` 與重試）。
   已在暫存資料夾且大小正確的檔案跳過（讓重試可以接續）。
3. **Importing**：把所有下載到的檔案（以及資料夾連結的資料夾）作為 `absolute_paths`，
   建立 `AssetImportRequest`，呼叫現有 `import_avatar` 等函式；`delete_source = true`、
   不使用資源回收筒；ZIP 解壓依目前偏好設定。匯入會等待素材資料庫的鎖，所以多個工作自然排隊。
4. **Completed**：刪除暫存資料夾。
5. **Failed**：保留暫存資料夾與 `.part` 檔，供重試續傳。
6. **Cancelled**：中止 tokio 工作、關閉 WebView2 視窗、刪除暫存資料夾。

#### 匯入函式調整

`import_wrapper.rs` 的 `import_avatar` 等函式把 `app_handle: &AppHandle` 改為 `Option<&AppHandle>`。
背景工作傳 `None`，避免全域 `ProgressEvent` 干擾前景匯入對話框的進度條；現有呼叫端傳 `Some(...)`，行為不變。

#### 縮圖

`imageFilename` 指向暫存圖片（`images/temp_*`），在匯入階段才由 `bind_temp_image` 綁定。
暫存圖片只會在程式啟動時清除，因此同一次執行期間等待中的工作不受影響。

### 3.4 Commands

| Command | 說明 |
|---|---|
| `parse_download_links(text) -> Vec<ParsedLine>` | 每行回傳 `{ line, site?, kind?, hasPassword, error? }`，給輸入分頁即時顯示 |
| `request_download_import(request) -> Uuid` | `request = { linksText, preAsset: DownloadPreAsset }`，`DownloadPreAsset` 為 `Avatar(PreAvatar)`／`AvatarWearable(...)`／`WorldObject(...)`／`OtherAsset(...)` 的 enum |
| `list_download_jobs() -> Vec<DownloadJob>` | 前端初始化時取得 |
| `cancel_download_job(id)` | 取消 |
| `retry_download_job(id, password?)` | 重試；提供 `password` 時套用到需要密碼的那一行 |
| `dismiss_download_job(id)` | 從清單移除已結束的工作（若有暫存資料夾一併刪除） |

## 4. 前端

### 4.1 新增素材對話框

- `AssetPathSelectorTab`：新增 `SelectLinkCard`，三張卡片並排（各 `w-[200px]`，容器加寬）。
- 新分頁 `LinkInputTab`（tab 值 `link-input`），總步數維持 5（連結輸入算第 1 步的延伸，標題為 `(1/5)`）。
- `AddAssetDialogContext` 新增 `downloadLinks?: string`；檔案模式與連結模式互斥，選檔案時清空連結，反之亦然。
- `useAddAssetDialog.submit`：連結模式時略過 `validatePaths`，呼叫 `requestDownloadImport`，
  成功後 toast、`setDialogOpen(false)`；失敗時 toast 錯誤並留在對話框。

### 4.2 背景工作清單

- `src/stores/DownloadJobStore`（zustand）：`jobs: DownloadJob[]`；初始化時呼叫 `listDownloadJobs`，
  監聽 `downloadJobUpdated` 更新；偵測狀態變成 `Completed`／`Failed` 時發 toast，
  `Completed` 時呼叫 `refreshAssetSummaries` 與 `refreshFilteredIds`。
- `src/components/models/download-jobs/DownloadJobsButton`：放在 `StatusBar` 右側；
  無工作時隱藏；進行中數量顯示為徽章。
- `DownloadJobsPopover`：每個工作一列：素材名稱、網站、階段、進度條（未知大小時為不確定動畫）、
  `目前檔名 (2/5)`、錯誤訊息；按鈕：取消（進行中）、重試／移除（失敗）、移除（完成）。
  `PasswordRequired`／`WrongPassword` 時，重試按鈕旁出現密碼輸入框。

### 4.3 語系

所有新字串加入 `ja-JP`、`en-US`、`en-GB`、`zh-CN`、`zh-TW`（台灣用語為主），key 以 `addasset:link-input:*`、`download-jobs:*` 命名。
`changelog.json` 新增條目時必須包含 zh-TW。

## 5. 錯誤處理

| 情況 | 行為 |
|---|---|
| 無法辨識的連結 | 輸入分頁標紅，無法進下一步 |
| 檔案已刪除／404／需要登入 | Fatal，工作失敗並顯示原因 |
| 需要密碼／密碼錯誤 | 工作失敗，Popover 顯示密碼輸入框，重試時套用 |
| 網路暫時錯誤、大小不符 | 依網站規則等待後重試，最多 4 次 |
| pixeldrain 限制下載、Google Drive 配額 | Fatal，訊息提示改用瀏覽器手動下載並附上網址 |
| 匯入失敗 | 工作失敗（保留已下載檔案），重試時直接重新匯入 |
| 下載中關閉程式 | 工作消失；下次啟動清空暫存資料夾 |

## 6. 測試

- **Rust 單元測試**（`crates/downloader`）：
  - 連結解析：每個網站的各種網址格式、密碼、註解行、無效行（對照 Python 版的正規表示式）。
  - `filename_from_headers`（`filename*=UTF-8''`、引號、latin-1 → UTF-8）、`safe_name`。
  - Google Drive 確認頁表單、`embeddedfolderview` 解析（使用 HTML fixture）。
  - pixeldrain JSON 解析。
  - `save_stream` 續傳與大小檢查（以本機 HTTP 測試伺服器）。
- **Rust 單元測試**（`download_job`）：狀態轉換、取消時刪除資料夾、重試時跳過已完成檔案。
- **前端 vitest**：`LinkInputTab` 依解析結果啟用／停用下一步；`DownloadJobStore` 事件處理與 toast。
- **手動端對端**：以 `links.txt` 的實際連結測試 workupload 單一檔案＋密碼、workupload 資料夾＋密碼、
  pixeldrain 檔案；確認背景匯入成功、能同時排入第二個素材、取消與重試正常。

## 7. 不在範圍內

- 工作在重開程式後繼續（持久化佇列）。
- 拖放或深層連結（deep link）直接帶入下載連結。
- 「加入到這裡」（把下載的檔案加到既有素材）。
- 其他網站（gofile、MEGA 等）；架構上新增一個 `sites/*.rs` 並登記即可擴充。
