use super::definitions::{BoothAssetInfo, BoothItemDetail};
use chrono::Local;
use std::collections::HashMap;

const EXPIRATION_SECONDS: i64 = 60;

pub struct BoothCache {
    cache: HashMap<u64, (BoothAssetInfo, BoothItemDetail, i64)>,
}

impl BoothCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    fn get_valid(&self, id: u64) -> Option<&(BoothAssetInfo, BoothItemDetail, i64)> {
        let result = self.cache.get(&id)?;

        if result.2 < Local::now().timestamp() {
            return None;
        }

        Some(result)
    }

    pub fn get(&self, id: u64) -> Option<BoothAssetInfo> {
        self.get_valid(id).map(|r| r.0.clone())
    }

    pub fn get_detail(&self, id: u64) -> Option<BoothItemDetail> {
        self.get_valid(id).map(|r| r.1.clone())
    }

    pub fn insert(&mut self, id: u64, info: BoothAssetInfo, detail: BoothItemDetail) {
        self.cache.insert(
            id,
            (info, detail, Local::now().timestamp() + EXPIRATION_SECONDS),
        );
    }
}

#[cfg(test)]
mod tests {
    use model::AssetType;

    use super::*;

    fn get_example_asset_info() -> BoothAssetInfo {
        BoothAssetInfo {
            id: 12345,
            name: "Test Asset".to_string(),
            creator: "Test Creator".to_string(),
            estimated_asset_type: Some(AssetType::Avatar),
            image_urls: vec![],
            published_at: 12345,
        }
    }

    fn get_example_detail() -> BoothItemDetail {
        BoothItemDetail {
            name: "Test Asset".to_string(),
            shop_name: "Test Creator".to_string(),
            category_name: Some("Test Category".to_string()),
            description: "Test Description".to_string(),
            tags: vec!["Tag".to_string()],
            variation_names: vec!["Variation".to_string()],
        }
    }

    #[test]
    fn test_cache_hit() {
        let id = 12345;
        let asset_info = get_example_asset_info();
        let detail = get_example_detail();

        let mut cache = BoothCache::new();
        cache.insert(id, asset_info.clone(), detail.clone());

        assert_eq!(cache.get(id), Some(asset_info));
        assert_eq!(cache.get_detail(id), Some(detail));
    }

    #[test]
    fn test_cache_miss() {
        let id = 12345;
        let cache = BoothCache::new();

        assert_eq!(cache.get(id), None);
        assert_eq!(cache.get_detail(id), None);
    }

    #[test]
    fn test_cache_expired() {
        let id = 12345;
        let asset_info = get_example_asset_info();

        let mut cache = BoothCache::new();

        cache.cache.insert(
            id,
            (
                asset_info,
                get_example_detail(),
                Local::now().timestamp() - 1,
            ),
        );

        assert_eq!(cache.get(id), None);
        assert_eq!(cache.get_detail(id), None);
    }
}
