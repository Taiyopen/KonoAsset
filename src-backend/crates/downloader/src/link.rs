use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, specta::Type, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SiteKind {
    Workupload,
    Pixeldrain,
    GoogleDrive,
    OneDrive,
    Direct,
}

impl SiteKind {
    /// Sites that must be downloaded one at a time (only Workupload).
    pub fn is_exclusive(self) -> bool {
        matches!(self, SiteKind::Workupload)
    }
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    File,
    Folder,
}

/// `id` is the full URL for `Direct` and `OneDrive`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub kind: JobKind,
    pub id: String,
    pub password: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkLine {
    pub line_number: u32,
    pub site: SiteKind,
    pub job: Job,
}

#[derive(Serialize, specta::Type, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLine {
    pub line_number: u32,
    pub text: String,
    pub site: Option<SiteKind>,
    pub kind: Option<JobKind>,
    pub has_password: bool,
    pub valid: bool,
}

static WORKUPLOAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"workupload\.com/(file|start|archive)/([A-Za-z0-9]+)").unwrap());
static PIXELDRAIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"pixeldrain\.(?:com|net)/(u|l)/([A-Za-z0-9]+)").unwrap());
static GDRIVE_FOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"drive\.google\.com/drive/(?:u/\d+/)?folders/([\w-]+)").unwrap());
static GDRIVE_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:drive|docs)\.google\.com/(?:file/d/|open\?id=|uc\?(?:[^ ]*&)?id=)([\w-]+)|drive\.usercontent\.google\.com/download\?(?:[^ ]*&)?id=([\w-]+)",
    )
    .unwrap()
});
static ONEDRIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://(?:1drv\.ms|onedrive\.live\.com|[\w-]+\.sharepoint\.com)/\S+").unwrap()
});
static DIRECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://\S+").unwrap());

fn normalize(line: &str) -> String {
    line.replace('\u{3000}', " ").trim().to_string()
}

/// Parses one line of the form `<url>[ <password>]`.
pub fn parse_line(line: &str) -> Option<(SiteKind, Job)> {
    let line = normalize(line);
    let mut tokens = line.split_whitespace();
    let url = tokens.next()?;
    let password = tokens.next().unwrap_or("").to_string();

    let job = |kind: JobKind, id: &str| Job {
        kind,
        id: id.to_string(),
        password: password.clone(),
    };

    if let Some(c) = WORKUPLOAD.captures(url) {
        let kind = if &c[1] == "archive" {
            JobKind::Folder
        } else {
            JobKind::File
        };
        return Some((SiteKind::Workupload, job(kind, &c[2])));
    }
    if let Some(c) = PIXELDRAIN.captures(url) {
        let kind = if &c[1] == "l" {
            JobKind::Folder
        } else {
            JobKind::File
        };
        return Some((SiteKind::Pixeldrain, job(kind, &c[2])));
    }
    if let Some(c) = GDRIVE_FOLDER.captures(url) {
        return Some((SiteKind::GoogleDrive, job(JobKind::Folder, &c[1])));
    }
    if let Some(c) = GDRIVE_FILE.captures(url) {
        let id = c.get(1).or_else(|| c.get(2))?.as_str();
        return Some((SiteKind::GoogleDrive, job(JobKind::File, id)));
    }
    if let Some(m) = ONEDRIVE.find(url) {
        return Some((SiteKind::OneDrive, job(JobKind::File, m.as_str())));
    }
    if let Some(m) = DIRECT.find(url) {
        return Some((SiteKind::Direct, job(JobKind::File, m.as_str())));
    }
    None
}

/// Parses every non-empty, non-comment (`#`) line. Line numbers are 1-based.
pub fn parse_text(text: &str) -> Vec<ParsedLine> {
    let mut result = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = normalize(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parsed = parse_line(&line);
        let has_password = parsed
            .as_ref()
            .is_some_and(|(_, job)| !job.password.is_empty());
        result.push(ParsedLine {
            line_number: index as u32 + 1,
            text: line,
            site: parsed.as_ref().map(|(site, _)| *site),
            kind: parsed.as_ref().map(|(_, job)| job.kind),
            has_password,
            valid: parsed.is_some(),
        });
    }
    result
}

/// Returns all lines as `LinkLine`, or `Err` if any line is invalid or there are none.
pub fn valid_lines(text: &str) -> Result<Vec<LinkLine>, String> {
    let lines = parse_text(text)
        .into_iter()
        .map(|parsed| {
            let line_number = parsed.line_number;
            let (site, job) = parse_line(&parsed.text)
                .ok_or_else(|| format!("Invalid link on line {line_number}"))?;
            Ok(LinkLine {
                line_number,
                site,
                job,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if lines.is_empty() {
        return Err("No links found".to_string());
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_each_site() {
        let cases = [
            (
                "https://workupload.com/file/DQTyYjAQqUJ huna",
                SiteKind::Workupload,
                JobKind::File,
                "DQTyYjAQqUJ",
                "huna",
            ),
            (
                "https://workupload.com/archive/q2hv3vcBcy 12345",
                SiteKind::Workupload,
                JobKind::Folder,
                "q2hv3vcBcy",
                "12345",
            ),
            (
                "https://pixeldrain.com/u/pHP3XpbB",
                SiteKind::Pixeldrain,
                JobKind::File,
                "pHP3XpbB",
                "",
            ),
            (
                "https://pixeldrain.net/l/abc123",
                SiteKind::Pixeldrain,
                JobKind::Folder,
                "abc123",
                "",
            ),
            (
                "https://drive.google.com/drive/folders/1AbC-_x",
                SiteKind::GoogleDrive,
                JobKind::Folder,
                "1AbC-_x",
                "",
            ),
            (
                "https://drive.google.com/file/d/1XyZ/view?usp=sharing",
                SiteKind::GoogleDrive,
                JobKind::File,
                "1XyZ",
                "",
            ),
            (
                "https://drive.usercontent.google.com/download?export=download&id=1Q",
                SiteKind::GoogleDrive,
                JobKind::File,
                "1Q",
                "",
            ),
            (
                "https://1drv.ms/u/s!Abc",
                SiteKind::OneDrive,
                JobKind::File,
                "https://1drv.ms/u/s!Abc",
                "",
            ),
            (
                "https://example.com/a.zip",
                SiteKind::Direct,
                JobKind::File,
                "https://example.com/a.zip",
                "",
            ),
        ];
        for (line, site, kind, id, pw) in cases {
            let (s, job) = parse_line(line).unwrap_or_else(|| panic!("{line}"));
            assert_eq!(
                (s, job.kind, job.id.as_str(), job.password.as_str()),
                (site, kind, id, pw),
                "{line}"
            );
        }
    }

    #[test]
    fn parse_handles_crlf_and_fullwidth_space() {
        let lines = valid_lines("  https://workupload.com/file/abc\u{3000}密碼  \r\n\r\n# comment\r\nhttps://pixeldrain.com/u/x\r\n").unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            (lines[0].line_number, lines[0].job.password.as_str()),
            (1, "密碼")
        );
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
}
