//! Google Drive: public files (including the "can't scan for viruses" confirmation page of large
//! files) and public folders, using plain HTTP only.
//!
//! * File: `GET <download_base>/download?id=<id>&export=download&confirm=t`. Large files answer
//!   with an HTML page holding a `download-form`; submitting it (a GET) gives the file.
//! * Folder: the `embeddedfolderview` page lists one level of the folder; sub-folders are
//!   fetched recursively.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use reqwest::header::CONTENT_TYPE;

use crate::error::DownloadError;
use crate::http::{filename_from_headers, redact_url, safe_name};
use crate::link::{Job, JobKind};
use crate::site::{Ctx, FileItem, FileProgress, Site};

use super::{check_status, save_named};

pub const DOWNLOAD_BASE: &str = "https://drive.usercontent.google.com";
pub const FOLDER_BASE: &str = "https://drive.google.com";

const NOT_PUBLIC: &str = "folder is not shared publicly";
/// Safety net against folder shortcuts that point back up the tree.
const MAX_FOLDERS: usize = 1000;

static RE_FORM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)(<form\b[^>]*\bid\s*=\s*["']download-form["'][^>]*>)(.*?)</form>"#).unwrap());
static RE_INPUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<input\b[^>]*>").unwrap());
static RE_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\b([a-z][a-z0-9_-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap());
static RE_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());
static RE_TITLE_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s[-\u{2013}]\sGoogle\s\S+$").unwrap());
static RE_ENTRY_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)class="flip-entry-title"[^>]*>(.*?)</div>"#).unwrap());
static RE_HREF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<a\b[^>]*\bhref\s*=\s*"([^"]*)""#).unwrap());
static RE_FILE_HREF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^https://drive\.google\.com/file/d/([\w-]+)").unwrap());
static RE_FOLDER_HREF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^https://drive\.google\.com/drive/(?:u/\d+/)?folders/([\w-]+)").unwrap());
static RE_ENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);").unwrap());

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FolderEntry {
    pub id: String,
    pub name: String,
    pub is_folder: bool,
}

pub struct GoogleDrive {
    download_base: String,
    folder_base: String,
}

impl GoogleDrive {
    pub fn new() -> Self {
        Self::with_bases(DOWNLOAD_BASE.to_string(), FOLDER_BASE.to_string())
    }

    /// Different base urls, for tests against a local server.
    pub fn with_bases(download_base: String, folder_base: String) -> Self {
        Self { download_base, folder_base }
    }
}

impl Default for GoogleDrive {
    fn default() -> Self {
        Self::new()
    }
}

/// Decodes the entities Google puts into names (`&amp;`, `&#39;`, `&#x4e2d;`, ...).
fn html_unescape(text: &str) -> String {
    RE_ENTITY
        .replace_all(text, |c: &regex::Captures| {
            let entity = &c[1];
            let decoded = if let Some(hex) = entity.strip_prefix("#x") {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = entity.strip_prefix('#') {
                dec.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    _ => None,
                }
            };
            decoded.map_or_else(|| c[0].to_string(), String::from)
        })
        .into_owned()
}

fn attr(tag: &str, name: &str) -> Option<String> {
    RE_ATTR
        .captures_iter(tag)
        .find(|c| c[1].eq_ignore_ascii_case(name))
        .map(|c| html_unescape(c.get(2).or_else(|| c.get(3)).map_or("", |m| m.as_str())))
}

/// The url the "Download anyway" form of a large file's confirmation page leads to: the form's
/// `action` plus all its hidden inputs as query parameters.
#[cfg(test)]
pub(crate) fn confirm_form_url(html: &str) -> Option<String> {
    confirm_form_url_at(html, DOWNLOAD_BASE)
}

/// [`confirm_form_url`], resolving a relative form `action` against `base`.
fn confirm_form_url_at(html: &str, base: &str) -> Option<String> {
    let form = RE_FORM.captures(html)?;
    let action = attr(&form[1], "action")?;
    let mut url = reqwest::Url::parse(&action)
        .or_else(|_| reqwest::Url::parse(base).and_then(|b| b.join(&action)))
        .ok()?;
    {
        let mut query = url.query_pairs_mut();
        for input in RE_INPUT.find_iter(&form[2]) {
            let input = input.as_str();
            let is_hidden = attr(input, "type").is_some_and(|t| t.eq_ignore_ascii_case("hidden"));
            if let (true, Some(name)) = (is_hidden, attr(input, "name")) {
                query.append_pair(&name, &attr(input, "value").unwrap_or_default());
            }
        }
    }
    Some(url.to_string())
}

/// Drive's "Too many users have viewed or downloaded this file recently" page.
pub(crate) fn is_quota_page(html: &str) -> bool {
    let lower = html.to_lowercase();
    lower.contains("too many users") || lower.contains("quota")
}

/// `(folder title, entries)` of an `embeddedfolderview` page. Entries that are neither a Drive
/// file nor a Drive folder (Google Docs, Sheets, shortcuts, ...) cannot be downloaded as they are
/// and are left out.
pub(crate) fn parse_folder_listing(html: &str) -> (Option<String>, Vec<FolderEntry>) {
    let title = RE_TITLE
        .captures(html)
        .map(|c| html_unescape(c[1].trim()))
        .map(|t| RE_TITLE_SUFFIX.replace(&t, "").trim().to_string())
        .filter(|t| !t.is_empty());

    let mut entries = Vec::new();
    // Every entry starts with `<div class="flip-entry"` (the other `flip-entry-*` classes differ).
    for segment in html.split(r#"<div class="flip-entry""#).skip(1) {
        let Some(href) = RE_HREF.captures(segment).map(|c| html_unescape(&c[1])) else { continue };
        let Some(name) = RE_ENTRY_TITLE.captures(segment).map(|c| html_unescape(c[1].trim())) else { continue };
        if let Some(c) = RE_FOLDER_HREF.captures(&href) {
            entries.push(FolderEntry { id: c[1].to_string(), name, is_folder: true });
        } else if let Some(c) = RE_FILE_HREF.captures(&href) {
            entries.push(FolderEntry { id: c[1].to_string(), name, is_folder: false });
        }
    }
    (title, entries)
}

fn is_login_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .is_some_and(|h| h.starts_with("accounts.google."))
}

/// Sends the request; maps error statuses and a redirect to the sign-in page to errors.
async fn send(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, DownloadError> {
    let response = client.get(url).send().await?;
    check_status(response.status(), url)?;
    if is_login_url(response.url().as_str()) {
        return Err(DownloadError::LoginRequired(format!("{} needs a login or is not shared publicly", redact_url(url))));
    }
    Ok(response)
}

enum Fetched {
    /// The response is the file; its body has not been read.
    File { url: String, name: Option<String>, size: Option<u64> },
    Page(String),
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Fetched, DownloadError> {
    let response = send(client, url).await?;
    let name = filename_from_headers(response.headers());
    let is_html = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("text/html"));
    // An html file that is really downloaded comes with a file name; a page does not.
    if is_html && name.is_none() {
        return Ok(Fetched::Page(response.text().await?));
    }
    Ok(Fetched::File { url: response.url().to_string(), name, size: response.content_length() })
}

impl GoogleDrive {
    /// Follows the confirmation page of large files; returns the url to save and the file's
    /// name and size from the headers.
    async fn resolve_file(
        &self,
        client: &reqwest::Client,
        id: &str,
    ) -> Result<(String, Option<String>, Option<u64>), DownloadError> {
        let not_a_file = || DownloadError::NotAFile(format!("{id} is a web page, not a file"));
        let rate_limited = || {
            DownloadError::RateLimited(format!(
                "too many downloads of this file recently (https://drive.google.com/file/d/{id}/view)"
            ))
        };
        let url = format!("{}/download?id={id}&export=download&confirm=t", self.download_base);
        let html = match fetch(client, &url).await? {
            Fetched::File { url, name, size } => return Ok((url, name, size)),
            Fetched::Page(html) => html,
        };
        // The confirmation page comes first: its file name may contain "quota".
        let Some(confirm_url) = confirm_form_url_at(&html, &self.download_base) else {
            return Err(if is_quota_page(&html) { rate_limited() } else { not_a_file() });
        };
        match fetch(client, &confirm_url).await? {
            Fetched::File { url, name, size } => Ok((url, name, size)),
            Fetched::Page(html) if is_quota_page(&html) => Err(rate_limited()),
            Fetched::Page(_) => Err(not_a_file()),
        }
    }

    async fn folder_page(&self, client: &reqwest::Client, id: &str) -> Result<String, DownloadError> {
        let url = format!("{}/embeddedfolderview?id={id}", self.folder_base);
        match send(client, &url).await {
            Ok(response) => Ok(response.text().await?),
            Err(DownloadError::NotFound(_)) => Err(DownloadError::LoginRequired(NOT_PUBLIC.into())),
            Err(e) => Err(e),
        }
    }

    async fn list_folder(
        &self,
        client: &reqwest::Client,
        root: &str,
    ) -> Result<(Option<String>, Vec<FileItem>), DownloadError> {
        let mut title = None;
        let mut items = Vec::new();
        let mut seen = HashSet::from([root.to_string()]);
        // (folder id, subdir of its files), breadth first.
        let mut queue = VecDeque::from([(root.to_string(), String::new())]);
        while let Some((id, subdir)) = queue.pop_front() {
            let html = self.folder_page(client, &id).await?;
            let (page_title, entries) = parse_folder_listing(&html);
            if id == root {
                // An empty folder cannot be told apart from a page that is not a folder
                // (a file id), and has nothing to download either way.
                if !html.contains("flip-entries") || entries.is_empty() {
                    return Err(DownloadError::LoginRequired(NOT_PUBLIC.into()));
                }
                title = page_title;
            }
            for entry in entries {
                if entry.is_folder {
                    if seen.len() >= MAX_FOLDERS {
                        return Err(DownloadError::Fatal(format!("more than {MAX_FOLDERS} folders")));
                    }
                    if seen.insert(entry.id.clone()) {
                        let name = safe_name(&entry.name);
                        let child = if subdir.is_empty() { name } else { format!("{subdir}\\{name}") };
                        queue.push_back((entry.id, child));
                    }
                } else {
                    items.push(FileItem { id: entry.id, subdir: subdir.clone(), name: Some(entry.name) });
                }
            }
        }
        if items.is_empty() {
            return Err(DownloadError::Fatal("the folder has no downloadable files".into()));
        }
        Ok((title, items))
    }
}

#[async_trait::async_trait]
impl Site for GoogleDrive {
    async fn list_files(&mut self, ctx: &Ctx, job: &Job) -> Result<(Option<String>, Vec<FileItem>), DownloadError> {
        match job.kind {
            JobKind::File => Ok((None, vec![FileItem { id: job.id.clone(), subdir: String::new(), name: None }])),
            JobKind::Folder => self.list_folder(&ctx.http, &job.id).await,
        }
    }

    async fn download(
        &mut self,
        ctx: &Ctx,
        item: &FileItem,
        _password: &str,
        out_dir: &Path,
        progress: FileProgress<'_>,
    ) -> Result<PathBuf, DownloadError> {
        let (url, header_name, size) = self.resolve_file(&ctx.http, &item.id).await?;
        let header_name_missing = header_name.is_none();
        let name = header_name.or_else(|| item.name.clone()).unwrap_or_else(|| item.id.clone());
        // Without a file name in the headers the answer could be a page again (a used-up
        // confirmation link, the quota page); never save that as the file.
        save_named(&ctx.http, &url, &name, size, out_dir, progress, header_name_missing).await
    }

    fn delay(&self) -> Duration {
        Duration::from_secs(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client;
    use crate::test_server::{serve, Route};

    const CONFIRM: &str = include_str!("../../test/gdrive_confirm.html");
    const QUOTA: &str = include_str!("../../test/gdrive_quota.html");
    const FOLDER: &str = include_str!("../../test/gdrive_folder.html");

    #[test]
    fn parses_confirm_form() {
        assert_eq!(
            confirm_form_url(CONFIRM).unwrap(),
            "https://drive.usercontent.google.com/download?id=1AbC&export=download&confirm=t&uuid=u-1"
        );
    }

    #[test]
    fn detects_quota_page() {
        assert!(is_quota_page(QUOTA));
        assert!(!is_quota_page(CONFIRM));
    }

    #[test]
    fn parses_folder_listing() {
        let (title, entries) = parse_folder_listing(FOLDER);
        assert_eq!(title.as_deref(), Some("素材パック"));
        assert_eq!(
            entries,
            vec![
                FolderEntry { id: "1file".into(), name: "a.zip".into(), is_folder: false },
                FolderEntry { id: "1sub".into(), name: "Textures".into(), is_folder: true },
            ]
        );
    }

    #[test]
    fn folder_listing_decodes_entities_and_plain_title() {
        let html = r#"<title>Tom &amp; Jerry</title><div class="flip-entries"><div class="flip-entry" id="entry-1x"><div class="flip-entry-info"><a href="https://drive.google.com/file/d/1x/view?usp=drive_web" target="_blank"><div class="flip-entry-title">a &amp; b&#39;s.zip</div></a></div></div></div>"#;
        let (title, entries) = parse_folder_listing(html);
        assert_eq!(title.as_deref(), Some("Tom & Jerry"));
        assert_eq!(entries, vec![FolderEntry { id: "1x".into(), name: "a & b's.zip".into(), is_folder: false }]);
    }

    #[test]
    fn confirm_form_without_form_is_none() {
        assert_eq!(confirm_form_url("<html><body>nothing</body></html>"), None);
        assert_eq!(confirm_form_url(QUOTA), None);
    }

    #[test]
    fn sign_in_pages_are_recognised() {
        assert!(is_login_url("https://accounts.google.com/ServiceLogin?continue=x"));
        assert!(!is_login_url("https://drive.usercontent.google.com/download?id=1"));
    }

    fn html_route(path: &str, body: &str) -> Route {
        let mut route = Route::ok(path, body.as_bytes().to_vec());
        route.headers.push(("Content-Type".into(), "text/html; charset=utf-8".into()));
        route
    }

    fn file_route(path: &str, body: Vec<u8>, name: &str) -> Route {
        let mut route = Route::ok(path, body);
        route.headers.push(("Content-Disposition".into(), format!("attachment; filename=\"{name}\"")));
        route
    }

    fn payload() -> Vec<u8> {
        (0..5_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn ctx() -> Ctx {
        Ctx { app: None, http: client() }
    }

    async fn download_one(base: &str, id: &str, dir: &Path) -> Result<PathBuf, DownloadError> {
        let mut site = GoogleDrive::with_bases(base.to_string(), base.to_string());
        let item = FileItem { id: id.into(), subdir: String::new(), name: None };
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        site.download(&ctx(), &item, "", dir, &noop).await
    }

    #[tokio::test]
    async fn small_file_downloads_directly() {
        let base = serve(vec![file_route("/download", payload(), "a.zip")]).await;
        let dir = tempfile::tempdir().unwrap();
        let path = download_one(&base, "1AbC", dir.path()).await.unwrap();
        assert_eq!(path, dir.path().join("a.zip"));
        assert_eq!(std::fs::read(path).unwrap(), payload());
    }

    #[tokio::test]
    async fn large_file_goes_through_confirm_page() {
        // The form's relative action resolves against the (injected) download base.
        let form = r#"<form id="download-form" action="/confirmed" method="get"><input type="hidden" name="id" value="1AbC"><input type="hidden" name="uuid" value="u-1"></form>"#;
        let base = serve(vec![html_route("/download", form), file_route("/confirmed", payload(), "big.zip")]).await;
        let dir = tempfile::tempdir().unwrap();
        let path = download_one(&base, "1AbC", dir.path()).await.unwrap();
        assert_eq!(path, dir.path().join("big.zip"));
        assert_eq!(std::fs::read(path).unwrap(), payload());
    }

    #[tokio::test]
    async fn confirm_page_for_file_named_quota_is_not_rate_limited() {
        let confirm = CONFIRM.replace("big-pack.zip", "quota-report.zip");
        let form = confirm.replace("https://drive.usercontent.google.com/download", "/confirmed");
        let base = serve(vec![html_route("/download", &form), file_route("/confirmed", payload(), "quota-report.zip")]).await;
        let dir = tempfile::tempdir().unwrap();
        let path = download_one(&base, "1AbC", dir.path()).await.unwrap();
        assert_eq!(path, dir.path().join("quota-report.zip"));
    }

    #[tokio::test]
    async fn html_on_the_final_get_is_never_saved_as_the_file() {
        // The first answer is a nameless "file"; the second GET (the one that is saved) gets a
        // page, e.g. a used-up confirmation link.
        let mut route = Route::ok("/download", payload());
        route.then = Some(Box::new(html_route("/download", "<html>please confirm</html>")));
        let base = serve(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let item = FileItem { id: "1AbC".into(), subdir: String::new(), name: Some("a.zip".into()) };
        let mut site = GoogleDrive::with_bases(base.clone(), base);
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let err = site.download(&ctx(), &item, "", dir.path(), &noop).await.unwrap_err();
        assert!(matches!(err, DownloadError::NotAFile(_)), "{err:?}");
        assert!(!dir.path().join("a.zip").exists());
        assert!(!dir.path().join("a.zip.part").exists());
    }

    #[tokio::test]
    async fn named_html_file_is_still_downloaded() {
        let mut route = Route::ok("/download", b"<html>my page</html>".to_vec());
        route.headers.push(("Content-Type".into(), "text/html".into()));
        route.headers.push(("Content-Disposition".into(), "attachment; filename=\"page.html\"".into()));
        let base = serve(vec![route]).await;
        let dir = tempfile::tempdir().unwrap();
        let path = download_one(&base, "1AbC", dir.path()).await.unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"<html>my page</html>");
    }

    #[tokio::test]
    async fn quota_page_is_rate_limited() {
        let base = serve(vec![html_route("/download", QUOTA)]).await;
        let dir = tempfile::tempdir().unwrap();
        let err = download_one(&base, "1AbC", dir.path()).await.unwrap_err();
        // The user is told to download it in a browser, so the page is part of the message.
        assert_eq!(
            err,
            DownloadError::RateLimited(
                "too many downloads of this file recently (https://drive.google.com/file/d/1AbC/view)".into()
            )
        );
    }

    #[tokio::test]
    async fn unknown_html_page_is_not_a_file() {
        let base = serve(vec![html_route("/download", "<html><body>hello</body></html>")]).await;
        let dir = tempfile::tempdir().unwrap();
        let err = download_one(&base, "1AbC", dir.path()).await.unwrap_err();
        assert!(matches!(err, DownloadError::NotAFile(_)), "{err:?}");
    }

    #[tokio::test]
    async fn confirm_page_leading_to_another_page_is_not_a_file() {
        let form = r#"<form id="download-form" action="/again" method="get"><input type="hidden" name="id" value="1"></form>"#;
        let base = serve(vec![html_route("/download", form), html_route("/again", "<html>again</html>")]).await;
        let dir = tempfile::tempdir().unwrap();
        let err = download_one(&base, "1AbC", dir.path()).await.unwrap_err();
        assert!(matches!(err, DownloadError::NotAFile(_)), "{err:?}");
    }

    #[tokio::test]
    async fn missing_file_is_not_found() {
        let base = serve(vec![]).await;
        let dir = tempfile::tempdir().unwrap();
        let err = download_one(&base, "1AbC", dir.path()).await.unwrap_err();
        assert!(matches!(err, DownloadError::NotFound(_)), "{err:?}");
    }

    #[tokio::test]
    async fn listed_name_is_used_when_headers_have_none() {
        let base = serve(vec![Route::ok("/download", payload())]).await;
        let dir = tempfile::tempdir().unwrap();
        let mut site = GoogleDrive::with_bases(base.clone(), base);
        let item = FileItem { id: "1AbC".into(), subdir: String::new(), name: Some("from:list.zip".into()) };
        let noop = |_: &str, _: u64, _: Option<u64>| {};
        let path = site.download(&ctx(), &item, "", dir.path(), &noop).await.unwrap();
        assert_eq!(path, dir.path().join("from_list.zip"));
    }

    fn folder_job(id: &str) -> Job {
        Job { kind: JobKind::Folder, id: id.into(), password: String::new() }
    }

    #[tokio::test]
    async fn folder_listing_recurses_into_subfolders() {
        // The test server ignores the query, so every folder shows the same page: the root
        // contributes a.zip at the top, and the sub-folder `1sub` (visited once) contributes it
        // again under Textures.
        let base = serve(vec![html_route("/embeddedfolderview", FOLDER)]).await;
        let mut site = GoogleDrive::with_bases(base.clone(), base);
        let (title, items) = site.list_files(&ctx(), &folder_job("root")).await.unwrap();
        assert_eq!(title.as_deref(), Some("素材パック"));
        assert_eq!(
            items,
            vec![
                FileItem { id: "1file".into(), subdir: String::new(), name: Some("a.zip".into()) },
                FileItem { id: "1file".into(), subdir: "Textures".into(), name: Some("a.zip".into()) },
            ]
        );
    }

    #[tokio::test]
    async fn nested_subdirs_are_sanitised_per_component() {
        let entry = |href: &str, name: &str| {
            format!(r#"<div class="flip-entry"><a href="{href}"><div class="flip-entry-title">{name}</div></a></div>"#)
        };
        let body = format!(
            r#"<title>t</title><div class="flip-entries">{}{}</div>"#,
            entry("https://drive.google.com/drive/folders/fa", "A:b"),
            entry("https://drive.google.com/file/d/f1/view", "x.bin"),
        );
        let base = serve(vec![html_route("/embeddedfolderview", &body)]).await;
        let mut site = GoogleDrive::with_bases(base.clone(), base);
        let (_, items) = site.list_files(&ctx(), &folder_job("root")).await.unwrap();
        assert_eq!(items.iter().map(|i| i.subdir.as_str()).collect::<Vec<_>>(), vec!["", "A_b"]);
    }

    #[tokio::test]
    async fn folder_404_is_login_required() {
        let base = serve(vec![]).await;
        let mut site = GoogleDrive::with_bases(base.clone(), base);
        let err = site.list_files(&ctx(), &folder_job("root")).await.unwrap_err();
        assert_eq!(err, DownloadError::LoginRequired(NOT_PUBLIC.into()));
    }

    #[tokio::test]
    async fn page_without_entries_container_is_login_required() {
        let base = serve(vec![html_route("/embeddedfolderview", "<html><title>x</title></html>")]).await;
        let mut site = GoogleDrive::with_bases(base.clone(), base);
        let err = site.list_files(&ctx(), &folder_job("root")).await.unwrap_err();
        assert_eq!(err, DownloadError::LoginRequired(NOT_PUBLIC.into()));
    }

    #[tokio::test]
    async fn empty_folder_listing_is_login_required() {
        let page = r#"<title>f</title><div class="flip-entries"></div>"#;
        let base = serve(vec![html_route("/embeddedfolderview", page)]).await;
        let mut site = GoogleDrive::with_bases(base.clone(), base);
        let err = site.list_files(&ctx(), &folder_job("root")).await.unwrap_err();
        assert_eq!(err, DownloadError::LoginRequired(NOT_PUBLIC.into()));
    }

    #[tokio::test]
    async fn file_job_lists_itself() {
        let mut site = GoogleDrive::new();
        let job = Job { kind: JobKind::File, id: "1AbC".into(), password: String::new() };
        let (folder, items) = site.list_files(&ctx(), &job).await.unwrap();
        assert_eq!(folder, None);
        assert_eq!(items, vec![FileItem { id: "1AbC".into(), subdir: String::new(), name: None }]);
    }

    #[test]
    fn pauses_two_seconds_between_files() {
        assert_eq!(GoogleDrive::new().delay(), Duration::from_secs(2));
    }
}
