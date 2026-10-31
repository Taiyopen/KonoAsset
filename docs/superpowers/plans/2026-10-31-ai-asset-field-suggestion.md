# AI 自動填入分類、對應虛擬角色與標籤 實作計畫

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增素材時，依 BOOTH 商品資訊以 Claude API 自動填入分類、對應虛擬角色與標籤，新名稱標示「新增」。

**Architecture:** `booth` crate 多解析商品說明等欄位；新的 `ai_suggest` crate 負責組請求、呼叫 Claude Messages API（可替換的 transport）、解析並比對現有清單；Tauri 指令 `suggest_asset_fields_with_ai` 串起來。前端在新增素材對話框進入填寫頁時呼叫，只填空白或 AI 先前填過且未修改的欄位。

**Tech Stack:** Rust 2024（Tauri 2、reqwest 0.13、serde、specta rc.22、thiserror）、React 19 + react-hook-form、vitest。

**Spec:** `docs/superpowers/specs/2026-10-31-ai-asset-field-suggestion-design.md`

## Global Constraints

- 在 `main` 分支開發；每個 Task 完成並通過檢查後立即 commit（conventional commits），訊息結尾加 `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`；不要 push。
- Rust 測試一律在 `src-backend/` 下執行 `cargo test --workspace [過濾字串]`（單獨 `-p model` 會因 feature 統一失敗）。
- 前端測試：專案根目錄執行 `pnpm test:run <路徑>`；pre-commit 會跑 `tsc --noEmit` 與 `eslint src/`。
- 模型 `claude-haiku-5-5`；端點 `https://api.anthropic.com/v1/messages`；標頭 `anthropic-version: 2026-10-21`；`max_tokens` 4096；`output_config.effort` `"low"`；逾時 15 秒，不自動重試；不送 `temperature`／`top_p`／`top_k`，不用 prefill。
- 標籤最多 5 個；分類最多 1 個。
- 設定檔版本維持 6；新欄位 `ai_suggest_enabled: bool`（預設 false）、`ai_suggest_api_key: String`（預設 ""），皆加 `#[serde(default)]`。
- 每個新的語言鍵都要加進 `src/locales/` 下全部 5 個檔案（en-US、en-GB、ja-JP、zh-CN、zh-TW），zh-TW 使用台灣用語；`node .github/workflows/scripts/locales-test.js` 必須通過。
- `src/lib/bindings.ts` 由 tauri-specta 在 debug 執行時產生；本計畫先手動編輯成與產生結果相同的內容，Task 10 以 `pnpm tauri dev` 重新產生並確認沒有差異。
- 不實作編輯對話框中的 AI、素材類型判斷、圖片輸入、模型選擇、金鑰測試按鈕、作業系統認證管理員。

## Review Focus

1. **過時的回應**：AI 回應抵達時，使用者已改了素材類型、或關掉對話框改新增別的商品 → 不得套用到目前的表單。（Task 7：`isResponseForCurrentForm` 測試）
2. **金鑰前後有空白或換行**（從網頁複製貼上很常見）→ 只有空白時視為未設定；有內容時去掉前後空白再使用與儲存。（Task 7：`shouldRunAiSuggest` 測試；Task 6：指令 trim；Task 9：失焦儲存時 trim）
3. **第一次使用、資料庫完全沒有素材**（現有清單全空）→ 請求仍正常產生，所有建議都標示「新增」。（Task 3、Task 4 測試）
4. **AI 回傳同一名稱的不同寫法**（「マヌカ」「マヌカ 」「MANUKA」與清單「Manuka」）→ 正規化後只留一個。（Task 4 測試）
5. **已啟用但金鑰是空的** → 不呼叫指令、不跳出錯誤提示。（Task 7：`shouldRunAiSuggest` 測試）

---

### Task 1: BOOTH 商品詳細資料解析與快取

**Files:**
- Modify: `src-backend/crates/booth/src/definitions.rs`
- Modify: `src-backend/crates/booth/src/cache.rs`
- Modify: `src-backend/crates/booth/src/booth.rs`
- Modify: `src-backend/crates/booth/src/lib.rs`（匯出 `BoothItemDetail`）
- Test: `src-backend/crates/booth/test/minimal.json`（新增，只含必要欄位）

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct BoothItemDetail {
      pub name: String,
      pub shop_name: String,
      pub category_name: Option<String>,
      pub description: String,
      pub tags: Vec<String>,
      pub variation_names: Vec<String>, // 去掉 null 與空字串
  }
  impl BoothFetcher {
      pub async fn fetch_detail(&mut self, id: u64) -> Result<BoothItemDetail, BoothInfoFetchError>;
  }
  ```
- `BoothJsonSchema` 追加 `#[serde(default)] description: String`、`#[serde(default)] tags: Vec<BoothTag { name }>`、`#[serde(default)] variations: Vec<BoothVariation { name: Option<String> }>`；`BoothCategory` 追加 `#[serde(default)] name: Option<String>`。
- `BoothCache` 改存 `(BoothAssetInfo, BoothItemDetail, i64)`：`get(id) -> Option<BoothAssetInfo>`（不變）、新增 `get_detail(id) -> Option<BoothItemDetail>`、`insert(id, info, detail)`。到期時間仍為 60 秒。
- `BoothAssetInfo` 與 `fetch` 的對外行為不變。

- [ ] **Step 1: 寫失敗的測試**（`booth.rs` tests 模組）

```rust
#[test]
fn test_parse_detail() {
    let mut fetcher = BoothFetcher::new("0.0.0+cargo-test");
    let body = include_str!("../test/6641548.json");
    fetcher.parse_response_and_process(body).unwrap();
    let detail = fetcher.cache.get_detail(6641548).unwrap();
    assert_eq!(detail.name, "KonoAsset - VRChat向けアセット管理ツール");
    assert_eq!(detail.shop_name, "silolab");
    assert_eq!(detail.category_name.as_deref(), Some("3Dツール・システム"));
    assert_eq!(detail.tags, vec!["VRChat".to_string()]);
    assert!(detail.variation_names.is_empty()); // 唯一的規格 name 為 null
    assert!(!detail.description.is_empty());
}

#[test]
fn test_parse_minimal_json_without_detail_fields() {
    let mut fetcher = BoothFetcher::new("0.0.0+cargo-test");
    let body = include_str!("../test/minimal.json");
    let info = fetcher.parse_response_and_process(body).unwrap();
    let detail = fetcher.cache.get_detail(info.id).unwrap();
    assert_eq!(detail.description, "");
    assert!(detail.tags.is_empty());
    assert!(detail.variation_names.is_empty());
    assert_eq!(detail.category_name, None);
}
```

`test/minimal.json` 只放 `id`、`name`、`shop.name`、`images: []`、`category: {"id": 209}`、`published_at`（RFC 3339）。cache.rs 現有測試改用新的 `insert` 簽名。

- [ ] **Step 2: 執行確認失敗**

Run: `cargo test --workspace test_parse_` 　Expected: 編譯失敗（`get_detail` 不存在）

- [ ] **Step 3: 實作** `BoothItemDetail`、schema 欄位、`BoothCache` 新簽名、`parse_response_and_process` 同時建立 detail 並寫入快取、`fetch_detail`（快取命中就回傳，否則走與 `fetch` 相同的請求後再讀快取）。

- [ ] **Step 4: 執行確認通過**

Run: `cargo test --workspace` 　Expected: 全部 PASS

- [ ] **Step 5: Commit** — `feat(booth): parse item description, tags and variations`

---

### Task 2: 設定新增 AI 輔助欄位

**Files:**
- Modify: `src-backend/crates/model/src/preference.rs`
- Modify: `src-backend/crates/loader/src/preferences.rs`（V5 → 現行版本的轉換、測試）
- Modify: `src/lib/bindings.ts`（`PreferenceStore` 型別）
- Modify: `src/lib/utils.ts:46-52`（預設 preference 物件）
- Modify: 其他建構 `PreferenceStore` 物件的 TS 測試（`src/components/context/PreferenceContext/hook/index.test.ts`、`logic/index.test.ts`），補上新欄位

**Interfaces:**
- Produces: `PreferenceStore.ai_suggest_enabled: bool`、`PreferenceStore.ai_suggest_api_key: String`；TS 端為 `aiSuggestEnabled: boolean`、`aiSuggestApiKey: string`。`overwrite()` 會複製這兩個欄位。

- [ ] **Step 1: 寫失敗的測試**（`loader/src/preferences.rs` tests）

```rust
#[test]
fn test_v6_without_ai_fields_loads_with_defaults() {
    let json = r#"{"version":6,"data":{"dataDirPath":"C:/data","theme":"system","language":"zh-TW",
      "deleteOnImport":false,"zipExtraction":true,"useUnitypackageSelectedOpen":true,
      "useTrashBin":true,"updateChannel":"Stable"}}"#;
    let versioned: VersionedPreferences = serde_json::from_str(json).unwrap();
    let pref: PreferenceStore = versioned.try_into().unwrap();
    assert!(!pref.ai_suggest_enabled);
    assert_eq!(pref.ai_suggest_api_key, "");
}

#[test]
fn test_ai_fields_round_trip() {
    // 建立含 ai_suggest_enabled = true、ai_suggest_api_key = "sk-test" 的 PreferenceStore，
    // 經 VersionedPreferences 序列化再反序列化後，兩個值不變，且 version 仍為 6
}
```

（`model/src/preference.rs` 另加：`overwrite()` 後兩個新欄位與來源相同。）

- [ ] **Step 2: 執行確認失敗** — Run: `cargo test --workspace ai_fields` 　Expected: 編譯失敗

- [ ] **Step 3: 實作** 欄位、`default()`、`overwrite()`、`LegacyPreferenceStoreV5 → PreferenceStore`（新欄位給預設值）；TS 型別與預設物件。

- [ ] **Step 4: 執行確認通過** — Run: `cargo test --workspace`、`pnpm test:run src/components/context/PreferenceContext` 　Expected: PASS

- [ ] **Step 5: Commit** — `feat(preference): add AI suggestion settings`

---

### Task 3: `ai_suggest` crate 與請求產生（prompt）

**Files:**
- Create: `src-backend/crates/ai_suggest/Cargo.toml`（依賴：model、booth、serde、serde_json、reqwest、thiserror、specta、log、tokio；workspace 欄位比照 `booth/Cargo.toml`）
- Create: `src-backend/crates/ai_suggest/src/lib.rs`
- Create: `src-backend/crates/ai_suggest/src/definitions.rs`
- Create: `src-backend/crates/ai_suggest/src/prompt.rs`
- Modify: `src-backend/Cargo.toml`（`[workspace.dependencies]` 加 `ai_suggest = { path = "./crates/ai_suggest", version = "0.0.0" }`）

**Interfaces:**
- Produces（`definitions.rs`，皆由 lib.rs 匯出）：
  ```rust
  pub struct ExistingCandidates { pub categories: Vec<String>, pub supported_avatars: Vec<String>, pub tags: Vec<String> }

  #[derive(Serialize, Clone, Debug, PartialEq, Eq, specta::Type)]
  #[serde(rename_all = "camelCase")]
  pub struct SuggestedValue { pub value: String, pub is_new: bool }

  #[derive(Serialize, Clone, Debug, PartialEq, Eq, specta::Type)]
  #[serde(rename_all = "camelCase")]
  pub struct AiSuggestion {
      pub category: Option<SuggestedValue>,
      pub supported_avatars: Vec<SuggestedValue>,
      pub tags: Vec<SuggestedValue>,
  }

  #[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, specta::Type, thiserror::Error)]
  pub enum AiSuggestErrorKind {
      NotConfigured, InvalidApiKey, InsufficientCredit, RateLimited, Overloaded,
      Timeout, Network, Refused, Truncated, InvalidResponse,
  } // 每個變體加 #[error("...")] 英文說明供 log 使用

  pub fn rank_by_usage<I, S>(values: I) -> Vec<String> where I: IntoIterator<Item = S>, S: AsRef<str>;
  // trim、略過空字串、依出現次數由多到少排序，次數相同依字串升冪；結果不重複
  ```
- Produces（`prompt.rs`）：
  ```rust
  pub const MODEL: &str = "claude-haiku-5-5";
  pub const MAX_TOKENS: u32 = 4096;
  pub const MAX_TAGS: usize = 5;
  pub fn response_schema() -> serde_json::Value;           // 規格 4.2 的 schema，原樣
  pub fn build_request(asset_type: AssetType, item: &BoothItemDetail, existing: &ExistingCandidates) -> serde_json::Value;
  ```
- `build_request` 產生 `{model, max_tokens, output_config: {effort: "low", format: {type: "json_schema", schema}}, system, messages: [{role: "user", content}]}`。
  `system` 依規格 4.3 的六條規則寫成英文指示（要求名稱沿用清單寫法、新名稱照 BOOTH 寫法、標籤最多 5 個、不適用欄位留空、無法判斷時留空）。
  `content` 依規格 4.3 的標記格式；清單每行一個名稱，清單為空時該標記內容寫 `(none)`；
  `existing_categories` 只在 AvatarWearable／WorldObject／OtherAsset 送出，`existing_supported_avatars` 只在 AvatarWearable 送出，`existing_tags` 一律送出。

- [ ] **Step 1: 寫失敗的測試**（`prompt.rs`、`definitions.rs` tests）

```rust
#[test]
fn request_has_fixed_api_parameters() {
    let req = build_request(AssetType::AvatarWearable, &sample_item(), &sample_existing());
    assert_eq!(req["model"], "claude-haiku-5-5");
    assert_eq!(req["max_tokens"], 4096);
    assert_eq!(req["output_config"]["effort"], "low");
    assert_eq!(req["output_config"]["format"]["type"], "json_schema");
    assert_eq!(req["output_config"]["format"]["schema"], response_schema());
    assert!(req.get("temperature").is_none());
}

#[test]
fn wearable_request_contains_item_and_all_lists() {
    let content = user_content(&build_request(AssetType::AvatarWearable, &sample_item(), &sample_existing()));
    for s in ["<asset_type>AvatarWearable</asset_type>", "マヌカ用", "衣装", "Modular Avatar", sample_item().description.as_str()] {
        assert!(content.contains(s), "missing {s}");
    }
    assert!(content.contains("<existing_supported_avatars>"));
    assert!(content.contains("<existing_categories>"));
}

#[test]
fn avatar_request_omits_category_and_avatar_lists() {
    let content = user_content(&build_request(AssetType::Avatar, &sample_item(), &sample_existing()));
    assert!(!content.contains("<existing_categories>"));
    assert!(!content.contains("<existing_supported_avatars>"));
    assert!(content.contains("<existing_tags>"));
}

#[test]
fn empty_existing_lists_render_as_none() { // Review Focus 3
    let content = user_content(&build_request(AssetType::WorldObject, &sample_item(), &ExistingCandidates::default()));
    assert!(content.contains("<existing_categories>(none)</existing_categories>"));
}

#[test]
fn rank_by_usage_orders_by_count_then_name() {
    assert_eq!(rank_by_usage([" b", "a", "b", "", "c", "a", "b"]), vec!["b", "a", "c"]);
}
```

（`sample_item()` 的 variation_names 含 `"マヌカ用"`；`sample_existing()` 的分類含 `"衣装"`、標籤含 `"Modular Avatar"`。`user_content` 為取出 `messages[0].content` 字串的測試輔助函式。`ExistingCandidates` derive `Default`。）

- [ ] **Step 2: 執行確認失敗** — Run: `cargo test --workspace -- prompt` 　Expected: 編譯失敗

- [ ] **Step 3: 實作** 上述型別與函式。

- [ ] **Step 4: 執行確認通過** — Run: `cargo test --workspace` 　Expected: PASS

- [ ] **Step 5: Commit** — `feat(ai_suggest): add crate with request builder`

---

### Task 4: 回應比對與「新增」判斷（resolve）

**Files:**
- Create: `src-backend/crates/ai_suggest/src/resolve.rs`

**Interfaces:**
- Consumes: Task 3 的 `ExistingCandidates`、`AiSuggestion`、`SuggestedValue`、`MAX_TAGS`
- Produces:
  ```rust
  #[derive(Deserialize, Debug, PartialEq)]
  pub(crate) struct RawSuggestion {
      pub category: Option<String>,
      #[serde(rename = "supportedAvatars")] pub supported_avatars: Vec<String>,
      pub tags: Vec<String>,
  }
  pub(crate) fn resolve(raw: RawSuggestion, asset_type: AssetType, existing: &ExistingCandidates) -> AiSuggestion;
  ```
- 規則（規格 4.4 第 4 點）：trim、捨棄空字串；與對應清單比對時以 `to_lowercase()` 比較，命中就改用清單寫法並 `is_new = false`；正規化後去除重複（保留第一次出現的順序）；標籤取前 `MAX_TAGS` 個；Avatar 的 `category` 為 `None` 且 `supported_avatars` 為空；WorldObject／OtherAsset 的 `supported_avatars` 為空。

- [ ] **Step 1: 寫失敗的測試**

```rust
#[test]
fn existing_names_are_not_new_and_use_list_spelling() {
    let existing = ExistingCandidates { categories: vec!["衣装".into()], supported_avatars: vec!["Manuka".into()], tags: vec!["Modular Avatar".into()] };
    let raw = RawSuggestion { category: Some(" 衣装 ".into()), supported_avatars: vec!["MANUKA".into(), "桔梗".into()], tags: vec!["modular avatar".into()] };
    let s = resolve(raw, AssetType::AvatarWearable, &existing);
    assert_eq!(s.category, Some(SuggestedValue { value: "衣装".into(), is_new: false }));
    assert_eq!(s.supported_avatars, vec![
        SuggestedValue { value: "Manuka".into(), is_new: false },
        SuggestedValue { value: "桔梗".into(), is_new: true },
    ]);
    assert_eq!(s.tags, vec![SuggestedValue { value: "Modular Avatar".into(), is_new: false }]);
}

#[test]
fn duplicates_after_normalization_are_removed() { // Review Focus 4
    let existing = ExistingCandidates { supported_avatars: vec!["Manuka".into()], ..Default::default() };
    let raw = RawSuggestion { category: None, supported_avatars: vec!["manuka".into(), "Manuka ".into(), "MANUKA".into(), "".into()], tags: vec![] };
    assert_eq!(resolve(raw, AssetType::AvatarWearable, &existing).supported_avatars.len(), 1);
}

#[test]
fn everything_is_new_when_lists_are_empty() { // Review Focus 3
    let raw = RawSuggestion { category: Some("衣装".into()), supported_avatars: vec!["マヌカ".into()], tags: vec!["VRChat".into()] };
    let s = resolve(raw, AssetType::AvatarWearable, &ExistingCandidates::default());
    assert!(s.category.unwrap().is_new && s.supported_avatars[0].is_new && s.tags[0].is_new);
}

#[test]
fn tags_are_limited_to_five() { /* 7 個標籤 → 長度 5，順序與輸入相同 */ }

#[test]
fn fields_not_applicable_to_type_are_dropped() {
    // Avatar：category None、supported_avatars 空、tags 保留
    // WorldObject 與 OtherAsset：supported_avatars 空、category 保留
}
```

- [ ] **Step 2: 執行確認失敗** — Run: `cargo test --workspace -- resolve` 　Expected: 編譯失敗

- [ ] **Step 3: 實作 `resolve`**

- [ ] **Step 4: 執行確認通過** — Run: `cargo test --workspace` 　Expected: PASS

- [ ] **Step 5: Commit** — `feat(ai_suggest): resolve suggestions against existing names`

---

### Task 5: Claude API 呼叫與錯誤對應（client）及 `suggest()`

**Files:**
- Create: `src-backend/crates/ai_suggest/src/client.rs`
- Modify: `src-backend/crates/ai_suggest/src/lib.rs`

**Interfaces:**
- Consumes: Task 3 `build_request`、Task 4 `RawSuggestion`／`resolve`
- Produces（`client.rs`）：
  ```rust
  pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
  pub const ANTHROPIC_VERSION: &str = "2026-10-21";
  pub const TIMEOUT_SECS: u64 = 15;

  pub struct HttpResponse { pub status: u16, pub body: String }

  pub trait MessagesTransport {
      fn send(&self, api_key: &str, body: &serde_json::Value)
          -> impl std::future::Future<Output = Result<HttpResponse, AiSuggestErrorKind>> + Send;
  }

  pub struct ReqwestTransport { /* reqwest::Client */ }
  impl ReqwestTransport { pub fn new<S: AsRef<str>>(version: S) -> Self; } // UA "KonoAsset/{version}"，timeout 15 秒
  // reqwest 錯誤：is_timeout() → Timeout，其餘 → Network

  pub(crate) fn map_http_error(status: u16, body: &str) -> AiSuggestErrorKind;
  pub(crate) fn extract_output_text(body: &str) -> Result<String, AiSuggestErrorKind>;
  ```
- `map_http_error`：先讀 body 的 `error.type`：`authentication_error`／`permission_error` → `InvalidApiKey`，`billing_error` → `InsufficientCredit`，`rate_limit_error` → `RateLimited`，`overloaded_error`／`api_error` → `Overloaded`；body 無法解析時依狀態碼：401／403 → `InvalidApiKey`、402 → `InsufficientCredit`、429 → `RateLimited`、5xx 與 529 → `Overloaded`；其他 → `InvalidResponse`。
- `extract_output_text`：`stop_reason == "refusal"` → `Refused`；`"max_tokens"` → `Truncated`；跳過非 `text` 區塊，回傳第一個 `text` 區塊；沒有 → `InvalidResponse`。
- Produces（`lib.rs`）：
  ```rust
  pub async fn suggest<T: MessagesTransport>(
      transport: &T, api_key: &str, asset_type: AssetType,
      item: &BoothItemDetail, existing: &ExistingCandidates,
  ) -> Result<AiSuggestion, AiSuggestErrorKind>;
  // build_request → send → 非 2xx 用 map_http_error → extract_output_text → serde_json 解析成 RawSuggestion（失敗 → InvalidResponse）→ resolve
  ```

- [ ] **Step 1: 寫失敗的測試**（`client.rs` 與 `lib.rs` tests；用 `FakeTransport { response: Result<HttpResponse, AiSuggestErrorKind> }` 實作 trait，並記錄收到的 api_key 與 body）

```rust
#[tokio::test]
async fn suggest_returns_resolved_values_on_success() {
    let body = r#"{"stop_reason":"end_turn","content":[{"type":"thinking","thinking":""},
      {"type":"text","text":"{\"category\":\"衣装\",\"supportedAvatars\":[\"マヌカ\"],\"tags\":[]}"}]}"#;
    let t = FakeTransport::ok(200, body);
    let s = suggest(&t, "sk-test", AssetType::AvatarWearable, &sample_item(), &ExistingCandidates::default()).await.unwrap();
    assert_eq!(s.category.unwrap().value, "衣装");
    assert_eq!(t.received_key(), "sk-test");
}

#[test]
fn http_errors_map_to_kinds() {
    let e = |status, ty: &str| map_http_error(status, &format!(r#"{{"type":"error","error":{{"type":"{ty}","message":"x"}}}}"#));
    assert_eq!(e(401, "authentication_error"), AiSuggestErrorKind::InvalidApiKey);
    assert_eq!(e(403, "permission_error"), AiSuggestErrorKind::InvalidApiKey);
    assert_eq!(e(402, "billing_error"), AiSuggestErrorKind::InsufficientCredit);
    assert_eq!(e(429, "rate_limit_error"), AiSuggestErrorKind::RateLimited);
    assert_eq!(e(529, "overloaded_error"), AiSuggestErrorKind::Overloaded);
    assert_eq!(map_http_error(502, "<html>bad gateway</html>"), AiSuggestErrorKind::Overloaded);
    assert_eq!(map_http_error(404, "not json"), AiSuggestErrorKind::InvalidResponse);
}

#[test]
fn stop_reasons_and_missing_text_are_errors() {
    assert_eq!(extract_output_text(r#"{"stop_reason":"refusal","content":[]}"#), Err(AiSuggestErrorKind::Refused));
    assert_eq!(extract_output_text(r#"{"stop_reason":"max_tokens","content":[{"type":"text","text":"{"}]}"#), Err(AiSuggestErrorKind::Truncated));
    assert_eq!(extract_output_text(r#"{"stop_reason":"end_turn","content":[{"type":"thinking","thinking":""}]}"#), Err(AiSuggestErrorKind::InvalidResponse));
}

#[tokio::test]
async fn invalid_json_text_and_transport_errors_propagate() {
    // text 為 "not json" → InvalidResponse
    // FakeTransport::err(Timeout) → Timeout；FakeTransport::err(Network) → Network
}
```

- [ ] **Step 2: 執行確認失敗** — Run: `cargo test --workspace -- client` 　Expected: 編譯失敗

- [ ] **Step 3: 實作** `client.rs`、`ReqwestTransport`（POST，標頭 `x-api-key`、`anthropic-version`、`content-type: application/json`）、`suggest`。

- [ ] **Step 4: 執行確認通過** — Run: `cargo test --workspace` 　Expected: PASS

- [ ] **Step 5: Commit** — `feat(ai_suggest): call Claude Messages API and map errors`

---

### Task 6: Tauri 指令 `suggest_asset_fields_with_ai`

**Files:**
- Create: `src-backend/tauri/src/command/external/ai_suggest.rs`
- Modify: `src-backend/tauri/src/command/external/mod.rs`（`pub mod ai_suggest;`）
- Modify: `src-backend/tauri/src/command/mod.rs`（在「外部API関連」加入 `external::ai_suggest::suggest_asset_fields_with_ai`）
- Modify: `src-backend/tauri/src/lib.rs:82` 附近（`.manage(ai_suggest::ReqwestTransport::new(VERSION))`）
- Modify: `src-backend/tauri/Cargo.toml`（`ai_suggest.workspace = true`）
- Modify: `src/lib/bindings.ts`（指令與 `AiSuggestion`、`SuggestedValue`、`AiSuggestErrorKind` 型別）

**Interfaces:**
- Consumes: Task 1 `fetch_detail`、Task 2 preference 欄位、Task 3 `rank_by_usage`／型別、Task 5 `suggest`／`ReqwestTransport`
- Produces:
  ```rust
  #[tauri::command]
  #[specta::specta]
  pub async fn suggest_asset_fields_with_ai(
      preference: State<'_, Arc<Mutex<PreferenceStore>>>,
      booth_fetcher: State<'_, Mutex<BoothFetcher>>,
      basic_store: State<'_, Arc<Mutex<AssetStorage>>>,
      transport: State<'_, ReqwestTransport>,
      booth_item_id: u64,
      asset_type: AssetType,
  ) -> Result<AiSuggestion, AiSuggestErrorKind>
  ```
  TS：`commands.suggestAssetFieldsWithAi(boothItemId: number, assetType: AssetType): Promise<Result<AiSuggestion, AiSuggestErrorKind>>`；
  `AiSuggestErrorKind` 為 `"NotConfigured" | "InvalidApiKey" | "InsufficientCredit" | "RateLimited" | "Overloaded" | "Timeout" | "Network" | "Refused" | "Truncated" | "InvalidResponse"`。
- 流程：讀 preference，金鑰 `trim()` 後為空或未啟用 → `NotConfigured`；`fetch_detail` 失敗 → `Network`；
  現有清單：分類依類型取對應 store 的 `category`（Avatar 為空），對應角色取 AvatarWearable 的 `supported_avatars`，標籤取四個 store 全部的 `description.tags`，各自經 `rank_by_usage`；
  呼叫 `suggest(&transport, key.trim(), ...)`。成功與失敗都以 `log::info!`／`log::error!` 記錄（不記錄金鑰）。
- 鎖的順序：先複製出 preference 值並釋放，再鎖 booth_fetcher，再鎖 storage；呼叫 API 時不持有任何鎖。

- [ ] **Step 1: 實作指令、註冊、manage、bindings**（集計邏輯已由 Task 3 的 `rank_by_usage` 測試覆蓋；此 Task 只做串接）

- [ ] **Step 2: 確認編譯與既有測試** — Run: `cargo check --workspace` 與 `cargo test --workspace` 　Expected: 成功、PASS；`pnpm run type-check` 通過

- [ ] **Step 3: Commit** — `feat: add suggest_asset_fields_with_ai command`

---

### Task 7: 前端 AI 建議邏輯與 hook

**Files:**
- Create: `src/components/model-legacy/asset-dialogs/AddAssetDialog/ai-suggest/logic.ts`
- Create: `src/components/model-legacy/asset-dialogs/AddAssetDialog/ai-suggest/logic.test.ts`
- Create: `src/components/model-legacy/asset-dialogs/AddAssetDialog/ai-suggest/hook.ts`
- Modify: `src/components/model-legacy/asset-dialogs/AddAssetDialog/hook/index.ts`（呼叫 `useAiSuggest`，回傳 `aiSuggest`）
- Modify: `src/components/model-legacy/asset-dialogs/AddAssetDialog/index.tsx`（把 `aiSuggest` 傳給 `ManualInputTab`）
- Modify: 5 個語言檔（錯誤訊息鍵，見下表）

**Interfaces:**
- Consumes: Task 6 的 `commands.suggestAssetFieldsWithAi` 與型別；Task 2 的 `aiSuggestEnabled`／`aiSuggestApiKey`
- Produces（`logic.ts`）：
  ```ts
  export type AiFilledValues = { category: string | null; supportedAvatars: string[] | null; tags: string[] | null }
  export const EMPTY_AI_FILLED: AiFilledValues // 三個皆 null
  export type AiNewValues = { category: string | null; supportedAvatars: string[]; tags: string[] }
  export type AiSuggestViewState = { loading: boolean; newValues: AiNewValues }
  export const shouldRunAiSuggest = (p: { enabled: boolean; apiKey: string; boothItemId: number | null }) => boolean
  export const requestKey = (boothItemId: number, assetType: AssetType) => string // `${boothItemId}:${assetType}`
  export const isResponseForCurrentForm = (key: string, form: AssetFormType) => boolean
  export const applyAiSuggestion = (form: AssetFormType, s: AiSuggestion, previous: AiFilledValues) => AiFilledValues
  export const collectNewValues = (s: AiSuggestion) => AiNewValues
  export const errorMessageKey = (e: AiSuggestErrorKind) => string | null // 回傳語言鍵；NotConfigured → null
  ```
- `applyAiSuggestion` 規則：每個欄位「目前為空（'' 或 []）或與 `previous` 記錄的值相同」才可替換；可替換時寫入建議值（建議為空也寫入空值，以清掉上一個類型留下的 AI 值），並把寫入值記錄到回傳的 `AiFilledValues`；不可替換時回傳值沿用 `previous` 的該欄位。
- Produces（`hook.ts`）：
  ```ts
  export const useAiSuggest = (p: { form: AssetFormType; tab: string; dialogOpen: boolean }) => AiSuggestViewState
  ```
  `tab === 'manual-input'` 且 `shouldRunAiSuggest` 為真、且 `requestKey` 與上次不同時才呼叫；回應到達時 `isResponseForCurrentForm` 為假就丟棄；`dialogOpen` 變成 true 時重設上次 key、`AiFilledValues` 與 `newValues`。錯誤時依 `errorMessageKey` 用 `useToast` 顯示（`title: t(key)`，`duration: 5000`）。
- 語言鍵（zh-TW／en-US／ja-JP；en-GB 同 en-US；zh-CN 為對應簡體）：

  | 鍵 | zh-TW | en-US | ja-JP |
  |---|---|---|---|
  | `addasset:ai-suggest:error:invalid-api-key` | API 金鑰無效或沒有權限，請到設定確認 | The API key is invalid or lacks permission. Please check your settings | APIキーが無効か権限がありません。設定を確認してください |
  | `addasset:ai-suggest:error:insufficient-credit` | Claude API 額度不足 | Your Claude API credit balance is too low | Claude APIのクレジットが不足しています |
  | `addasset:ai-suggest:error:rate-limited` | 請求太頻繁，請稍後再試 | Too many requests. Please try again later | リクエストが多すぎます。しばらくしてから再試行してください |
  | `addasset:ai-suggest:error:overloaded` | Claude 服務忙碌中，請稍後再試 | Claude is busy right now. Please try again later | Claudeが混雑しています。しばらくしてから再試行してください |
  | `addasset:ai-suggest:error:failed` | AI 判斷失敗，請手動填寫 | AI suggestion failed. Please fill in the fields manually | AIによる判定に失敗しました。手動で入力してください |

- [ ] **Step 1: 寫失敗的測試**（`logic.test.ts`，用 `useForm` 的 `renderHook` 建立真實表單）

```ts
it('fills only empty fields', () => {
  form.setValue('tags', ['手動'])
  const filled = applyAiSuggestion(form, suggestion({ category: '衣装', supportedAvatars: ['マヌカ'], tags: ['VRChat'] }), EMPTY_AI_FILLED)
  expect(form.getValues('category')).toBe('衣装')
  expect(form.getValues('supportedAvatars')).toEqual(['マヌカ'])
  expect(form.getValues('tags')).toEqual(['手動'])
  expect(filled.tags).toBeNull()
})

it('replaces AI-filled untouched values on re-run but keeps user edits', () => {
  const first = applyAiSuggestion(form, suggestion({ category: '衣装', supportedAvatars: ['マヌカ'], tags: ['A'] }), EMPTY_AI_FILLED)
  form.setValue('tags', ['A', '手動'])
  applyAiSuggestion(form, suggestion({ category: 'ワールド', supportedAvatars: [], tags: ['B'] }), first)
  expect(form.getValues('category')).toBe('ワールド')
  expect(form.getValues('supportedAvatars')).toEqual([])
  expect(form.getValues('tags')).toEqual(['A', '手動'])
})

it('ignores responses for another item or type', () => { // Review Focus 1
  form.setValue('boothItemId', 1); form.setValue('assetType', 'WorldObject')
  expect(isResponseForCurrentForm(requestKey(1, 'AvatarWearable'), form)).toBe(false)
  expect(isResponseForCurrentForm(requestKey(2, 'WorldObject'), form)).toBe(false)
  expect(isResponseForCurrentForm(requestKey(1, 'WorldObject'), form)).toBe(true)
})

it('runs only when enabled with a non-blank key and a BOOTH item', () => { // Review Focus 2, 5
  expect(shouldRunAiSuggest({ enabled: true, apiKey: 'sk', boothItemId: 1 })).toBe(true)
  expect(shouldRunAiSuggest({ enabled: true, apiKey: '  \n', boothItemId: 1 })).toBe(false)
  expect(shouldRunAiSuggest({ enabled: true, apiKey: '', boothItemId: 1 })).toBe(false)
  expect(shouldRunAiSuggest({ enabled: false, apiKey: 'sk', boothItemId: 1 })).toBe(false)
  expect(shouldRunAiSuggest({ enabled: true, apiKey: 'sk', boothItemId: null })).toBe(false)
})

it('collects only new values', () => { /* is_new 的項目進入 AiNewValues，其餘不進 */ })

it('maps error kinds to messages', () => {
  expect(errorMessageKey('NotConfigured')).toBeNull()
  expect(errorMessageKey('InvalidApiKey')).toBe('addasset:ai-suggest:error:invalid-api-key')
  expect(errorMessageKey('InsufficientCredit')).toBe('addasset:ai-suggest:error:insufficient-credit')
  expect(errorMessageKey('RateLimited')).toBe('addasset:ai-suggest:error:rate-limited')
  expect(errorMessageKey('Overloaded')).toBe('addasset:ai-suggest:error:overloaded')
  for (const k of ['Timeout', 'Network', 'Refused', 'Truncated', 'InvalidResponse'] as const)
    expect(errorMessageKey(k)).toBe('addasset:ai-suggest:error:failed')
})
```

- [ ] **Step 2: 執行確認失敗** — Run: `pnpm test:run src/components/model-legacy/asset-dialogs/AddAssetDialog/ai-suggest` 　Expected: FAIL（模組不存在）

- [ ] **Step 3: 實作** `logic.ts`、`hook.ts`、接到 `useAddAssetDialog` 與 `AddAssetDialog`、加語言鍵。

- [ ] **Step 4: 執行確認通過** — Run: 同上測試、`node .github/workflows/scripts/locales-test.js`、`pnpm run type-check` 　Expected: PASS

- [ ] **Step 5: Commit** — `feat: run AI suggestion when entering manual input tab`

---

### Task 8: 填寫頁的載入提示與「新增」標記

**Files:**
- Modify: `src/components/ui/multi-select.tsx`（新增選填 props `highlightedValues?: string[]`、`highlightLabel?: string`）
- Modify: `src/components/model-legacy/asset-dialogs/components/tabs/ManualInputTab/index.tsx`（選填 prop `aiSuggest?: AiSuggestViewState`；`loading` 時在 layout 上方顯示 `Loader2`（`animate-spin`）與 `t('addasset:ai-suggest:loading')`；把 `aiSuggest?.newValues` 傳給 layout）
- Modify: `ManualInputTab/layout/{AvatarLayout,AvatarWearableLayout,WorldObjectLayout,OtherAssetLayout}/index.tsx`（選填 prop `aiNewValues?: AiNewValues`）
- Modify: 5 個語言檔
- Test: `src/components/ui/multi-select.test.tsx`（新增）

**Interfaces:**
- Consumes: Task 7 `AiSuggestViewState`、`AiNewValues`
- `MultipleSelector`：`highlightedValues` 中的已選值，徽章加上 `data-highlighted` 屬性與醒目樣式（例如 `bg-amber-500 text-white`），並在名稱後顯示 `highlightLabel` 小字。
- layout：對應角色與標籤的 `MultipleSelector` 傳 `highlightedValues={aiNewValues?.supportedAvatars / tags}`、`highlightLabel={t('addasset:ai-suggest:new')}`；`value` 改用 `form.watch('supportedAvatars')`／`form.watch('tags')`，確保 AI 寫入後畫面更新。
  分類：`aiNewValues?.category` 有值且等於 `form.watch('category')` 時，在分類 `Label` 旁顯示 `Badge`（文字 `t('addasset:ai-suggest:new')`）。
- `EditAssetDialog` 不傳新 props，行為不變。
- 語言鍵：

  | 鍵 | zh-TW | en-US | ja-JP |
  |---|---|---|---|
  | `addasset:ai-suggest:loading` | AI 判斷中… | AI is suggesting… | AIが判定中… |
  | `addasset:ai-suggest:new` | 新增 | New | 新規 |

- [ ] **Step 1: 寫失敗的測試**（`multi-select.test.tsx`，用 Testing Library render）

```tsx
it('marks highlighted badges with the label', () => {
  render(<MultipleSelector value={['A', 'B']} highlightedValues={['B']} highlightLabel="新增" />)
  expect(screen.getByText('B').closest('[data-badge]')?.hasAttribute('data-highlighted')).toBe(true)
  expect(screen.getByText('A').closest('[data-badge]')?.hasAttribute('data-highlighted')).toBe(false)
  expect(screen.getAllByText('新增')).toHaveLength(1)
})
```

（vitest 已設定 jsdom；未安裝 jest-dom，故用 `hasAttribute` 判斷。寫法比照 `src/components/model-legacy/asset-dialogs/components/tabs/BoothInputTab/add/index.test.tsx`，結尾 `afterEach(cleanup)`。）

- [ ] **Step 2: 執行確認失敗** — Run: `pnpm test:run src/components/ui/multi-select.test.tsx` 　Expected: FAIL

- [ ] **Step 3: 實作** 上述元件變更與語言鍵。

- [ ] **Step 4: 執行確認通過** — Run: 同上、`node .github/workflows/scripts/locales-test.js`、`pnpm run type-check`、`pnpm run lint` 　Expected: PASS

- [ ] **Step 5: Commit** — `feat: show AI loading state and new-name badges`

---

### Task 9: 設定頁「AI 輔助」區塊

**Files:**
- Create: `src/components/model-legacy/preference/AiSuggestSettings/index.tsx`
- Modify: `src/pages/Preference/components/model/PreferenceTabs/tabs/SettingsTab/index.tsx`（放在 `UseTrashBinSelectorToggle` 之後、`<Separator />` 之前）
- Modify: 5 個語言檔

**Interfaces:**
- Consumes: Task 2 欄位
- Produces:
  ```tsx
  type Props = { enabled: boolean; apiKey: string; setEnabled: (enabled: boolean) => void; setApiKey: (apiKey: string) => void }
  export const AiSuggestSettings: FC<Props>
  ```
  版面比照 `UseTrashBinSelectorToggle`（標題、說明、右側 `Switch`）；下方一列為金鑰 `Input`（`type` 在 `password`／`text` 間切換，右側 `Eye`／`EyeOff` 圖示按鈕），`autoComplete="off"`。
  輸入時只更新本地 state；`onBlur` 時把 `trim()` 後的值交給 `setApiKey`（值沒變就不呼叫）。
  `SettingsTab` 以 `setPreference({ ...preference, aiSuggestEnabled }, true)`／`setPreference({ ...preference, aiSuggestApiKey }, true)` 儲存。
- 語言鍵：

  | 鍵 | zh-TW | en-US | ja-JP |
  |---|---|---|---|
  | `preference:settings:ai-suggest` | AI 輔助 | AI assistance | AIアシスト |
  | `preference:settings:ai-suggest:explanation-text` | 新增素材時，以 Claude 自動填入分類、對應虛擬角色與標籤。會將 BOOTH 商品資訊與你現有的分類、虛擬角色、標籤清單傳送給 Anthropic，費用由此金鑰所屬的帳號支付 | Automatically fill in category, supported avatars and tags with Claude when adding assets. BOOTH item details and your existing category, avatar and tag lists are sent to Anthropic, and usage is billed to the account that owns this key | アセット追加時に、Claudeでカテゴリ・対応アバター・タグを自動入力します。BOOTHの商品情報と既存のカテゴリ・アバター・タグの一覧がAnthropicに送信され、料金はこのキーのアカウントに請求されます |
  | `preference:settings:ai-suggest:api-key` | Claude API 金鑰 | Claude API key | Claude APIキー |
  | `preference:settings:ai-suggest:api-key:placeholder` | sk-ant-... | sk-ant-... | sk-ant-... |

- [ ] **Step 1: 實作元件、接到 SettingsTab、加語言鍵**（純 UI 串接，沒有新的邏輯分支；trim 行為由 Step 2 手動確認）

- [ ] **Step 2: 確認** — Run: `node .github/workflows/scripts/locales-test.js`、`pnpm run type-check`、`pnpm run lint` 　Expected: PASS

- [ ] **Step 3: Commit** — `feat: add AI assistance settings`

---

### Task 10: 整合確認、bindings 重新產生、更新紀錄

**Files:**
- Modify: `CHANGELOG.md`（「## 尚未發布」→「### 🚀新功能」加一行：`- 新增素材時，可用 Claude 自動填入分類、對應虛擬角色與標籤（需在設定中填入 API 金鑰）`）
- 可能 Modify: `src/lib/bindings.ts`（若重新產生後有差異）

- [ ] **Step 1: 全部測試** — Run: `cargo test --workspace`（於 `src-backend/`）、`pnpm test:run`、`pnpm run type-check`、`pnpm run lint`、`node .github/workflows/scripts/locales-test.js` 　Expected: 全部 PASS

- [ ] **Step 2: 重新產生 bindings** — Run: `pnpm tauri dev`，啟動後關閉；`git diff src/lib/bindings.ts` 應無差異（有差異就採用產生結果並重跑 `pnpm run type-check`）

- [ ] **Step 3: 實際測試（須先徵得使用者同意，會花費約台幣 0.05 元以內）** — 在 dev 版的設定中啟用 AI 輔助並填入使用者的金鑰；新增素材 → 貼 `https://edgemode.booth.pm/items/9059890` → 選素材類型 → 確認填寫頁出現「AI 判斷中…」，之後三個欄位有合理的值，清單外的名稱有「新增」標記；回上一頁改類型再進入，確認只有 AI 填的欄位被替換。
  另確認：金鑰填錯字時顯示「API 金鑰無效或沒有權限，請到設定確認」；關閉 AI 輔助時沒有任何呼叫。

- [ ] **Step 4: Commit** — `docs: add AI suggestion to changelog`（如 bindings 有變更一併提交）
