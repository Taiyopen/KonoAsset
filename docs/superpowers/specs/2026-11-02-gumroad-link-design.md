# Gumroad 商品連結支援 — 設計規格

- 日期：2026-11-02
- 狀態：待審查
- 範圍：新增／編輯素材時可貼上 Gumroad 商品連結，自動填入素材資訊，並把連結存進素材資料。

## 1. 目的

目前 KonoAsset 只認得 BOOTH 連結。不少 VRChat 素材在 Gumroad 販售，使用者只能手動輸入名稱、作者、縮圖，
之後也沒辦法從素材回到商品頁。本功能讓 Gumroad 連結享有與 BOOTH 連結相同的體驗。

### 成功條件

- 在「商品連結」分頁貼上 Gumroad 商品網址，按下取得資訊後自動填入名稱、作者、發布日期與縮圖。
- 連結存進素材；素材卡片選單出現「在 Gumroad 開啟」。
- 同一個 Gumroad 商品重複新增時，跳出與 BOOTH 相同的重複確認。
- 一個素材可以同時有 BOOTH 連結與 Gumroad 連結。
- 舊資料不需轉換即可正常讀取；原版 KonoAsset 讀取本 fork 寫出的資料也不會出錯（只是看不到 Gumroad 連結）。
- 貼 BOOTH 連結時，現有流程完全不變。

### 已確定的決策

| 項目 | 決定 |
|---|---|
| 連結數量 | BOOTH 與 Gumroad 各存一個欄位，可同時存在 |
| 輸入方式 | 共用一個網址欄，自動判斷平台（沿用原本的 BOOTH 輸入分頁） |
| 存放內容 | Gumroad 回傳的正式網址（JSON 的 `url` 欄位），不是使用者貼上的原文 |
| 資料版本 | 不提高版本號；新欄位用 `#[serde(default)]` |
| 素材類型 | Gumroad 沒有分類，不推測；維持表單目前的值 |

## 2. 資料來源（已實測）

在商品網址後加 `.json` 即回傳公開 JSON，不需登入：

```
GET https://bananabread.gumroad.com/l/ajfrt.json
```

使用的欄位：

| KonoAsset | Gumroad JSON |
|---|---|
| 正式網址 | `url` |
| 名稱 | `name` |
| 作者 | `seller.name` |
| 圖片 | `covers[].url`（`type == "image"`），沒有時退回 `thumbnail_url` |
| 發布日期 | `created_at`（ISO 8601） |

實測結果：

- `https://gumroad.com/l/ajfrt` 會轉址到 `https://bananabread.gumroad.com/l/ajfrt`，`.json` 版本亦同。
- `https://otherseller.gumroad.com/l/ajfrt.json` 回 404 → 商品代碼不保證跨賣家唯一，必須存完整網址。
- 網址帶查詢參數（例：`?layout=profile`）也能開啟；取 JSON 前要去掉。

風險：此端點不是 Gumroad 公開文件記載的 API，日後可能變動。變動時只會讓「取得資訊」失敗，
已存的連結與其他功能不受影響。

## 3. 後端

### 3.1 資料格式

`crates/model/src/base.rs`：

```rust
pub struct AssetDescription {
    // ...既有欄位
    pub booth_item_id: Option<u64>,
    #[serde(default)]
    pub gumroad_url: Option<String>,
    // ...
}
```

- 資料版本號維持 3。舊資料缺欄位 → `None`。
- `AssetSummary`（`crates/model/src/summary.rs`）加 `gumroad_url`。
- `crates/loader/src/share.rs` 的舊版轉換（V1/V2 → 現行）設為 `None`。
- 其他建立 `AssetDescription` 的地方（storage 測試、importer、download_job 等）補上欄位；
  非測試程式一律 `None`，除非該處是從表單接收資料。

### 3.2 新 crate：`crates/gumroad`

```
crates/gumroad/
  Cargo.toml
  src/lib.rs          // 對外介面
  src/url.rs          // 網址檢查與轉換
  src/fetcher.rs      // GumroadFetcher：取得並解析 JSON
  src/definitions.rs  // GumroadAssetInfo、JSON schema
  src/error.rs
  test/ajfrt.json     // 精簡後的實際回應（移除 description_html 等不用的欄位）
```

- `url.rs`
  - `is_gumroad_url(url) -> bool`：接受 `https://<賣家>.gumroad.com/l/<代碼>` 與 `https://gumroad.com/l/<代碼>`，
    允許結尾 `/` 與查詢參數。
  - `to_json_url(url) -> Result<String, _>`：去掉查詢參數與結尾 `/`，加上 `.json`。
- `GumroadAssetInfo`（`specta::Type`，傳給前端）：
  ```rust
  pub struct GumroadAssetInfo {
      pub url: String,           // 正式網址
      pub name: String,
      pub creator: String,
      pub image_urls: Vec<String>,
      pub published_at: i64,     // 毫秒，與 BoothAssetInfo 相同單位
  }
  ```
- HTTP client 與 User-Agent 比照 `crates/booth/src/client.rs`。不做快取。

### 3.3 圖片

沿用 `PximgResolver`（下載、縮圖、轉 JPEG）。`validate_url` 的允許網域由只有 `booth.pximg.net`
改為 `booth.pximg.net` 或 `public-files.gumroad.com`。

### 3.4 Tauri 指令

- `get_asset_info_from_gumroad(url: String) -> Result<GumroadAssetInfo, String>`
  （`src-backend/tauri/src/command/external/gumroad.rs`）
- `get_asset_displays_by_gumroad_url(url: String) -> Result<Vec<AssetSummary>, String>`
  （`command/asset/get.rs`，與 `get_asset_displays_by_booth_id` 並列，比對四種素材）
- `GumroadFetcher` 註冊為 Tauri state。因為不做快取、`fetch` 只需 `&self`，不必像 `BoothFetcher` 包 `Mutex`。

### 3.5 搜尋與匯出

- 搜尋（`crates/storage/src/search/logic.rs`）：關鍵字也比對 `gumroad_url`。
- 資料夾 zip 匯出（`human_readable_zip_exporter.rs`）：有 Gumroad 連結時多寫一個 `GUMROAD.url`，格式與 `BOOTH.url` 相同。

## 4. 前端

### 4.1 網址判斷

`src/lib/utils.ts` 加 `isGumroadURL(url)`，規則與後端 `is_gumroad_url` 一致。最終以後端檢查為準。

### 4.2 商品連結分頁（新增與編輯）

元件與檔名維持 `BoothInputTab`，避免無關的改名。

- 標題、說明、placeholder 改成同時提到 BOOTH 與 Gumroad。
- 輸入時：
  - 符合 BOOTH → 走現有流程。
  - 符合 Gumroad → 「取得資訊」按鈕可按，走新流程。
  - 都不符合 → 按鈕停用（與現在相同）。
- 新流程 `getAndSetAssetInfoFromGumroadToForm`（放在 `BoothInputTab/logic/index.ts`，與 BOOTH 版並列）：
  1. 呼叫 `getAssetInfoFromGumroad(url)`。
  2. 設定 `name`、`creator`、`publishedAt`、`gumroadUrl`（存回傳的正式網址）。不動 `assetType` 與 `boothItemId`。
  3. 有圖片時 `setImageUrls`，並以第一張呼叫 `resolvePximgFilename` 設定 `imageFilename`。
  4. 呼叫 `getAssetDisplaysByGumroadUrl(正式網址)` 做重複檢查，回傳格式與 BOOTH 版相同。
- 編輯畫面貼 Gumroad 連結會覆蓋名稱等資訊並設定 `gumroadUrl`，既有的 `boothItemId` 保留。

### 4.3 表單與送出

- `src/lib/form.ts` 加 `gumroadUrl: string | null`，預設 `null`。
- 新增／編輯對話框送出時把 `gumroadUrl` 帶進 `AssetDescription`；編輯時從既有素材讀入。

### 4.4 素材卡片

- `AssetCard` 與 `AssetListView` 把 `asset.gumroadUrl` 傳給 `AssetCardMeatballMenu`。
- 有值時選單多一項「在 Gumroad 開啟」，用與 BOOTH 相同的方式以外部瀏覽器開啟。

### 4.5 文字

- 五個語系（zh-TW、zh-CN、ja-JP、en-US、en-GB）都補上新文字，以 zh-TW 為主、使用台灣用語。
- 取得資訊失敗的提示文字改成不寫死 BOOTH。
- `changelog.json` 加一筆，含 zh-TW。

## 5. 錯誤處理

| 情況 | 行為 |
|---|---|
| 網址格式不符 | 前端按鈕停用；後端仍會檢查並回錯誤 |
| 商品不存在（404）、網路錯誤、JSON 格式變動 | 顯示「取得資訊失敗」提示，表單不變 |
| 圖片下載失敗 | 其他欄位照常填入，只是沒有縮圖（與 BOOTH 相同） |

## 6. 測試

- Rust
  - `url.rs`：各種合法／不合法網址、`to_json_url` 去掉查詢參數與結尾 `/`。
  - `fetcher.rs`：用 `test/ajfrt.json` 解析出正確欄位；`covers` 為空時退回 `thumbnail_url`；缺必要欄位時回錯誤。
  - `pximg.rs`：允許 `public-files.gumroad.com`，仍拒絕其他網域。
  - 搜尋：以 Gumroad 網址片段可搜到素材。
  - 舊資料（沒有 `gumroadUrl`）可正常讀取。
- 前端（vitest）
  - `isGumroadURL`。
  - 分頁 hook：BOOTH／Gumroad 網址各自呼叫對的指令；Gumroad 流程設定 `gumroadUrl` 且不動 `boothItemId`。

## 7. 不在範圍內

- AvatarExplorer 格式匯出（該格式只有 BOOTH ID 欄位）
- deep link
- 用 Gumroad 商品說明做 AI 欄位建議
- 賣家自訂網域
- 刪除已存的連結（BOOTH 目前也不能刪）
- Gumroad 資料快取
