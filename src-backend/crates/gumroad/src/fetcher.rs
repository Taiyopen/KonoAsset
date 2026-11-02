use chrono::DateTime;

use crate::{
    GumroadAssetInfo, GumroadInfoFetchError, client::get_reqwest_client,
    definitions::GumroadJsonSchema, is_gumroad_url, to_json_url, url::to_canonical_url,
};

pub struct GumroadFetcher {
    reqwest_client: reqwest::Client,
}

impl GumroadFetcher {
    pub fn new<S: AsRef<str>>(version: S) -> Self {
        Self {
            reqwest_client: get_reqwest_client(version).expect("Failed to create reqwest client"),
        }
    }

    pub async fn fetch(&self, url: &str) -> Result<GumroadAssetInfo, GumroadInfoFetchError> {
        let json_url = to_json_url(url)?;
        let response = self.reqwest_client.get(&json_url).send().await?;

        if response.status().as_u16() == 404 {
            return Err(GumroadInfoFetchError::NotFound(url.to_string()));
        }

        let body = response.text().await?;
        let mut info = parse_response(&body)?;
        info.url = canonical_url(info.url, url)?;

        Ok(info)
    }
}

/// 保存・リンク・ショートカットに使うため、Gumroad の商品 URL として妥当な値だけを採用する
/// （独自ドメインなどで妥当でない場合は入力された URL を正規化して使う）
fn canonical_url(returned: String, input: &str) -> Result<String, GumroadInfoFetchError> {
    if is_gumroad_url(&returned) {
        return Ok(returned);
    }

    to_canonical_url(input)
}

fn parse_response(body: &str) -> Result<GumroadAssetInfo, GumroadInfoFetchError> {
    let response: GumroadJsonSchema = serde_json::from_str(body)?;
    let published_at = DateTime::parse_from_rfc3339(&response.created_at)?.timestamp_millis();

    let mut image_urls: Vec<String> = response
        .covers
        .into_iter()
        .filter(|cover| cover.cover_type == "image")
        .filter_map(|cover| cover.url)
        .collect();

    if image_urls.is_empty() {
        image_urls.extend(response.thumbnail_url);
    }

    Ok(GumroadAssetInfo {
        url: response.url,
        name: response.name,
        creator: response.seller.name,
        image_urls,
        published_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_url_keeps_returned_gumroad_url() {
        assert_eq!(
            canonical_url(
                "https://bananabread.gumroad.com/l/ajfrt".to_string(),
                "https://gumroad.com/l/ajfrt?x=1"
            )
            .unwrap(),
            "https://bananabread.gumroad.com/l/ajfrt"
        );
    }

    #[test]
    fn test_canonical_url_falls_back_to_input_for_non_gumroad_url() {
        assert_eq!(
            canonical_url(
                "javascript:alert(1)\r\nIconFile=\\\\host\\share".to_string(),
                "https://gumroad.com/l/ajfrt?x=1"
            )
            .unwrap(),
            "https://gumroad.com/l/ajfrt"
        );
    }

    #[test]
    fn test_parse_skips_covers_without_url() {
        let body = body_with(
            r#", "covers": [
                { "type": "image", "url": null },
                { "type": "image" },
                { "type": "image", "url": "https://public-files.gumroad.com/image" }
            ]"#,
        );
        let info = parse_response(&body).unwrap();

        assert_eq!(
            info.image_urls,
            vec!["https://public-files.gumroad.com/image"]
        );
    }

    #[test]
    fn test_parse_fixture() {
        let body = std::fs::read_to_string("test/ajfrt.json").unwrap();
        let info = parse_response(&body).unwrap();

        assert_eq!(info.url, "https://bananabread.gumroad.com/l/ajfrt");
        assert_eq!(info.name, "Midnight Studio - VRChat World PC & Quest");
        assert_eq!(info.creator, "Banana's Assets for VRChat");
        assert_eq!(
            info.image_urls[0],
            "https://public-files.gumroad.com/3padttruejf2sb7rqp2w2hh0o7vx"
        );
        assert_eq!(info.image_urls.len(), 2);
        assert_eq!(info.published_at, 1687351822000);
    }

    fn body_with(images: &str) -> String {
        format!(
            r#"{{
                "url": "https://bananabread.gumroad.com/l/ajfrt",
                "name": "name",
                "seller": {{ "name": "seller" }},
                "created_at": "2023-06-21T12:50:22Z"
                {images}
            }}"#
        )
    }

    #[test]
    fn test_parse_falls_back_to_thumbnail() {
        let body =
            body_with(r#", "covers": [], "thumbnail_url": "https://public-files.gumroad.com/x""#);
        let info = parse_response(&body).unwrap();

        assert_eq!(info.image_urls, vec!["https://public-files.gumroad.com/x"]);
    }

    #[test]
    fn test_parse_no_images() {
        let body = body_with(r#", "thumbnail_url": null"#);
        let info = parse_response(&body).unwrap();

        assert!(info.image_urls.is_empty());
    }

    #[test]
    fn test_parse_skips_non_image_covers() {
        let body = body_with(
            r#", "covers": [
                { "type": "video", "url": "https://public-files.gumroad.com/video" },
                { "type": "image", "url": "https://public-files.gumroad.com/image" }
            ]"#,
        );
        let info = parse_response(&body).unwrap();

        assert_eq!(
            info.image_urls,
            vec!["https://public-files.gumroad.com/image"]
        );
    }

    #[test]
    fn test_parse_missing_name_fails() {
        let body = r#"{
            "url": "https://bananabread.gumroad.com/l/ajfrt",
            "seller": { "name": "seller" },
            "created_at": "2023-06-21T12:50:22Z"
        }"#;

        assert!(matches!(
            parse_response(body),
            Err(GumroadInfoFetchError::JSONParseError(_))
        ));
    }
}
