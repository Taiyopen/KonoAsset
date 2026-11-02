use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GumroadAssetInfo {
    /// Gumroad が返す正式な商品 URL（素材に保存する値）
    pub url: String,
    pub name: String,
    pub creator: String,
    pub image_urls: Vec<String>,
    pub published_at: i64,
}

#[derive(Deserialize)]
pub(crate) struct GumroadJsonSchema {
    pub url: String,
    pub name: String,
    pub seller: GumroadSeller,
    pub created_at: String,
    #[serde(default)]
    pub covers: Vec<GumroadCover>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct GumroadSeller {
    pub name: String,
}

#[derive(Deserialize)]
pub(crate) struct GumroadCover {
    #[serde(rename = "type")]
    pub cover_type: String,
    // 1 件の不正な cover で取得全体が失敗しないよう、欠落や null を許容する
    #[serde(default)]
    pub url: Option<String>,
}
