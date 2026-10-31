mod client;
mod definitions;
mod prompt;
mod resolve;

pub use client::{
    ANTHROPIC_VERSION, API_URL, HttpResponse, MessagesTransport, ReqwestTransport, TIMEOUT_SECS,
};
pub use definitions::{
    AiSuggestErrorKind, AiSuggestion, ExistingCandidates, SuggestedValue, rank_by_usage,
};
pub use prompt::{MAX_TAGS, MAX_TOKENS, MODEL, build_request, response_schema};

use booth::BoothItemDetail;
use client::{extract_output_text, map_http_error};
use model::AssetType;
use resolve::{RawSuggestion, resolve};

/// BOOTH の商品情報から Claude にフィールドを提案させ、既存の名称に照らして正規化した結果を返す
pub async fn suggest<T: MessagesTransport>(
    transport: &T,
    api_key: &str,
    asset_type: AssetType,
    item: &BoothItemDetail,
    existing: &ExistingCandidates,
) -> Result<AiSuggestion, AiSuggestErrorKind> {
    let request = build_request(asset_type, item, existing);
    let response = transport.send(api_key, &request).await?;
    if !(200..300).contains(&response.status) {
        return Err(map_http_error(response.status, &response.body));
    }

    let text = extract_output_text(&response.body)?;
    let raw: RawSuggestion =
        serde_json::from_str(&text).map_err(|_| AiSuggestErrorKind::InvalidResponse)?;
    Ok(resolve(raw, asset_type, existing))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeTransport {
        response: Result<HttpResponse, AiSuggestErrorKind>,
        received_key: std::sync::Mutex<Option<String>>,
        received_body: std::sync::Mutex<Option<serde_json::Value>>,
    }

    impl FakeTransport {
        fn new(response: Result<HttpResponse, AiSuggestErrorKind>) -> Self {
            Self {
                response,
                received_key: std::sync::Mutex::new(None),
                received_body: std::sync::Mutex::new(None),
            }
        }

        fn ok(status: u16, body: &str) -> Self {
            Self::new(Ok(HttpResponse {
                status,
                body: body.to_string(),
            }))
        }

        fn err(kind: AiSuggestErrorKind) -> Self {
            Self::new(Err(kind))
        }

        fn received_key(&self) -> String {
            self.received_key.lock().unwrap().clone().unwrap()
        }

        fn received_body(&self) -> serde_json::Value {
            self.received_body.lock().unwrap().clone().unwrap()
        }
    }

    impl MessagesTransport for FakeTransport {
        async fn send(
            &self,
            api_key: &str,
            body: &serde_json::Value,
        ) -> Result<HttpResponse, AiSuggestErrorKind> {
            *self.received_key.lock().unwrap() = Some(api_key.to_string());
            *self.received_body.lock().unwrap() = Some(body.clone());
            match &self.response {
                Ok(r) => Ok(HttpResponse {
                    status: r.status,
                    body: r.body.clone(),
                }),
                Err(kind) => Err(*kind),
            }
        }
    }

    fn sample_item() -> BoothItemDetail {
        BoothItemDetail {
            name: "テスト衣装".to_string(),
            shop_name: "テストショップ".to_string(),
            category_name: Some("3Dキャラクター".to_string()),
            description: "マヌカ用の衣装です。".to_string(),
            tags: vec!["VRChat".to_string()],
            variation_names: vec!["マヌカ用".to_string()],
        }
    }

    async fn run(t: &FakeTransport) -> Result<AiSuggestion, AiSuggestErrorKind> {
        suggest(
            t,
            "sk-test",
            AssetType::AvatarWearable,
            &sample_item(),
            &ExistingCandidates::default(),
        )
        .await
    }

    fn message_with_text(text: &str) -> String {
        serde_json::json!({
            "stop_reason": "end_turn",
            "content": [
                {"type": "thinking", "thinking": ""},
                {"type": "text", "text": text}
            ]
        })
        .to_string()
    }

    #[tokio::test]
    async fn suggest_returns_resolved_values_on_success() {
        let body =
            message_with_text(r#"{"category":"衣装","supportedAvatars":["マヌカ"],"tags":[]}"#);
        let t = FakeTransport::ok(200, &body);
        let s = run(&t).await.unwrap();
        assert_eq!(s.category.unwrap().value, "衣装");
        assert_eq!(s.supported_avatars[0].value, "マヌカ");
        assert_eq!(t.received_key(), "sk-test");
        assert_eq!(
            t.received_body(),
            build_request(
                AssetType::AvatarWearable,
                &sample_item(),
                &ExistingCandidates::default()
            )
        );
    }

    #[tokio::test]
    async fn suggest_maps_non_2xx_status_to_error_kind() {
        let t = FakeTransport::ok(
            401,
            r#"{"type":"error","error":{"type":"authentication_error","message":"x"}}"#,
        );
        assert_eq!(run(&t).await, Err(AiSuggestErrorKind::InvalidApiKey));
    }

    #[tokio::test]
    async fn invalid_json_text_and_transport_errors_propagate() {
        let t = FakeTransport::ok(200, &message_with_text("not json"));
        assert_eq!(run(&t).await, Err(AiSuggestErrorKind::InvalidResponse));

        let t = FakeTransport::err(AiSuggestErrorKind::Timeout);
        assert_eq!(run(&t).await, Err(AiSuggestErrorKind::Timeout));

        let t = FakeTransport::err(AiSuggestErrorKind::Network);
        assert_eq!(run(&t).await, Err(AiSuggestErrorKind::Network));
    }
}
