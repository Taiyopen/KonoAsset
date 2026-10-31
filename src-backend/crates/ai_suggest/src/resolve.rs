use std::collections::HashSet;

use model::AssetType;
use serde::Deserialize;

use crate::definitions::{AiSuggestion, ExistingCandidates, SuggestedValue};
use crate::prompt::MAX_TAGS;

/// AI のレスポンス（構造化出力）をそのままデシリアライズした値
#[derive(Deserialize, Debug, PartialEq)]
pub(crate) struct RawSuggestion {
    pub category: Option<String>,
    #[serde(rename = "supportedAvatars")]
    pub supported_avatars: Vec<String>,
    pub tags: Vec<String>,
}

/// AI の提案を正規化し、既存の名称と一致しないものに `is_new` を付ける
pub(crate) fn resolve(
    raw: RawSuggestion,
    asset_type: AssetType,
    existing: &ExistingCandidates,
) -> AiSuggestion {
    let category = match asset_type {
        AssetType::Avatar => None,
        _ => raw
            .category
            .and_then(|name| resolve_one(&name, &existing.categories)),
    };
    let supported_avatars = match asset_type {
        AssetType::AvatarWearable => {
            resolve_many(raw.supported_avatars, &existing.supported_avatars)
        }
        _ => Vec::new(),
    };
    let mut tags = resolve_many(raw.tags, &existing.tags);
    tags.truncate(MAX_TAGS);

    AiSuggestion {
        category,
        supported_avatars,
        tags,
    }
}

fn normalize_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// trim 後に空なら `None`。既存リストに（大文字小文字を無視して）一致すれば既存の表記を使う
fn resolve_one(name: &str, list: &[String]) -> Option<SuggestedValue> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let key = normalize_key(name);
    Some(match list.iter().find(|item| normalize_key(item) == key) {
        Some(existing) => SuggestedValue {
            value: existing.clone(),
            is_new: false,
        },
        None => SuggestedValue {
            value: name.to_string(),
            is_new: true,
        },
    })
}

/// 各名称を正規化・照合し、正規化後の重複を除く（最初の出現順を保持）
fn resolve_many(names: Vec<String>, list: &[String]) -> Vec<SuggestedValue> {
    let mut seen = HashSet::new();
    names
        .iter()
        .filter_map(|name| resolve_one(name, list))
        .filter(|resolved| seen.insert(normalize_key(&resolved.value)))
        .collect()
}

#[cfg(test)]
mod tests {
    use model::AssetType;

    use super::*;

    fn sv(value: &str, is_new: bool) -> SuggestedValue {
        SuggestedValue {
            value: value.into(),
            is_new,
        }
    }

    #[test]
    fn existing_names_are_not_new_and_use_list_spelling() {
        let existing = ExistingCandidates {
            categories: vec!["衣装".into()],
            supported_avatars: vec!["Manuka".into()],
            tags: vec!["Modular Avatar".into()],
        };
        let raw = RawSuggestion {
            category: Some(" 衣装 ".into()),
            supported_avatars: vec!["MANUKA".into(), "桔梗".into()],
            tags: vec!["modular avatar".into()],
        };
        let s = resolve(raw, AssetType::AvatarWearable, &existing);
        assert_eq!(s.category, Some(sv("衣装", false)));
        assert_eq!(
            s.supported_avatars,
            vec![sv("Manuka", false), sv("桔梗", true)]
        );
        assert_eq!(s.tags, vec![sv("Modular Avatar", false)]);
    }

    #[test]
    fn duplicates_after_normalization_are_removed() {
        let existing = ExistingCandidates {
            supported_avatars: vec!["Manuka".into()],
            ..Default::default()
        };
        let raw = RawSuggestion {
            category: None,
            supported_avatars: vec![
                "manuka".into(),
                "Manuka ".into(),
                "MANUKA".into(),
                "".into(),
            ],
            tags: vec![],
        };
        let s = resolve(raw, AssetType::AvatarWearable, &existing);
        assert_eq!(s.supported_avatars, vec![sv("Manuka", false)]);
    }

    #[test]
    fn new_names_differing_only_in_case_are_deduplicated_keeping_first() {
        let raw = RawSuggestion {
            category: None,
            supported_avatars: vec![],
            tags: vec![
                "VRChat".into(),
                "vrchat".into(),
                "  ".into(),
                "Quest".into(),
            ],
        };
        let s = resolve(
            raw,
            AssetType::AvatarWearable,
            &ExistingCandidates::default(),
        );
        assert_eq!(s.tags, vec![sv("VRChat", true), sv("Quest", true)]);
    }

    #[test]
    fn everything_is_new_when_lists_are_empty() {
        let raw = RawSuggestion {
            category: Some("衣装".into()),
            supported_avatars: vec!["マヌカ".into()],
            tags: vec!["VRChat".into()],
        };
        let s = resolve(
            raw,
            AssetType::AvatarWearable,
            &ExistingCandidates::default(),
        );
        assert!(s.category.unwrap().is_new && s.supported_avatars[0].is_new && s.tags[0].is_new);
    }

    #[test]
    fn fields_match_only_their_own_list() {
        let existing = ExistingCandidates {
            categories: vec!["衣装".into()],
            supported_avatars: vec![],
            tags: vec![],
        };
        let raw = RawSuggestion {
            category: None,
            supported_avatars: vec!["衣装".into()],
            tags: vec!["衣装".into()],
        };
        let s = resolve(raw, AssetType::AvatarWearable, &existing);
        assert_eq!(s.supported_avatars, vec![sv("衣装", true)]);
        assert_eq!(s.tags, vec![sv("衣装", true)]);
    }

    #[test]
    fn blank_category_becomes_none() {
        let raw = RawSuggestion {
            category: Some("   ".into()),
            supported_avatars: vec![],
            tags: vec![],
        };
        let s = resolve(raw, AssetType::WorldObject, &ExistingCandidates::default());
        assert_eq!(s.category, None);
    }

    #[test]
    fn tags_are_limited_to_five() {
        let raw = RawSuggestion {
            category: None,
            supported_avatars: vec![],
            tags: (1..=7).map(|i| format!("tag{i}")).collect(),
        };
        let s = resolve(
            raw,
            AssetType::AvatarWearable,
            &ExistingCandidates::default(),
        );
        let values: Vec<_> = s.tags.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(values, vec!["tag1", "tag2", "tag3", "tag4", "tag5"]);
    }

    #[test]
    fn tag_cap_applies_after_deduplication() {
        let raw = RawSuggestion {
            category: None,
            supported_avatars: vec![],
            tags: vec!["a", "A", "b", "c", "d", "e", "f"]
                .into_iter()
                .map(String::from)
                .collect(),
        };
        let s = resolve(
            raw,
            AssetType::AvatarWearable,
            &ExistingCandidates::default(),
        );
        let values: Vec<_> = s.tags.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(values, vec!["a", "b", "c", "d", "e"]);
    }

    fn full_raw() -> RawSuggestion {
        RawSuggestion {
            category: Some("衣装".into()),
            supported_avatars: vec!["Manuka".into()],
            tags: vec!["VRChat".into()],
        }
    }

    #[test]
    fn avatar_drops_category_and_supported_avatars() {
        let s = resolve(
            full_raw(),
            AssetType::Avatar,
            &ExistingCandidates::default(),
        );
        assert_eq!(s.category, None);
        assert!(s.supported_avatars.is_empty());
        assert_eq!(s.tags, vec![sv("VRChat", true)]);
    }

    #[test]
    fn world_object_and_other_asset_drop_supported_avatars() {
        for asset_type in [AssetType::WorldObject, AssetType::OtherAsset] {
            let s = resolve(full_raw(), asset_type, &ExistingCandidates::default());
            assert_eq!(s.category, Some(sv("衣装", true)));
            assert!(s.supported_avatars.is_empty());
            assert_eq!(s.tags, vec![sv("VRChat", true)]);
        }
    }

    #[test]
    fn raw_suggestion_deserializes_camel_case() {
        let raw: RawSuggestion =
            serde_json::from_str(r#"{"category":null,"supportedAvatars":["Manuka"],"tags":["a"]}"#)
                .unwrap();
        assert_eq!(raw.category, None);
        assert_eq!(raw.supported_avatars, vec!["Manuka"]);
    }
}
