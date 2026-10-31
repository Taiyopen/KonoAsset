use std::time::Duration;

use reqwest::header::HeaderValue;
use serde::Deserialize;

use crate::definitions::AiSuggestErrorKind;

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
pub const TIMEOUT_SECS: u64 = 15;

pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Claude Messages API へのリクエスト送信を抽象化する（テストで差し替えるため）
pub trait MessagesTransport {
    fn send(
        &self,
        api_key: &str,
        body: &serde_json::Value,
    ) -> impl std::future::Future<Output = Result<HttpResponse, AiSuggestErrorKind>> + Send;
}

pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new<S: AsRef<str>>(version: S) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(format!("KonoAsset/{}", version.as_ref()))
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .expect("failed to build the HTTP client");
        Self { client }
    }
}

fn map_reqwest_error(e: reqwest::Error) -> AiSuggestErrorKind {
    // Display には URL は含まれるが、ヘッダー（API キー）は含まれない
    log::warn!("Claude Messages API request failed: {e}");

    if e.is_timeout() {
        AiSuggestErrorKind::Timeout
    } else {
        AiSuggestErrorKind::Network
    }
}

/// API キーを `x-api-key` ヘッダー値にする。ログやデバッグ出力に出ないよう sensitive にする
fn api_key_header(api_key: &str) -> Result<HeaderValue, AiSuggestErrorKind> {
    let mut value = HeaderValue::from_str(api_key).map_err(|_| {
        log::warn!("The API key contains characters that are not valid in an HTTP header");
        AiSuggestErrorKind::InvalidApiKey
    })?;
    value.set_sensitive(true);
    Ok(value)
}

impl MessagesTransport for ReqwestTransport {
    async fn send(
        &self,
        api_key: &str,
        body: &serde_json::Value,
    ) -> Result<HttpResponse, AiSuggestErrorKind> {
        let response = self
            .client
            .post(API_URL)
            .header("x-api-key", api_key_header(api_key)?)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let status = response.status().as_u16();
        log::debug!("Claude Messages API responded with status {status}");
        let body = response.text().await.map_err(map_reqwest_error)?;
        Ok(HttpResponse { status, body })
    }
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    message: Option<String>,
}

/// ログに出す本文・メッセージの最大文字数
const LOG_SNIPPET_CHARS: usize = 200;

fn snippet(text: &str) -> String {
    text.chars().take(LOG_SNIPPET_CHARS).collect()
}

/// 2xx 以外のレスポンスをログ用に要約する。API のエラー本文にキーは含まれない
fn describe_http_error(status: u16, body: &str) -> String {
    match serde_json::from_str::<ErrorEnvelope>(body) {
        Ok(envelope) => format!(
            "status {status}, error.type {}, error.message {}",
            envelope.error.kind,
            snippet(envelope.error.message.as_deref().unwrap_or("")),
        ),
        Err(_) => format!("status {status}, body {}", snippet(body)),
    }
}

/// 2xx 以外のレスポンスをエラー種別に対応づける。`error.type` を優先し、読めなければ状態コードで判断する
pub(crate) fn map_http_error(status: u16, body: &str) -> AiSuggestErrorKind {
    log::warn!(
        "Claude Messages API returned an error: {}",
        describe_http_error(status, body)
    );

    if let Ok(envelope) = serde_json::from_str::<ErrorEnvelope>(body) {
        match envelope.error.kind.as_str() {
            "authentication_error" | "permission_error" => {
                return AiSuggestErrorKind::InvalidApiKey;
            }
            "billing_error" => return AiSuggestErrorKind::InsufficientCredit,
            "rate_limit_error" => return AiSuggestErrorKind::RateLimited,
            "overloaded_error" | "api_error" => return AiSuggestErrorKind::Overloaded,
            _ => {}
        }
    }

    match status {
        401 | 403 => AiSuggestErrorKind::InvalidApiKey,
        402 => AiSuggestErrorKind::InsufficientCredit,
        429 => AiSuggestErrorKind::RateLimited,
        500..=599 => AiSuggestErrorKind::Overloaded,
        _ => AiSuggestErrorKind::InvalidResponse,
    }
}

#[derive(Deserialize)]
struct MessageResponse {
    stop_reason: Option<String>,
    #[serde(default)]
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

/// 成功レスポンスから最初の `text` ブロックを取り出す
pub(crate) fn extract_output_text(body: &str) -> Result<String, AiSuggestErrorKind> {
    let message: MessageResponse = serde_json::from_str(body).map_err(|e| {
        log::warn!("Failed to parse the Claude Messages API response: {e}");
        AiSuggestErrorKind::InvalidResponse
    })?;

    match message.stop_reason.as_deref() {
        Some("refusal") => {
            log::warn!("Claude Messages API stopped with stop_reason refusal");
            return Err(AiSuggestErrorKind::Refused);
        }
        Some("max_tokens") => {
            log::warn!("Claude Messages API stopped with stop_reason max_tokens");
            return Err(AiSuggestErrorKind::Truncated);
        }
        _ => {}
    }

    message
        .content
        .into_iter()
        .find(|block| block.kind == "text")
        .and_then(|block| block.text)
        .ok_or(AiSuggestErrorKind::InvalidResponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_errors_map_to_kinds() {
        let e = |status, ty: &str| {
            map_http_error(
                status,
                &format!(r#"{{"type":"error","error":{{"type":"{ty}","message":"x"}}}}"#),
            )
        };
        assert_eq!(
            e(401, "authentication_error"),
            AiSuggestErrorKind::InvalidApiKey
        );
        assert_eq!(
            e(403, "permission_error"),
            AiSuggestErrorKind::InvalidApiKey
        );
        assert_eq!(
            e(402, "billing_error"),
            AiSuggestErrorKind::InsufficientCredit
        );
        assert_eq!(e(429, "rate_limit_error"), AiSuggestErrorKind::RateLimited);
        assert_eq!(e(529, "overloaded_error"), AiSuggestErrorKind::Overloaded);
        assert_eq!(e(500, "api_error"), AiSuggestErrorKind::Overloaded);
        assert_eq!(
            map_http_error(502, "<html>bad gateway</html>"),
            AiSuggestErrorKind::Overloaded
        );
        assert_eq!(
            map_http_error(404, "not json"),
            AiSuggestErrorKind::InvalidResponse
        );
    }

    #[test]
    fn http_errors_fall_back_to_status_code_when_body_is_unreadable() {
        assert_eq!(map_http_error(401, ""), AiSuggestErrorKind::InvalidApiKey);
        assert_eq!(map_http_error(403, "x"), AiSuggestErrorKind::InvalidApiKey);
        assert_eq!(
            map_http_error(402, "x"),
            AiSuggestErrorKind::InsufficientCredit
        );
        assert_eq!(map_http_error(429, "x"), AiSuggestErrorKind::RateLimited);
        assert_eq!(map_http_error(529, "x"), AiSuggestErrorKind::Overloaded);
        // 未知の error.type は状態コードで判断する
        assert_eq!(
            map_http_error(
                429,
                r#"{"type":"error","error":{"type":"unknown_error","message":"x"}}"#
            ),
            AiSuggestErrorKind::RateLimited
        );
        assert_eq!(
            map_http_error(
                400,
                r#"{"type":"error","error":{"type":"invalid_request_error"}}"#
            ),
            AiSuggestErrorKind::InvalidResponse
        );
    }

    #[test]
    fn api_key_header_is_sensitive_and_rejects_invalid_characters() {
        let value = api_key_header("sk-test").unwrap();
        assert!(value.is_sensitive());
        assert_eq!(value.to_str().unwrap(), "sk-test");
        assert_eq!(format!("{value:?}"), "Sensitive");

        assert_eq!(
            api_key_header("sk-test\nnext"),
            Err(AiSuggestErrorKind::InvalidApiKey)
        );
        assert_eq!(
            api_key_header("sk-test\u{7f}"),
            Err(AiSuggestErrorKind::InvalidApiKey)
        );
    }

    #[test]
    fn http_error_description_uses_error_fields_or_a_truncated_body() {
        let described = describe_http_error(
            400,
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad field"}}"#,
        );
        assert_eq!(
            described,
            "status 400, error.type invalid_request_error, error.message bad field"
        );

        let long_body = "x".repeat(500);
        let described = describe_http_error(502, &long_body);
        assert_eq!(
            described,
            format!("status 502, body {}", "x".repeat(LOG_SNIPPET_CHARS))
        );
    }

    #[test]
    fn stop_reasons_and_missing_text_are_errors() {
        assert_eq!(
            extract_output_text(r#"{"stop_reason":"refusal","content":[]}"#),
            Err(AiSuggestErrorKind::Refused)
        );
        assert_eq!(
            extract_output_text(
                r#"{"stop_reason":"max_tokens","content":[{"type":"text","text":"{"}]}"#
            ),
            Err(AiSuggestErrorKind::Truncated)
        );
        assert_eq!(
            extract_output_text(
                r#"{"stop_reason":"end_turn","content":[{"type":"thinking","thinking":""}]}"#
            ),
            Err(AiSuggestErrorKind::InvalidResponse)
        );
        assert_eq!(
            extract_output_text("not json"),
            Err(AiSuggestErrorKind::InvalidResponse)
        );
    }

    #[test]
    fn extract_output_text_skips_non_text_blocks() {
        let body = r#"{"stop_reason":"end_turn","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"first"},{"type":"text","text":"second"}]}"#;
        assert_eq!(extract_output_text(body), Ok("first".to_string()));
    }
}
