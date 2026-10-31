use model::AssetType;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BoothAssetInfo {
    pub id: u64,
    pub name: String,
    pub creator: String,
    pub image_urls: Vec<String>,
    pub published_at: i64,
    pub estimated_asset_type: Option<AssetType>,
}

/// BOOTH 商品的詳細資料（供 AI 建議欄位使用，不會傳給前端）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoothItemDetail {
    pub name: String,
    pub shop_name: String,
    pub category_name: Option<String>,
    pub description: String,
    pub tags: Vec<String>,
    /// 已去除 null 與空字串
    pub variation_names: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct BoothJsonSchema {
    pub id: u64,
    pub name: String,
    pub shop: BoothShop,
    pub images: Vec<BoothPximg>,
    pub category: BoothCategory,
    pub published_at: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<BoothTag>,
    #[serde(default)]
    pub variations: Vec<BoothVariation>,
}

#[derive(Deserialize)]
pub(crate) struct BoothShop {
    pub name: String,
}

#[derive(Deserialize)]
pub(crate) struct BoothPximg {
    pub original: String,
}

#[derive(Deserialize)]
pub(crate) struct BoothCategory {
    pub id: i32,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BoothTag {
    pub name: String,
}

#[derive(Deserialize)]
pub(crate) struct BoothVariation {
    pub name: Option<String>,
}
