mod client;
mod definitions;
mod error;
mod fetcher;
mod url;

pub use definitions::GumroadAssetInfo;
pub use error::GumroadInfoFetchError;
pub use fetcher::GumroadFetcher;
pub use url::{is_gumroad_url, to_json_url};
