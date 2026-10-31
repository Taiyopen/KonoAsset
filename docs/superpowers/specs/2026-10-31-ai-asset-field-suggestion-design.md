# AI 自動填入分類、對應虛擬角色與標籤 — 設計規格

- 日期：2026-10-31
- 狀態：待審查
- 範圍：Feature A（BOOTH 網址 → AI 建議欄位）。自動下載與匯入（Feature B）不在本規格範圍內。

## 1. 目的

新增素材時，使用者常需要大量貼上網址並手動選擇分類、對應虛擬角色與標籤。
本功能在取得 BOOTH 商品資訊後，使用 Claude API 自動判斷這三個欄位並預先填入，
使用者只需檢查並刪除不正確的項目。

### 成功條件

- 貼上 BOOTH 網址、選好素材類型後，進入填寫頁時三個欄位已由 AI 填入（大約 1～3 秒）。
- AI 優先使用使用者資料庫中既有的名稱；只有找不到時才提出新名稱，並清楚標示「新增」。
- 沒有啟用功能、沒有填入金鑰、手動輸入（沒有 BOOTH 網址）時，流程與現在完全相同。
- AI 失敗時不會擋住流程，使用者仍可手動填寫。

### 已確定的決策

| 項目 | 決定 |
|---|---|
| LLM 供應商 | Claude API，模型 `claude-haiku-5-5` |
| 名稱來源 | 優先用現有名稱，必要時才新增並標示「新增」 |
| 填寫欄位 | 分類、對應虛擬角色、標籤 |
| 執行時機 | 自動執行（確認素材類型、進入填寫頁時） |
| 金鑰存放 | `preference.json`（明文，僅限朋友間內部使用） |
| 實作位置 | Rust 後端呼叫 Claude API（以 reqwest 發送 HTTP） |

各素材類型可填的欄位：

| 素材類型 | 分類 | 對應虛擬角色 | 標籤 |
|---|---|---|---|
| 虛擬角色本體（Avatar） | — | — | ✓ |
| 虛擬角色相關（AvatarWearable） | ✓ | ✓ | ✓ |
| 世界素材（WorldObject） | ✓ | — | ✓ |
| 其他（OtherAsset） | ✓ | — | ✓ |

## 2. 整體架構

### 2.1 新增素材流程與執行時機

現有流程：選擇檔案 → BOOTH 網址輸入 → 素材類型選擇 → 填寫資訊（`manual-input`）→ 附加資訊 → 完成。

分類清單依素材類型而不同，因此 AI 在**進入 `manual-input` 分頁時**以當下選定的素材類型執行。
若使用者回到上一頁變更素材類型，再次進入時會以新類型重新執行（規則見 3.2）。

執行條件（全部成立才執行）：

1. 設定中已啟用 AI 輔助，且 API 金鑰不為空白。
2. 表單中有 `boothItemId`（即經由 BOOTH 網址取得資訊）。

### 2.2 後端元件

#### (1) `booth` crate 擴充

`BoothJsonSchema` 追加解析下列欄位（皆為選填，缺少時不視為錯誤）：

- `description: String` — 商品說明全文
- `tags: Vec<{ name: String }>` — 作者設定的標籤
- `variations: Vec<{ name: Option<String> }>` — 商品規格名稱（常包含對應角色，例如「マヌカ用」）
- `category.name: String` — BOOTH 分類名稱

新增內部結構 `BoothItemDetail { description, tags, variation_names, category_name }`，
與現有的 `BoothAssetInfo` 一起存在 `BoothCache` 中。
新增方法 `BoothFetcher::fetch_detail(id)`：快取有資料時直接回傳，沒有時與 `fetch` 共用同一個請求流程。
傳給前端的 `BoothAssetInfo` 結構不變。

#### (2) 新增 `ai_suggest` crate（`src-backend/crates/ai_suggest`）

依職責分成三個模組，各自可獨立測試：

| 模組 | 職責 | 輸入 → 輸出 |
|---|---|---|
| `prompt` | 組出送給 Claude 的請求內容 | BOOTH 資料 + 素材類型 + 現有清單 → 請求 JSON |
| `client` | 透過 HTTP 呼叫 Claude Messages API，並對應錯誤 | 請求 JSON + API 金鑰 → 回應文字或 `AiSuggestError` |
| `resolve` | 解析 AI 回傳的 JSON，與現有清單比對並標示「新增」 | 回應 JSON + 現有清單 + 素材類型 → `AiSuggestion` |

`client` 以 trait（例如 `MessagesTransport`）抽象 HTTP 呼叫，測試時可替換成假的回應。

#### (3) 新增 Tauri 指令

```rust
#[tauri::command]
#[specta::specta]
pub async fn suggest_asset_fields_with_ai(
    booth_item_id: u64,
    asset_type: AssetType,
) -> Result<AiSuggestion, AiSuggestErrorKind>
```

處理流程：

1. 從 `PreferenceStore` 讀取啟用狀態與金鑰；未啟用或金鑰為空時回傳 `NotConfigured`。
2. 以 `BoothFetcher::fetch_detail` 取得 BOOTH 資料（通常命中快取）。
3. 從 `AssetStorage` 取得現有清單，沿用 `command/suggest/get.rs` 的統計方式：
   - 分類：依素材類型，分別取 AvatarWearable / WorldObject / OtherAsset 的分類清單。
   - 對應虛擬角色：AvatarWearable 的對應角色清單。
   - 標籤：所有素材的標籤清單。
4. 依序呼叫 `prompt` → `client` → `resolve`，回傳結果。

回傳型別（傳給前端）：

```rust
pub struct SuggestedValue {
    pub value: String,
    pub is_new: bool,
}

pub struct AiSuggestion {
    pub category: Option<SuggestedValue>,     // Avatar 類型固定為 None
    pub supported_avatars: Vec<SuggestedValue>, // 非 AvatarWearable 固定為空
    pub tags: Vec<SuggestedValue>,
}
```

錯誤種類 `AiSuggestErrorKind`（前端依種類顯示訊息）：
`NotConfigured`、`InvalidApiKey`（401 `authentication_error`、403 `permission_error`）、
`InsufficientCredit`（402 `billing_error`）、`RateLimited`（429）、`Overloaded`（529／5xx）、
`Timeout`、`Network`、`Refused`、`Truncated`、`InvalidResponse`（其他狀態碼也歸入此類）。
分類依 HTTP 狀態碼與回應中的 `error.type` 判斷，不比對錯誤訊息文字。

### 2.3 設定

`PreferenceStore` 新增兩個欄位：

```rust
#[serde(default)]
pub ai_suggest_enabled: bool,       // 預設 false
#[serde(default)]
pub ai_suggest_api_key: String,     // 預設空字串
```

- 設定檔版本**維持 6**。新欄位有 `#[serde(default)]`，舊的 v6 設定檔可直接讀取；
  原版 KonoAsset 讀到多出的欄位會忽略，因此切回原版也不會讀取失敗。
- `PreferenceStore::default()`、`overwrite()`，以及 `loader` 中由 `LegacyPreferenceStoreV5` 轉換的程式都要補上新欄位。
- `bindings.ts` 的 `PreferenceStore` 型別同步更新。

## 3. 畫面設計

### 3.1 載入中

- 進入填寫頁開始執行時，三個欄位上方顯示「AI 判斷中…」與載入圖示。
- 執行期間欄位仍可輸入，不會被鎖住。

### 3.2 不覆蓋使用者的輸入

- AI 結果只會填入**結果回來時仍為空白**的欄位。
- 前端記錄哪些欄位是由 AI 填入、且之後使用者沒有修改過。
- 變更素材類型後重新執行時，只會替換「AI 填入且未修改」的欄位，以及空白欄位。
- 若同一個 BOOTH 商品與素材類型已經執行過（例如在分頁之間來回切換），不會重複呼叫 API。

### 3.3 「新增」標記

- 現有名稱：顯示方式與手動選擇相同。
- 新名稱（`is_new = true`）：
  - 對應虛擬角色、標籤：在該項目的徽章上加上小的「新增」標記，並使用不同顏色。
    需要讓 `MultipleSelector`（`src/components/ui/multi-select.tsx`）新增一個選填 prop
    （例如 `highlightedValues?: string[]`），讓指定的值以標記樣式顯示。
  - 分類（單一欄位）：在欄位標題旁顯示「新增」標記。
- 接受：保留該項目，儲存後即成為正式項目。
- 拒絕：按 × 刪除（與現有的刪除方式相同）。使用者修改分類後，「新增」標記隨即消失。

### 3.4 錯誤提示

以 toast 顯示，不中斷流程，欄位維持原狀讓使用者手動填寫：

| 錯誤 | 訊息（zh-TW） |
|---|---|
| `InvalidApiKey` | API 金鑰無效或沒有權限，請到設定確認 |
| `InsufficientCredit` | Claude API 額度不足 |
| `RateLimited` | 請求太頻繁，請稍後再試 |
| `Overloaded` | Claude 服務忙碌中，請稍後再試 |
| `Timeout`、`Network`、`Refused`、`Truncated`、`InvalidResponse` | AI 判斷失敗，請手動填寫 |

`NotConfigured` 不顯示提示（前端在條件不成立時本來就不會呼叫）。

### 3.5 設定頁

在 `SettingsTab`（`src/pages/Preference/components/model/PreferenceTabs/tabs/SettingsTab`）新增「AI 輔助」區塊：

- 開關：啟用 AI 自動填入。
- API 金鑰輸入欄：預設以遮罩顯示，可按眼睛圖示切換顯示。
- 說明文字：說明會將 BOOTH 商品資訊與現有的分類、角色、標籤清單傳送給 Anthropic，
  以及費用由金鑰所屬的帳號支付。

所有新的介面文字都要加進每個語言檔（`src/locales/*.json`），包括 zh-TW；
並通過 `.github/workflows/scripts/locales-test.js` 的鍵值檢查。

## 4. 與 Claude API 的溝通

### 4.1 請求

- 端點：`POST https://api.anthropic.com/v1/messages`
- 標頭：`x-api-key`、`anthropic-version: 2026-10-21`、`content-type: application/json`
- 主要參數：

```json
{
  "model": "claude-haiku-5-5",
  "max_tokens": 4096,
  "output_config": {
    "effort": "low",
    "format": { "type": "json_schema", "schema": { "...": "見 4.2" } }
  },
  "system": "<規則說明>",
  "messages": [{ "role": "user", "content": "<BOOTH 資料與現有清單>" }]
}
```

- 不送 `temperature` 等取樣參數，也不使用 assistant prefill（目前的模型都會回傳 400）。
- 逾時：15 秒。逾時與網路錯誤不自動重試（使用者可以手動填寫，避免畫面一直等待）。

### 4.2 回傳格式（JSON Schema）

四種素材類型共用同一份 schema（固定不變，可利用 API 端的 schema 快取）：

```json
{
  "type": "object",
  "properties": {
    "category": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
    "supportedAvatars": { "type": "array", "items": { "type": "string" } },
    "tags": { "type": "array", "items": { "type": "string" } }
  },
  "required": ["category", "supportedAvatars", "tags"],
  "additionalProperties": false
}
```

### 4.3 送出的內容

**system（規則）**：

- 你是 VRChat 素材的分類助手，根據 BOOTH 商品資訊填寫分類、對應虛擬角色與標籤。
- 優先從提供的現有清單中選擇。名稱寫法不同但指的是同一個時（例如「Manuka」與「マヌカ」），必須使用清單中的寫法。
- 只有清單中沒有合適的項目時，才提出新名稱，並使用 BOOTH 上的寫法。
- 分類最多一個；對應虛擬角色列出所有適用的（商品規格名稱是重要線索）；標籤最多 5 個。
- 不適用於此素材類型的欄位要留空（`category` 為 null、陣列為空）。
- 無法判斷時寧可留空，不要猜測。

**user（資料）**：以標記區隔各段資料，例如：

```
<asset_type>AvatarWearable</asset_type>
<booth_item>
  <name>…</name>
  <shop>…</shop>
  <booth_category>…</booth_category>
  <booth_tags>…</booth_tags>
  <variations>…</variations>
  <description>…（全文，不截斷）</description>
</booth_item>
<existing_categories>…</existing_categories>
<existing_supported_avatars>…</existing_supported_avatars>
<existing_tags>…</existing_tags>
```

不適用於該素材類型的清單（例如 Avatar 類型的分類清單）不會送出。

### 4.4 回應處理

1. HTTP 非 2xx：依狀態碼對應到 2.2 的錯誤種類。
2. `stop_reason` 為 `refusal` → `Refused`；為 `max_tokens` → `Truncated`。
3. 略過 `thinking` 區塊（Haiku 5.5 預設會思考），取出第一個 `text` 區塊並以 JSON 解析；
   沒有 `text` 區塊或解析失敗 → `InvalidResponse`。
4. `resolve` 正規化與比對：
   - 去除前後空白，捨棄空字串，移除重複項目。
   - 依素材類型捨棄不適用的欄位。
   - 與現有清單比對時忽略大小寫（例如「modular avatar」對應到清單中的「Modular Avatar」），
     比對成功就改用清單中的寫法，並標示 `is_new = false`；否則 `is_new = true`。
   - 標籤超過 5 個時只保留前 5 個。

### 4.5 費用估算

每個素材約輸入 3,000～5,000 tokens、輸出數百 tokens。
以 Haiku 5.5 的價格（輸入 $0.10／輸出 $0.50 每百萬 tokens）計算，每個素材約台幣 0.02～0.05 元。

## 5. 測試

### 5.1 Rust 單元測試（不呼叫真正的 API）

- `booth`：以現有的 `test/6641548.json` 確認說明、標籤、規格名稱、分類名稱都能解析；
  缺少這些欄位的 JSON 也能正常解析。
- `ai_suggest::prompt`：產生的請求包含 BOOTH 資料、對應類型的清單、固定的 schema；不適用的清單不會出現。
- `ai_suggest::resolve`：
  - 現有名稱 → `is_new = false`；新名稱 → `is_new = true`。
  - 大小寫與前後空白不同時，會改用清單中的寫法。
  - 重複項目、空字串會被移除；標籤上限 5 個。
  - 依素材類型捨棄不適用的欄位。
- `ai_suggest::client`：以假的 transport 驗證 401、403、402、429、529、其他狀態碼、逾時、refusal、max_tokens、
  沒有 text 區塊、不合法 JSON 都對應到正確的錯誤種類。
- `loader`：沒有新欄位的 v6 設定檔可以讀取，且新欄位為預設值。

一律使用 `cargo test --workspace` 執行（單獨測試 `-p model` 會因 feature 統一而失敗）。

### 5.2 前端測試（vitest）

- 只會填入空白欄位。
- 變更素材類型後，只會替換「AI 填入且未修改」的欄位。
- 同一個商品與類型不會重複呼叫指令。
- 錯誤種類會對應到正確的提示訊息。

### 5.3 實際測試

最後使用使用者的金鑰，以 BOOTH 商品 9059890（edgemode）實際執行一次，確認結果合理。
費用不到台幣 1 毛；**執行前須先徵得使用者同意**。

## 6. 不在範圍內

- 自動下載與匯入（Feature B）。
- 「編輯素材」對話框中的 AI 填入（只做新增素材）。
- 讓 AI 判斷素材類型（沿用現有的 BOOTH 分類對應）。
- 將商品圖片傳給 AI。
- 選擇模型、測試金鑰按鈕、將金鑰存進作業系統的認證管理員。
