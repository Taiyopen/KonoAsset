use booth::BoothItemDetail;
use model::AssetType;
use serde_json::{Value, json};

use crate::definitions::ExistingCandidates;

pub const MODEL: &str = "claude-haiku-5-5";
pub const MAX_TOKENS: u32 = 4096;
pub const MAX_TAGS: usize = 5;

const SYSTEM_PROMPT: &str = "\
You are an assistant that classifies VRChat assets. Based on a BOOTH product's information, fill in its category, the avatars it supports, and its tags.

Rules:
1. Prefer choosing from the provided existing lists. If a list entry refers to the same thing as a name you would write in a different spelling (for example \"Manuka\" and \"マヌカ\"), you must use the spelling used in the list.
2. Propose a new name only when nothing in the list fits, and use the spelling used on BOOTH for it.
3. Choose at most one category. For supported avatars, list every avatar the product applies to; the variation names are a strong clue. Choose at most 5 tags.
4. Leave fields that do not apply to this asset type empty: set category to null and use empty arrays for the lists.
5. If you cannot decide, prefer leaving the field empty over guessing.
6. Respond only with JSON that matches the required schema.";

/// 4 種類の素材タイプで共通の、固定の応答スキーマ
pub fn response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "category": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
            "supportedAvatars": { "type": "array", "items": { "type": "string" } },
            "tags": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["category", "supportedAvatars", "tags"],
        "additionalProperties": false
    })
}

pub fn build_request(
    asset_type: AssetType,
    item: &BoothItemDetail,
    existing: &ExistingCandidates,
) -> Value {
    json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        "output_config": {
            "effort": "low",
            "format": { "type": "json_schema", "schema": response_schema() }
        },
        "system": SYSTEM_PROMPT,
        "messages": [{ "role": "user", "content": build_user_content(asset_type, item, existing) }]
    })
}

fn build_user_content(
    asset_type: AssetType,
    item: &BoothItemDetail,
    existing: &ExistingCandidates,
) -> String {
    let asset_type_name = match asset_type {
        AssetType::Avatar => "Avatar",
        AssetType::AvatarWearable => "AvatarWearable",
        AssetType::WorldObject => "WorldObject",
        AssetType::OtherAsset => "OtherAsset",
    };

    let mut content = format!("<asset_type>{asset_type_name}</asset_type>\n");

    content.push_str("<booth_item>\n");
    content.push_str(&format!("<name>{}</name>\n", item.name));
    content.push_str(&format!("<shop>{}</shop>\n", item.shop_name));
    content.push_str(&format!(
        "<booth_category>{}</booth_category>\n",
        item.category_name.as_deref().unwrap_or("(none)")
    ));
    content.push_str(&tagged_list("booth_tags", &item.tags));
    content.push_str(&tagged_list("variations", &item.variation_names));
    content.push_str(&format!("<description>{}</description>\n", item.description));
    content.push_str("</booth_item>\n");

    if asset_type != AssetType::Avatar {
        content.push_str(&tagged_list("existing_categories", &existing.categories));
    }
    if asset_type == AssetType::AvatarWearable {
        content.push_str(&tagged_list(
            "existing_supported_avatars",
            &existing.supported_avatars,
        ));
    }
    content.push_str(&tagged_list("existing_tags", &existing.tags));

    content
}

/// 1 行 1 項目で出力する。空の場合は `(none)` を 1 行で出力する
fn tagged_list(tag: &str, values: &[String]) -> String {
    if values.is_empty() {
        return format!("<{tag}>(none)</{tag}>\n");
    }
    format!("<{tag}>\n{}\n</{tag}>\n", values.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_item() -> BoothItemDetail {
        BoothItemDetail {
            name: "テスト衣装".to_string(),
            shop_name: "テストショップ".to_string(),
            category_name: Some("3Dキャラクター".to_string()),
            description: "Modular Avatar 対応です。\nマヌカ用の衣装です。".to_string(),
            tags: vec!["VRChat".to_string(), "衣装".to_string()],
            variation_names: vec!["マヌカ用".to_string(), "桔梗用".to_string()],
        }
    }

    fn sample_existing() -> ExistingCandidates {
        ExistingCandidates {
            categories: vec!["衣装".to_string(), "髪型".to_string()],
            supported_avatars: vec!["Manuka".to_string(), "Kikyo".to_string()],
            tags: vec!["Modular Avatar".to_string(), "VRChat".to_string()],
        }
    }

    fn user_content(req: &Value) -> String {
        req["messages"][0]["content"]
            .as_str()
            .expect("content should be a string")
            .to_string()
    }

    #[test]
    fn request_has_fixed_api_parameters() {
        let req = build_request(AssetType::AvatarWearable, &sample_item(), &sample_existing());
        assert_eq!(req["model"], "claude-haiku-5-5");
        assert_eq!(req["max_tokens"], 4096);
        assert_eq!(req["output_config"]["effort"], "low");
        assert_eq!(req["output_config"]["format"]["type"], "json_schema");
        assert_eq!(req["output_config"]["format"]["schema"], response_schema());
        assert!(req.get("temperature").is_none());
        assert_eq!(req["messages"][0]["role"], "user");
        assert!(req["system"].as_str().is_some_and(|s| !s.is_empty()));
    }

    #[test]
    fn wearable_request_contains_item_and_all_lists() {
        let content = user_content(&build_request(
            AssetType::AvatarWearable,
            &sample_item(),
            &sample_existing(),
        ));
        for s in [
            "<asset_type>AvatarWearable</asset_type>",
            "マヌカ用",
            "衣装",
            "Modular Avatar",
            sample_item().description.as_str(),
        ] {
            assert!(content.contains(s), "missing {s}");
        }
        assert!(content.contains("<existing_supported_avatars>"));
        assert!(content.contains("<existing_categories>"));
        assert!(content.contains("<existing_tags>"));
    }

    #[test]
    fn avatar_request_omits_category_and_avatar_lists() {
        let content = user_content(&build_request(
            AssetType::Avatar,
            &sample_item(),
            &sample_existing(),
        ));
        assert!(!content.contains("<existing_categories>"));
        assert!(!content.contains("<existing_supported_avatars>"));
        assert!(content.contains("<existing_tags>"));
    }

    #[test]
    fn world_object_request_omits_avatar_list_only() {
        let content = user_content(&build_request(
            AssetType::WorldObject,
            &sample_item(),
            &sample_existing(),
        ));
        assert!(content.contains("<existing_categories>"));
        assert!(!content.contains("<existing_supported_avatars>"));
    }

    #[test]
    fn empty_existing_lists_render_as_none() {
        let content = user_content(&build_request(
            AssetType::WorldObject,
            &sample_item(),
            &ExistingCandidates::default(),
        ));
        assert!(content.contains("<existing_categories>(none)</existing_categories>"));
        assert!(content.contains("<existing_tags>(none)</existing_tags>"));
    }

    #[test]
    fn existing_lists_have_one_name_per_line() {
        let content = user_content(&build_request(
            AssetType::AvatarWearable,
            &sample_item(),
            &sample_existing(),
        ));
        assert!(content.contains("<existing_tags>\nModular Avatar\nVRChat\n</existing_tags>"));
    }

    #[test]
    fn long_description_is_not_truncated() {
        let mut item = sample_item();
        item.description = "あ".repeat(20_000);
        let content = user_content(&build_request(
            AssetType::OtherAsset,
            &item,
            &sample_existing(),
        ));
        assert!(content.contains(item.description.as_str()));
    }
}
