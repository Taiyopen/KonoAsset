use std::sync::Arc;

use ai_suggest::{
    AiSuggestErrorKind, AiSuggestion, ExistingCandidates, ReqwestTransport, rank_by_usage, suggest,
};
use booth::BoothFetcher;
use model::{AssetType, preference::PreferenceStore};
use storage::asset_storage::AssetStorage;
use tauri::{State, async_runtime::Mutex};

#[tauri::command]
#[specta::specta]
pub async fn suggest_asset_fields_with_ai(
    preference: State<'_, Arc<Mutex<PreferenceStore>>>,
    booth_fetcher: State<'_, Mutex<BoothFetcher>>,
    basic_store: State<'_, Arc<Mutex<AssetStorage>>>,
    transport: State<'_, ReqwestTransport>,
    booth_item_id: u64,
    asset_type: AssetType,
) -> Result<AiSuggestion, AiSuggestErrorKind> {
    log::info!(
        "Requesting AI suggestion (Booth Item ID = {:?}, asset type = {:?})",
        booth_item_id,
        asset_type
    );

    let result = run(
        &preference,
        &booth_fetcher,
        &basic_store,
        &transport,
        booth_item_id,
        asset_type,
    )
    .await;

    match &result {
        Ok(_) => log::info!(
            "Successfully got AI suggestion (Booth Item ID = {:?})",
            booth_item_id
        ),
        Err(kind) => log::error!(
            "Failed to get AI suggestion (Booth Item ID = {:?}): {:?}",
            booth_item_id,
            kind
        ),
    }

    result
}

async fn run(
    preference: &Mutex<PreferenceStore>,
    booth_fetcher: &Mutex<BoothFetcher>,
    basic_store: &Mutex<AssetStorage>,
    transport: &ReqwestTransport,
    booth_item_id: u64,
    asset_type: AssetType,
) -> Result<AiSuggestion, AiSuggestErrorKind> {
    let (enabled, api_key) = {
        let preference = preference.lock().await;
        (
            preference.ai_suggest_enabled,
            preference.ai_suggest_api_key.trim().to_string(),
        )
    };
    if !enabled || api_key.is_empty() {
        return Err(AiSuggestErrorKind::NotConfigured);
    }

    let item = {
        let mut fetcher = booth_fetcher.lock().await;
        fetcher.fetch_detail(booth_item_id).await
    }
    .map_err(|e| {
        log::error!("Failed to fetch Booth item detail: {}", e);
        AiSuggestErrorKind::Network
    })?;

    let existing = collect_existing(basic_store, asset_type).await;

    suggest(transport, &api_key, asset_type, &item, &existing).await
}

async fn collect_existing(
    basic_store: &Mutex<AssetStorage>,
    asset_type: AssetType,
) -> ExistingCandidates {
    let store = basic_store.lock().await;

    let avatars = store.get_avatar_store().get_all().await;
    let wearables = store.get_avatar_wearable_store().get_all().await;
    let world_objects = store.get_world_object_store().get_all().await;
    let other_assets = store.get_other_asset_store().get_all().await;

    let categories = match asset_type {
        AssetType::Avatar => Vec::new(),
        AssetType::AvatarWearable => rank_by_usage(wearables.iter().map(|a| &a.category)),
        AssetType::WorldObject => rank_by_usage(world_objects.iter().map(|a| &a.category)),
        AssetType::OtherAsset => rank_by_usage(other_assets.iter().map(|a| &a.category)),
    };

    let supported_avatars = rank_by_usage(wearables.iter().flat_map(|a| &a.supported_avatars));

    let tags = rank_by_usage(
        avatars
            .iter()
            .flat_map(|a| &a.description.tags)
            .chain(wearables.iter().flat_map(|a| &a.description.tags))
            .chain(world_objects.iter().flat_map(|a| &a.description.tags))
            .chain(other_assets.iter().flat_map(|a| &a.description.tags)),
    );

    ExistingCandidates {
        categories,
        supported_avatars,
        tags,
    }
}
