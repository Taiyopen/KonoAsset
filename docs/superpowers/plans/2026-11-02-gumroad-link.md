# Gumroad 商品連結支援 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增／編輯素材時可貼 Gumroad 商品連結自動填入資訊，並把正式網址存進素材資料。

**Architecture:** `AssetDescription` 加 `gumroad_url: Option<String>`（`#[serde(default)]`，不升版號）。新 crate `crates/gumroad` 取得並解析 `<商品網址>.json`；Tauri 新增兩個指令（取得資訊、依網址找重複素材）。前端沿用 `BoothInputTab`，依網址自動分流到 BOOTH 或 Gumroad 流程；卡片選單多「在 Gumroad 開啟」。

**Tech Stack:** Rust（tauri 2、reqwest 0.13、serde、specta rc.22、regex、chrono、thiserror）、React + react-hook-form + zod、vitest。

**Spec:** `docs/superpowers/specs/2026-11-02-gumroad-link-design.md`

## Global Constraints

- 在 `main` 分支開發（追蹤 `origin/main`），每個 task 完成就 commit；commit 訊息結尾加 `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`。
- 不要 commit 與本功能無關的 `src/routeTree.gen.ts` 改動。
- 資料版本號維持 3；`gumroad_url` 一律 `#[serde(default)]`，缺欄位 = `None`。
- 存的值是 Gumroad JSON 的 `url` 欄位（正式網址），不是使用者貼上的原文。
- 接受的網址：`https://<賣家>.gumroad.com/l/<代碼>`、`https://gumroad.com/l/<代碼>`，可帶結尾 `/` 與 `?查詢參數`；只接受 `https`。
- 圖片允許網域：`booth.pximg.net`、`public-files.gumroad.com`，其他一律拒絕。
- `published_at` 單位為毫秒（與 `BoothAssetInfo` 相同）。
- 不推測素材類型；Gumroad 流程不得改動 `assetType` 與 `boothItemId`。
- 語系：zh-TW、zh-CN、ja-JP、en-US、en-GB 全部補齊，zh-TW 用台灣用語。
- 元件與檔名維持 `BoothInputTab`，不做無關改名。
- `src/lib/bindings.ts` 由 debug 執行時自動產生；task 內先手動照既有格式更新，Task 7 以 `pnpm tauri dev` 重新產生並確認無差異。
- Rust 指令在 `src-backend/` 下執行；前端指令在專案根目錄執行。

## Review Focus

1. 同一商品以不同格式貼上（`gumroad.com/l/ajfrt` vs `bananabread.gumroad.com/l/ajfrt/?x=1`）→ 仍要偵測到重複。重複檢查必須用取得結果的 `url`，不是輸入字串（Task 5 測試）。
2. 編輯已有 `boothItemId` 的素材後以 Gumroad 覆寫 → `boothItemId` 保留（Task 5 測試）。
3. 貼賣家首頁 `https://bananabread.gumroad.com/` 或 `http://` 網址 → 不視為 Gumroad 商品，按鈕停用（Task 2、Task 5 測試）。
4. 商品沒有 `covers` 也沒有 `thumbnail_url` → 回傳空圖片清單，不報錯（Task 2 測試）。
5. 舊資料 JSON 沒有 `gumroadUrl` → 正常讀取為 `None`（Task 1 測試）。

---

### Task 1: `gumroad_url` 欄位貫通（資料格式＋表單）

**Files:**
- Modify: `src-backend/crates/model/src/base.rs`（`AssetDescription`）
- Modify: `src-backend/crates/model/src/summary.rs`（`AssetSummary` 與四個 `From`）
- Modify: `src-backend/crates/loader/src/share.rs`（V1/V2 → 現行轉換設 `None`）
- Modify: 其他所有建構 `AssetDescription { .. }` 的地方（用 `cargo build --tests` 的錯誤清單逐一補 `gumroad_url: None`；已知：`loader/src/{avatar,avatar_wearables,world_objects}.rs`、`storage/src/{asset_storage,delete,json_asset_container}.rs`、`storage/src/search/logic.rs`、`tauri/src/adapter/exporter/avatar_explorer/builder.rs`、`tauri/src/importer/import_wrapper.rs`、`tauri/src/download_job/*`、`tauri/src/command/external/ai_suggest.rs`）
- Modify: `src/lib/bindings.ts`（`AssetDescription`、`AssetSummary` 型別）
- Modify: `src/lib/form.ts`、`src/components/model-legacy/asset-dialogs/AddAssetDialog/hook/index.ts`（zod schema、預設值、送出）、`src/components/model-legacy/asset-dialogs/EditAssetDialog/hook/index.tsx`（schema、預設值、從 description 讀入）、`src/components/model-legacy/asset-dialogs/EditAssetDialog/logic/index.ts`（四個 update 函式）
- Modify: 因型別變更而無法編譯的前端測試／stories 物件字面值（補 `gumroadUrl: null`）
- Test: `src-backend/crates/model/src/base.rs`（新增 `#[cfg(test)]`）

**Interfaces:**
- Produces: Rust `AssetDescription.gumroad_url: Option<String>`、`AssetSummary.gumroad_url: Option<String>`；TS `AssetDescription.gumroadUrl: string | null`、`AssetSummary.gumroadUrl: string | null`、表單欄位 `gumroadUrl: string | null`（預設 `null`）。

- [ ] **Step 1: 寫失敗測試** `test_description_without_gumroad_url_deserializes`：以既有 camelCase JSON（含 `name`、`creator`、`imageFilename: null`、`tags: []`、`memo: null`、`boothItemId: 1`、`dependencies: []`、`createdAt: 0`、`publishedAt: null`，沒有 `gumroadUrl`）反序列化，斷言 `gumroad_url == None` 且 `booth_item_id == Some(1)`；另一個 `test_description_with_gumroad_url_roundtrips`：序列化含 `Some("https://bananabread.gumroad.com/l/ajfrt")` 的值，輸出含 `"gumroadUrl":"https://bananabread.gumroad.com/l/ajfrt"`，再讀回相等。
- [ ] **Step 2: 執行確認失敗** — `cargo test -p model`，預期編譯失敗（沒有 `gumroad_url` 欄位）。
- [ ] **Step 3: 實作** — 在 `booth_item_id` 後加 `#[serde(default)] pub gumroad_url: Option<String>`；`AssetSummary` 同位置加欄位並在四個 `From` 帶入；修正所有建構處。
- [ ] **Step 4: 執行確認通過** — `cargo test --workspace`，預期全部 PASS。
- [ ] **Step 5: 前端貫通** — 更新 `bindings.ts` 兩個型別（`gumroadUrl: string | null` 放在 `boothItemId` 後）；`form.ts` 加欄位；兩個對話框 zod 加 `gumroadUrl: z.string().nullable()`、預設 `null`；新增送出時帶 `form.getValues('gumroadUrl') ?? null`；編輯讀入 `form.setValue('gumroadUrl', description.gumroadUrl ?? null)`；四個 update 函式帶 `gumroadUrl`。
- [ ] **Step 6: 驗證** — `pnpm type-check` 無錯、`pnpm test:run` 全 PASS。
- [ ] **Step 7: Commit** — `feat: store Gumroad URL in asset description`

---

### Task 2: `crates/gumroad`（網址判斷＋取得資訊）

**Files:**
- Create: `src-backend/crates/gumroad/Cargo.toml`（依賴比照 `crates/booth/Cargo.toml`，去掉 `model`、`file`、`uuid`、`tokio`，加 `regex.workspace = true`；`[dev-dependencies] tokio`）
- Create: `src-backend/crates/gumroad/src/{lib.rs,url.rs,fetcher.rs,definitions.rs,error.rs}`
- Create: `src-backend/crates/gumroad/test/ajfrt.json`（取實際回應，只留 `url`、`name`、`seller.name`、`thumbnail_url`、`created_at` 與前 2 筆 `covers` 的 `type`、`url`）
- Modify: `src-backend/Cargo.toml`（`[workspace.dependencies]` 加 `gumroad = { path = "./crates/gumroad", version = "0.0.0" }`）

**Interfaces:**
- Produces:
  - `pub fn is_gumroad_url(url: &str) -> bool`
  - `pub fn to_json_url(url: &str) -> Result<String, GumroadInfoFetchError>`（例：`https://bananabread.gumroad.com/l/ajfrt/?layout=profile` → `https://bananabread.gumroad.com/l/ajfrt.json`）
  - `#[derive(Serialize, Clone, Debug, PartialEq, Eq, specta::Type)] #[serde(rename_all = "camelCase")] pub struct GumroadAssetInfo { pub url: String, pub name: String, pub creator: String, pub image_urls: Vec<String>, pub published_at: i64 }`
  - `pub struct GumroadFetcher`；`GumroadFetcher::new<S: AsRef<str>>(version: S) -> Self`；`pub async fn fetch(&self, url: &str) -> Result<GumroadAssetInfo, GumroadInfoFetchError>`
  - `pub enum GumroadInfoFetchError { InvalidUrl(String), NotFound(String), APICallError(reqwest::Error), JSONParseError(serde_json::Error), DateParseError(chrono::ParseError) }`（thiserror，訊息比照 `BoothInfoFetchError`）

- [ ] **Step 1: 寫失敗測試（`url.rs`）** — `test_is_gumroad_url`：以下為 `true`：`https://bananabread.gumroad.com/l/ajfrt`、`https://gumroad.com/l/ajfrt`、`https://bananabread.gumroad.com/l/ajfrt/`、`https://bananabread.gumroad.com/l/ajfrt?layout=profile`、`https://seller-name.gumroad.com/l/My_Item-2`；以下為 `false`：`http://bananabread.gumroad.com/l/ajfrt`、`https://bananabread.gumroad.com/`、`https://bananabread.gumroad.com/l/`、`https://evilgumroad.com/l/ajfrt`、`https://gumroad.com.evil.com/l/ajfrt`、`https://booth.pm/ja/items/6641548`。`test_to_json_url`：上面四個合法形式分別轉成 `https://bananabread.gumroad.com/l/ajfrt.json` 或 `https://gumroad.com/l/ajfrt.json`；不合法輸入回 `Err(InvalidUrl)`。
- [ ] **Step 2: 寫失敗測試（`fetcher.rs`）** — `test_parse_fixture`：解析 `test/ajfrt.json` 得到 `url == "https://bananabread.gumroad.com/l/ajfrt"`、`name == "Midnight Studio - VRChat World PC & Quest"`、`creator == "Banana's Assets for VRChat"`、`image_urls[0] == "https://public-files.gumroad.com/3padttruejf2sb7rqp2w2hh0o7vx"`、`image_urls.len() == 2`、`published_at == 1687351822000`。`test_parse_falls_back_to_thumbnail`：`covers: []`、`thumbnail_url: "https://public-files.gumroad.com/x"` → `image_urls == ["https://public-files.gumroad.com/x"]`。`test_parse_no_images`：無 `covers`、`thumbnail_url: null` → `image_urls` 為空。`test_parse_skips_non_image_covers`：`covers` 含 `{"type":"video","url":"..."}` 與一筆 image → 只留 image。`test_parse_missing_name_fails`：缺 `name` → `Err(JSONParseError)`。
- [ ] **Step 3: 執行確認失敗** — `cargo test -p gumroad`，預期編譯失敗。
- [ ] **Step 4: 實作** — 正規表示式：`^https://(?:([0-9a-z-]+)\.)?gumroad\.com/l/([0-9A-Za-z_-]+)/?(?:\?.*)?$`，`to_json_url` 用擷取群組重組 `https://{host}/l/{permalink}.json`。解析用 `fn parse_response(body: &str) -> Result<GumroadAssetInfo, GumroadInfoFetchError>`，`covers`、`thumbnail_url` 皆 `#[serde(default)]`；`created_at` 用 `DateTime::parse_from_rfc3339(..).timestamp_millis()`。`fetch`：先 `to_json_url`，404 → `NotFound(url)`，其餘交給 `parse_response`。HTTP client：`KonoAsset/{version}` User-Agent、5 秒逾時（同 `booth/src/client.rs`），預設會跟隨轉址。
- [ ] **Step 5: 執行確認通過** — `cargo test -p gumroad`，全部 PASS。
- [ ] **Step 6: Commit** — `feat: add gumroad crate for fetching product info`

---

### Task 3: 圖片網域與 Tauri 指令

**Files:**
- Modify: `src-backend/crates/booth/src/pximg.rs`（`validate_url` 與 `test_validate_url`）
- Create: `src-backend/tauri/src/command/external/gumroad.rs`
- Modify: `src-backend/tauri/src/command/external/mod.rs`、`src-backend/tauri/src/command/mod.rs`、`src-backend/tauri/src/command/asset/get.rs`、`src-backend/tauri/src/lib.rs`、`src-backend/tauri/Cargo.toml`
- Modify: `src/lib/bindings.ts`

**Interfaces:**
- Consumes: Task 2 的 `GumroadFetcher`、`GumroadAssetInfo`。
- Produces（TS）：`commands.getAssetInfoFromGumroad(url: string): Promise<Result<GumroadAssetInfo, string>>`、`commands.getAssetDisplaysByGumroadUrl(url: string): Promise<Result<AssetSummary[], string>>`、型別 `GumroadAssetInfo = { url: string; name: string; creator: string; imageUrls: string[]; publishedAt: number }`。

- [ ] **Step 1: 寫失敗測試** — 在 `test_validate_url` 加：`https://public-files.gumroad.com/3padttruejf2sb7rqp2w2hh0o7vx` → `Ok`；`https://evil.gumroad.com/x`、`https://public-files.gumroad.com.evil.com/x`、`http://public-files.gumroad.com/x` → `Err`。
- [ ] **Step 2: 執行確認失敗** — `cargo test -p booth validate_url`，預期 FAIL。
- [ ] **Step 3: 實作** — `validate_url` 改為網域屬於 `["booth.pximg.net", "public-files.gumroad.com"]`（完全相等比對）。
- [ ] **Step 4: 執行確認通過** — `cargo test -p booth`，PASS。
- [ ] **Step 5: 加指令** — `get_asset_info_from_gumroad(gumroad_fetcher: State<'_, GumroadFetcher>, url: String) -> Result<GumroadAssetInfo, String>`，log 寫法比照 `get_asset_info_from_booth`；`get_asset_displays_by_gumroad_url(basic_store, url: String) -> Result<Vec<AssetSummary>, String>`，結構比照 `get_asset_displays_by_booth_id`，條件為 `asset.description.gumroad_url.as_deref() == Some(url.as_str())`。`fetch` 只用 `&self`，`GumroadFetcher` 直接 `.manage(GumroadFetcher::new(VERSION))`，不包 `Mutex`。兩個指令註冊在 `collect_commands!`（分別接在 booth 對應指令後）。
- [ ] **Step 6: 更新 `bindings.ts`** — 照既有格式新增兩個 command 與 `GumroadAssetInfo` 型別。
- [ ] **Step 7: 驗證** — `cargo build -p konoasset` 成功、`cargo test --workspace` PASS、`pnpm type-check` 無錯。
- [ ] **Step 8: Commit** — `feat: add Tauri commands for Gumroad product info`

---

### Task 4: 搜尋與資料夾 zip 匯出

**Files:**
- Modify: `src-backend/crates/storage/src/search/logic.rs`
- Modify: `src-backend/tauri/src/adapter/exporter/human_readable_zip_exporter.rs`

**Interfaces:**
- Consumes: Task 1 的 `gumroad_url`。

- [ ] **Step 1: 寫失敗測試** — `test_check_text_contains_gumroad_url`：description 的 `gumroad_url = Some("https://bananabread.gumroad.com/l/ajfrt")`，`check_text_contains(&description, &vec!["ajfrt"])` 為 `true`、`vec!["zzzz"]` 為 `false`。匯出：擴充 `test_human_readable_export_fn` 的測試資料，讓其中一個素材有 `gumroad_url`，斷言 zip 內有 `<該素材路徑>GUMROAD.url` 且內容含 `URL=https://bananabread.gumroad.com/l/ajfrt`；沒有 `gumroad_url` 的素材沒有此檔。
- [ ] **Step 2: 執行確認失敗** — `cargo test -p storage check_text_contains`、`cargo test -p konoasset human_readable`，預期 FAIL。
- [ ] **Step 3: 實作** — 搜尋：在 `booth_item_id` 判斷後以 `||` 接 `gumroad_url` 的 `to_ascii_lowercase()` 包含判斷。匯出：`create_link_file_data(booth_id: u64)` 改為 `create_link_file_data(url: &str)`，BOOTH 呼叫端傳 `format!("https://booth.pm/ja/items/{}/", id)`；兩個寫檔處各加一段 `GUMROAD.url`。
- [ ] **Step 4: 執行確認通過** — 同 Step 2 指令，PASS。
- [ ] **Step 5: Commit** — `feat: search and export Gumroad links`

---

### Task 5: 商品連結分頁的 Gumroad 流程

**Files:**
- Modify: `src/lib/utils.ts`、`src/lib/utils.test.ts`
- Modify: `src/components/model-legacy/asset-dialogs/components/tabs/BoothInputTab/logic/index.ts`、`logic/index.test.ts`
- Modify: `BoothInputTab/add/hook/index.ts`、`add/hook/index.test.ts`、`add/index.tsx`
- Modify: `BoothInputTab/edit/hook/index.ts`、`edit/index.tsx`

**Interfaces:**
- Consumes: Task 3 的兩個 command；Task 1 的表單欄位 `gumroadUrl`。
- Produces:
  - `export function isGumroadURL(url: string): boolean`（規則同 Task 2 的正規表示式）
  - `export const getAndSetAssetInfoFromGumroadToForm = async ({ gumroadUrl, form, setImageUrls }: { gumroadUrl: string; form: AssetFormType; setImageUrls: (imageUrls: string[]) => void }): Promise<Result<ReturnProps, string>>`（`ReturnProps` 與 BOOTH 版共用）
  - 兩個 hook 新增回傳 `canFetch: boolean`（BOOTH 或 Gumroad 網址皆為 `true`），view 的按鈕改用 `disabled={fetching || !canFetch}`。

- [ ] **Step 1: 寫失敗測試（utils）** — `isGumroadURL` 用 Task 2 Step 1 的同一組 true／false 網址。
- [ ] **Step 2: 寫失敗測試（logic）** — mock `commands.getAssetInfoFromGumroad` 回傳 `{ url: 'https://bananabread.gumroad.com/l/ajfrt', name: 'N', creator: 'C', imageUrls: ['https://public-files.gumroad.com/a'], publishedAt: 1687351822000 }`：
  - `fills form and stores canonical url`：以 `gumroadUrl: 'https://gumroad.com/l/ajfrt?x=1'` 呼叫後，表單 `name == 'N'`、`creator == 'C'`、`publishedAt == 1687351822000`、`gumroadUrl == 'https://bananabread.gumroad.com/l/ajfrt'`；`resolvePximgFilename` 以 `'https://public-files.gumroad.com/a'` 呼叫；`getAssetDisplaysByGumroadUrl` 以 `'https://bananabread.gumroad.com/l/ajfrt'` 呼叫（不是輸入字串）。
  - `keeps boothItemId and assetType`：預先設 `boothItemId = 6641548`、`assetType = 'WorldObject'`，呼叫後兩者不變。
  - `returns duplicated`：重複檢查回傳一筆 → `{ goNext: false, duplicated: true }`。
  - `returns error`：`getAssetInfoFromGumroad` 回 error → 回傳 error，且 `name` 未被改。
- [ ] **Step 3: 寫失敗測試（add hook）** — 輸入 Gumroad 網址後 `canFetch === true`，觸發取得時呼叫 `getAssetInfoFromGumroad` 而非 `getAssetInfoFromBooth`；輸入 BOOTH 網址時呼叫 `getAssetInfoFromBooth`；輸入 `https://bananabread.gumroad.com/` 時 `canFetch === false`。
- [ ] **Step 4: 執行確認失敗** — `pnpm vitest run src/lib/utils.test.ts src/components/model-legacy/asset-dialogs/components/tabs/BoothInputTab`，預期 FAIL。
- [ ] **Step 5: 實作** — `utils.ts` 加 `isGumroadURL`；`logic/index.ts` 加 `getAndSetAssetInfoFromGumroadToForm`（步驟見 spec §4.2）；兩個 hook 在取得時依 `isGumroadURL(input)` 分流（BOOTH 走原流程），失敗時沿用 `addasset:booth-input:failed-to-get-info` toast；view 改用 `canFetch`，placeholder 改為 `https://booth.pm/ja/items/6641548 / https://xxx.gumroad.com/l/xxxx`。
- [ ] **Step 6: 執行確認通過** — 同 Step 4 指令 PASS，並跑 `pnpm type-check`、`pnpm lint`。
- [ ] **Step 7: Commit** — `feat: fetch asset info from Gumroad links`

---

### Task 6: 卡片選單「在 Gumroad 開啟」

**Files:**
- Modify: `src/components/models/asset-card/AssetCardMeatballMenu/AssetCardMeatballMenu.tsx`
- Modify: `src/components/models/asset-card/AssetCardMeatballMenu/internal/InternalAssetCardMeatballMenu.tsx`、`InternalAssetCardMeatballMenu.stories.tsx`
- Modify: `src/components/model-legacy/AssetCard/index.tsx`、`src/components/model-legacy/asset-view/AssetListView/index.tsx`
- Create: `src/components/models/asset-card/AssetCardMeatballMenu/internal/InternalAssetCardMeatballMenu.test.tsx`

**Interfaces:**
- Consumes: Task 1 的 `AssetSummary.gumroadUrl`。
- Produces: `AssetCardMeatballMenu` 與 `InternalAssetCardMeatballMenu` 新增 prop `gumroadUrl?: string`。

- [ ] **Step 1: 寫失敗測試** — `gumroadUrl='https://bananabread.gumroad.com/l/ajfrt'` 時，選單有文字為 `assetcard:more-button:open-gumroad` 的連結，`href` 為該網址、`target="_blank"`；未提供時不顯示此項。
- [ ] **Step 2: 執行確認失敗** — `pnpm vitest run src/components/models/asset-card/AssetCardMeatballMenu`，FAIL。
- [ ] **Step 3: 實作** — 在「在 BOOTH 開啟」之後加一個 `DropdownMenuItem asChild` 的 `<a>`，只在有值時渲染（不需 tooltip）；`AssetCard`、`AssetListView` 傳 `gumroadUrl={asset.gumroadUrl ?? undefined}`；story 加一個有 Gumroad 連結的案例。
- [ ] **Step 4: 執行確認通過** — 同 Step 2 指令 PASS。
- [ ] **Step 5: Commit** — `feat: open Gumroad link from asset card menu`

---

### Task 7: 文字、changelog 與整體驗證

**Files:**
- Modify: `src/locales/{zh-TW,zh-CN,ja-JP,en-US,en-GB}.json`
- Modify: `changelog.json`
- Modify（若重新產生有差異）: `src/lib/bindings.ts`

- [ ] **Step 1: 文字** — 新增 `assetcard:more-button:open-gumroad`，並把下列既有鍵改成同時提到 BOOTH 與 Gumroad。zh-TW 定稿如下，其他語系依意思翻譯：

  | 鍵 | zh-TW |
  |---|---|
  | `addasset:booth-input:explanation-text` | 輸入 BOOTH 或 Gumroad 商品連結，即可自動填入素材資訊！ |
  | `addasset:booth-input:get-info` | 從商品連結取得資訊 |
  | `addasset:booth-edit` | 編輯商品資訊 |
  | `addasset:booth-edit:explanation-text` | 請從商品連結取得資訊並覆寫，或前往下一個分頁 |
  | `addasset:booth-edit:overwrite` | 以商品連結的資訊覆寫 |
  | `assetcard:more-button:open-gumroad` | 在 Gumroad 開啟 |

- [ ] **Step 2: changelog** — 頂端 1.2.7 已發布，新增 `"version": "1.2.8"` 項目於最前，`features` 一筆：英文 `Gumroad product links can now be used to fill in asset info automatically, and are saved with the asset`；ja-JP 與 zh-TW（`現在可貼上 Gumroad 商品連結自動填入素材資訊，連結也會存進素材`）。不改 `package.json` 版本。
- [ ] **Step 3: 全部測試** — `cargo test --workspace`（在 `src-backend/`）、`pnpm test:run`、`pnpm type-check`、`pnpm lint`，全部通過。
- [ ] **Step 4: 重新產生 bindings** — `pnpm tauri dev` 啟動後關閉，`git diff src/lib/bindings.ts` 應無差異；有差異就以產生結果為準並重跑 Step 3。
- [ ] **Step 5: 實機確認**（`pnpm tauri dev`）—
  1. 新增素材 → 貼 `https://gumroad.com/l/ajfrt` → 取得資訊 → 名稱、作者、縮圖、日期正確 → 存檔。
  2. 卡片選單「在 Gumroad 開啟」開到 `https://bananabread.gumroad.com/l/ajfrt`。
  3. 再新增一次並貼 `https://bananabread.gumroad.com/l/ajfrt/?layout=profile` → 出現重複確認。
  4. 貼 BOOTH 連結 → 流程與之前相同。
  5. 搜尋 `ajfrt` 找得到該素材；匯出資料夾 zip 內有 `GUMROAD.url`。
- [ ] **Step 6: Commit** — `feat: add Gumroad link strings and changelog`
