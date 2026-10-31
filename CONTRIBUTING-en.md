# KonoAsset 貢獻指南

感謝你有興趣為 KonoAsset 做出貢獻。

本文件說明為了維持穩定的開發體驗，希望大家遵守的事項。  
此外，參與貢獻時請務必遵守 [Code of Conduct](CODE_OF_CONDUCT.md)。


## 關於使用語言

本專案以日文作為主要語言。  
但這**並不**代表你必須用日文溝通。

你可以用自己的母語撰寫 Issue 或 PR，回覆日文留言時也不需要使用日文。  
如此一來，讀者可以自由使用翻譯工具，盡可能準確地理解你的意思。


## 貢獻方式

為 KonoAsset 做出貢獻不一定要修改程式碼。例如：

- 撰寫程式碼並建立 Pull Request
- 建立回報錯誤或功能需求的 Issue
- 建立或修正翻譯檔案
- 撰寫或改善文件
- 回答 [Discussions](https://github.com/siloneco/KonoAsset/discussions) 中的提問

貢獻的形式有很多種。


### Issues

修改程式碼之前，建議先建立 [Issue](https://github.com/siloneco/KonoAsset/issues)，討論要做哪些變更。  
與程式碼變更無關的提問或疑難排解，請到 [Discussions](https://github.com/siloneco/KonoAsset/discussions) 進行。

著手處理某個 Issue 時，請將自己指派為負責人，以避免工作重複。  
如果你沒有指派的權限，請留言表明你正在處理。

> [!WARNING]
> 建立新的 Issue 之前，請先確認是否已有重複的 Issue。  
> 此外，即使實作方向已經決定，在程式碼完全合併之前也請不要關閉 Issue。


### Pull Requests

建立 Pull Request 時，請遵守以下事項：

1. 須有與該變更相關的 Issue 或 Security Advisory（沒有的話請先建立）
2. 為了方便審查，請將變更控制在最小範圍
3. （盡可能）在分支名稱加上前綴（`feat/`、`fix/`、`perf/`、`docs/` 等）
4. （盡可能）如有 UI 變更，請附上螢幕截圖

語言檔案新增項目時，你不需要自己填完所有語言檔案。  
建立 PR 後，可以請熟悉該語言的人協助製作翻譯檔案。


### 安全性相關回報

如果你發現漏洞或相關的錯誤，請透過 [GitHub Security Advisories](https://github.com/siloneco/KonoAsset/security/advisories/new) 回報問題。  
如果你打算自己提交修正，請依情況採取以下做法：

- 若認為在修正發布前公開 PoC（Proof of Concept，概念驗證）有危險
  - 請在 [Private Fork](https://docs.github.com/zh/code-security/security-advisories/working-with-repository-security-advisories/collaborating-in-a-temporary-private-fork-to-resolve-a-repository-security-vulnerability) 等地方建立修正的 commit，並分享給貢獻者
- 若是在修正發布前公開 PoC 也沒有問題的輕微漏洞
  - 請依照一般建立 PR 的流程進行
- 若無法判斷屬於哪一種
  - 請不要公開，改為提交到 Private Fork


### 授權

你的貢獻會以與本專案相同的授權條款公開。  
詳情請參閱 [LICENSE](LICENSE) 檔案。


## 設定開發環境

請安裝以下工具：

- [Rust](https://www.rust-lang.org/tools/install)
- [Node.js (v23)](https://nodejs.org/en/download/package-manager)
- [pnpm](https://pnpm.io/installation)（`npm i -g pnpm`）

建議使用 [VSCode](https://code.visualstudio.com/) 作為編輯器，但你也可以依喜好使用其他編輯器。


### 安裝前端套件

使用以下指令安裝 Node.js 套件

```
pnpm i
```

Rust 套件會在執行 Tauri 時自動安裝


### 以開發模式執行

執行 `pnpm tauri dev` 即可啟動應用程式。

修改前端程式碼後會自動更新。  
如果沒有反映出來，可以在應用程式中按 `Ctrl-R` 重新載入整個畫面，通常就能解決。  

修改後端程式碼後，應用程式會自動重新啟動。

> [!NOTE]
> 修改後端程式碼時，每次存檔都會執行一次開發版建置，會讓開發體驗變差，建議只在需要測試時才執行。


### 程式碼分析（前端）

使用以下指令可以分析前端程式碼。

```bash
# 執行格式化
pnpm run fmt

# 檢查 Lint 錯誤
pnpm run lint

# TypeScript 型別檢查
pnpm run type-check
```


### 建置

使用以下指令可以建立 exe 執行檔。

```
pnpm tauri build --no-bundle
```
