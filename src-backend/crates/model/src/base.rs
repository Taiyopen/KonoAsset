use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Copy, Clone, Eq, PartialEq, specta::Type)]
pub enum AssetType {
    Avatar,
    AvatarWearable,
    WorldObject,
    OtherAsset,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Avatar {
    pub id: Uuid,
    pub description: AssetDescription,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AvatarWearable {
    pub id: Uuid,
    pub description: AssetDescription,
    pub category: String,
    pub supported_avatars: BTreeSet<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorldObject {
    pub id: Uuid,
    pub description: AssetDescription,
    pub category: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OtherAsset {
    pub id: Uuid,
    pub description: AssetDescription,
    pub category: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AssetDescription {
    pub name: String,
    pub creator: String,
    pub image_filename: Option<String>,
    pub tags: Vec<String>,
    pub memo: Option<String>,
    pub booth_item_id: Option<u64>,
    #[serde(default)]
    pub gumroad_url: Option<String>,
    pub dependencies: Vec<Uuid>,
    pub created_at: i64,
    pub published_at: Option<i64>,
}

impl Avatar {
    pub fn create(mut description: AssetDescription) -> Self {
        description.name = description.name.trim().to_string();
        description.creator = description.creator.trim().to_string();
        description.tags = description
            .tags
            .iter()
            .map(|tag| tag.trim().to_string())
            .collect();

        Self {
            id: Uuid::new_v4(),
            description,
        }
    }
}

impl AvatarWearable {
    pub fn create(
        mut description: AssetDescription,
        mut category: String,
        mut supported_avatars: BTreeSet<String>,
    ) -> Self {
        description.name = description.name.trim().to_string();
        description.creator = description.creator.trim().to_string();
        description.tags = description
            .tags
            .iter()
            .map(|tag| tag.trim().to_string())
            .collect();

        category = category.trim().to_string();
        supported_avatars = supported_avatars
            .iter()
            .map(|avatar| avatar.trim().to_string())
            .collect();

        Self {
            id: Uuid::new_v4(),
            description,
            category,
            supported_avatars,
        }
    }
}

impl WorldObject {
    pub fn create(mut description: AssetDescription, mut category: String) -> Self {
        description.name = description.name.trim().to_string();
        description.creator = description.creator.trim().to_string();
        description.tags = description
            .tags
            .iter()
            .map(|tag| tag.trim().to_string())
            .collect();

        category = category.trim().to_string();

        Self {
            id: Uuid::new_v4(),
            description,
            category,
        }
    }
}

impl OtherAsset {
    pub fn create(mut description: AssetDescription, mut category: String) -> Self {
        description.name = description.name.trim().to_string();
        description.creator = description.creator.trim().to_string();
        description.tags = description
            .tags
            .iter()
            .map(|tag| tag.trim().to_string())
            .collect();

        category = category.trim().to_string();

        Self {
            id: Uuid::new_v4(),
            description,
            category,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_description_without_gumroad_url_deserializes() {
        let json = r#"{
            "name": "name",
            "creator": "creator",
            "imageFilename": null,
            "tags": [],
            "memo": null,
            "boothItemId": 1,
            "dependencies": [],
            "createdAt": 0,
            "publishedAt": null
        }"#;

        let description: AssetDescription = serde_json::from_str(json).unwrap();

        assert_eq!(description.gumroad_url, None);
        assert_eq!(description.booth_item_id, Some(1));
    }

    #[test]
    fn test_description_with_gumroad_url_roundtrips() {
        let description = AssetDescription {
            name: "name".to_string(),
            creator: "creator".to_string(),
            image_filename: None,
            tags: vec![],
            memo: None,
            booth_item_id: None,
            gumroad_url: Some("https://bananabread.gumroad.com/l/ajfrt".to_string()),
            dependencies: vec![],
            created_at: 0,
            published_at: None,
        };

        let json = serde_json::to_string(&description).unwrap();
        assert!(json.contains(r#""gumroadUrl":"https://bananabread.gumroad.com/l/ajfrt""#));

        let restored: AssetDescription = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, description);
    }
}
