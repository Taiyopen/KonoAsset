#[derive(Debug, thiserror::Error)]
pub enum GumroadInfoFetchError {
    #[error("Invalid Gumroad URL: {0}")]
    InvalidUrl(String),
    #[error("Item not found with URL: {0}")]
    NotFound(String),
    #[error("Failed to fetch asset description from Gumroad: {0}")]
    APICallError(#[from] reqwest::Error),
    #[error("Failed to parse JSON from Gumroad response: {0}")]
    JSONParseError(#[from] serde_json::Error),
    #[error("Failed to parse date from Gumroad response: {0}")]
    DateParseError(#[from] chrono::format::ParseError),
}
