use std::collections::HashMap;

use serde::Serialize;

/// ユーザーのデータベースに存在する既存の名称一覧（AI に選ばせる候補）
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExistingCandidates {
    pub categories: Vec<String>,
    pub supported_avatars: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedValue {
    pub value: String,
    pub is_new: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiSuggestion {
    pub category: Option<SuggestedValue>,
    pub supported_avatars: Vec<SuggestedValue>,
    pub tags: Vec<SuggestedValue>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, specta::Type, thiserror::Error)]
pub enum AiSuggestErrorKind {
    #[error("AI suggestion is not configured")]
    NotConfigured,
    #[error("invalid API key or insufficient permission")]
    InvalidApiKey,
    #[error("insufficient API credit")]
    InsufficientCredit,
    #[error("rate limited by the API")]
    RateLimited,
    #[error("the API is overloaded or unavailable")]
    Overloaded,
    #[error("request timed out")]
    Timeout,
    #[error("network error while calling the API")]
    Network,
    #[error("the model refused to answer")]
    Refused,
    #[error("the response was truncated by the token limit")]
    Truncated,
    #[error("the response could not be interpreted")]
    InvalidResponse,
}

/// 値を trim し、空文字を除外して、出現回数の多い順（同数なら文字列の昇順）に重複なしで返す
pub fn rank_by_usage<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut counts: HashMap<String, usize> = HashMap::new();
    for value in values {
        let value = value.as_ref().trim();
        if value.is_empty() {
            continue;
        }
        *counts.entry(value.to_string()).or_insert(0) += 1;
    }

    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|(a_name, a_count), (b_name, b_count)| {
        b_count.cmp(a_count).then_with(|| a_name.cmp(b_name))
    });
    ranked.into_iter().map(|(name, _)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_by_usage_orders_by_count_then_name() {
        assert_eq!(
            rank_by_usage([" b", "a", "b", "", "c", "a", "b"]),
            vec!["b", "a", "c"]
        );
    }

    #[test]
    fn rank_by_usage_breaks_ties_by_name_and_skips_blank() {
        assert_eq!(rank_by_usage(["z", "y", "  ", "x"]), vec!["x", "y", "z"]);
        assert!(rank_by_usage(Vec::<String>::new()).is_empty());
    }
}
