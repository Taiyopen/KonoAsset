use std::sync::LazyLock;

use regex::Regex;

use crate::GumroadInfoFetchError;

static GUMROAD_URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https://(?:([0-9a-z-]+)\.)?gumroad\.com/l/([0-9A-Za-z_-]+)/?(?:\?.*)?$").unwrap()
});

pub fn is_gumroad_url(url: &str) -> bool {
    GUMROAD_URL_REGEX.is_match(url)
}

/// 商品 URL を JSON 版の URL に変換する（クエリと末尾の `/` は取り除く）
pub fn to_json_url(url: &str) -> Result<String, GumroadInfoFetchError> {
    let captures = GUMROAD_URL_REGEX
        .captures(url)
        .ok_or_else(|| GumroadInfoFetchError::InvalidUrl(url.to_string()))?;

    let host = match captures.get(1) {
        Some(seller) => format!("{}.gumroad.com", seller.as_str()),
        None => "gumroad.com".to_string(),
    };

    Ok(format!("https://{}/l/{}.json", host, &captures[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_URLS: [&str; 5] = [
        "https://bananabread.gumroad.com/l/ajfrt",
        "https://gumroad.com/l/ajfrt",
        "https://bananabread.gumroad.com/l/ajfrt/",
        "https://bananabread.gumroad.com/l/ajfrt?layout=profile",
        "https://seller-name.gumroad.com/l/My_Item-2",
    ];

    const INVALID_URLS: [&str; 6] = [
        "http://bananabread.gumroad.com/l/ajfrt",
        "https://bananabread.gumroad.com/",
        "https://bananabread.gumroad.com/l/",
        "https://evilgumroad.com/l/ajfrt",
        "https://gumroad.com.evil.com/l/ajfrt",
        "https://booth.pm/ja/items/6641548",
    ];

    #[test]
    fn test_is_gumroad_url() {
        for url in VALID_URLS {
            assert!(is_gumroad_url(url), "{url} should be valid");
        }
        for url in INVALID_URLS {
            assert!(!is_gumroad_url(url), "{url} should be invalid");
        }
    }

    #[test]
    fn test_to_json_url() {
        let seller_json = "https://bananabread.gumroad.com/l/ajfrt.json";

        assert_eq!(to_json_url(VALID_URLS[0]).unwrap(), seller_json);
        assert_eq!(
            to_json_url(VALID_URLS[1]).unwrap(),
            "https://gumroad.com/l/ajfrt.json"
        );
        assert_eq!(to_json_url(VALID_URLS[2]).unwrap(), seller_json);
        assert_eq!(to_json_url(VALID_URLS[3]).unwrap(), seller_json);

        for url in INVALID_URLS {
            assert!(matches!(
                to_json_url(url),
                Err(GumroadInfoFetchError::InvalidUrl(_))
            ));
        }
    }
}
