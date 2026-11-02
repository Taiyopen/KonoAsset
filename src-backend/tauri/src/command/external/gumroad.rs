use gumroad::{GumroadAssetInfo, GumroadFetcher};
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn get_asset_info_from_gumroad(
    gumroad_fetcher: State<'_, GumroadFetcher>,
    url: String,
) -> Result<GumroadAssetInfo, String> {
    log::info!("Fetching asset description from Gumroad (URL = {:?})", url);

    let result = gumroad_fetcher.fetch(&url).await;

    if let Ok(info) = &result {
        log::info!(
            "Successfully fetched asset description from Gumroad (URL = {:?})",
            url
        );
        log::debug!("Fetched info: {:?}", info);
    } else {
        log::error!(
            "Failed to fetch asset description from Gumroad: {:?}",
            result
        );
    }

    result.map_err(|e| e.to_string())
}
