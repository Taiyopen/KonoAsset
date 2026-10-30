use std::time::Duration;

use super::definitions::ChangelogVersion;

const URL: &str = "https://releases.konoasset.dev/manifests/changelog.json";

pub async fn fetch_and_parse_changelog<S: AsRef<str>>(
    version: S,
) -> Result<Vec<ChangelogVersion>, String> {
    let changelog_body = fetch_changelogs(version).await?;
    let changelog = parse_changelog(changelog_body).await?;

    Ok(changelog)
}

async fn fetch_changelogs<S: AsRef<str>>(version: S) -> Result<String, String> {
    let client = get_reqwest_client(version)?;

    let response = client
        .get(URL)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch changelogs: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch changelogs: {}", response.status()));
    }

    response
        .text()
        .await
        .map_err(|e| format!("Failed to fetch changelogs: {}", e))
}

pub(crate) async fn parse_changelog<S>(text: S) -> Result<Vec<ChangelogVersion>, String>
where
    S: AsRef<str>,
{
    let changelog = serde_json::from_str(text.as_ref())
        .map_err(|e| format!("Failed to parse changelog: {}", e))?;

    Ok(changelog)
}

fn get_reqwest_client<S: AsRef<str>>(version: S) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(format!("KonoAsset/{}", version.as_ref()))
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("Failed to create reqwest client: {}", e))
}

#[cfg(test)]
mod tests {
    use model::preference::LanguageCode;

    use super::*;

    #[tokio::test]
    async fn test_changelog_json() {
        let text_changelog = include_str!("../../../../changelog.json");
        parse_changelog(text_changelog).await.unwrap();
    }

    #[tokio::test]
    async fn test_changelog_json_has_zh_tw_translations() {
        let text_changelog = include_str!("../../../../changelog.json");
        let changelog = parse_changelog(text_changelog).await.unwrap();

        for version in changelog {
            let entries = version
                .features
                .iter()
                .chain(version.fixes.iter())
                .chain(version.others.iter());

            for entry in entries {
                assert!(
                    entry
                        .langs
                        .iter()
                        .any(|lang| lang.lang.is_language_supported(&LanguageCode::ZhTw)),
                    "v{}: missing zh-TW translation for \"{}\"",
                    version.version,
                    entry.text
                );
            }
        }
    }
}
