use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use downloader::{Ctx, LineError, LinkLine, Progress, client, download_all, sites};
use model::preference::PreferenceStore;
use storage::asset_storage::AssetStorage;
use tauri::{AppHandle, Manager, async_runtime::Mutex};

use crate::definitions::import_request::{AssetImportRequest, PreAsset};
use crate::importer::import_wrapper::{
    import_avatar, import_avatar_wearable, import_other_asset, import_world_object,
};

use super::definitions::DownloadPreAsset;
use super::manager::JobBackend;

/// Downloads with the `downloader` crate and imports with the regular importer.
pub struct TauriJobBackend {
    app: AppHandle,
}

impl TauriJobBackend {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait]
impl JobBackend for TauriJobBackend {
    async fn download(
        &self,
        lines: Vec<LinkLine>,
        out_dir: PathBuf,
        progress: Arc<dyn Fn(Progress) + Send + Sync>,
    ) -> Result<Vec<PathBuf>, LineError> {
        let ctx = Ctx {
            app: Some(self.app.clone()),
            http: client(),
        };
        download_all(&ctx, &sites::create, &lines, &out_dir, progress.as_ref()).await
    }

    async fn import(&self, pre_asset: DownloadPreAsset, paths: Vec<PathBuf>) -> Result<(), String> {
        // Not managed when the data failed to load at startup.
        let storage = self
            .app
            .try_state::<Arc<Mutex<AssetStorage>>>()
            .ok_or("Asset storage is not loaded")?
            .inner()
            .clone();
        let zip_extraction = self
            .app
            .try_state::<Arc<Mutex<PreferenceStore>>>()
            .ok_or("Preferences are not loaded")?
            .lock()
            .await
            .zip_extraction;

        let absolute_paths: Vec<String> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        log::info!("Importing downloaded files: {:?}", absolute_paths);

        let storage = storage.lock().await;
        match pre_asset {
            DownloadPreAsset::Avatar(pre) => import_avatar(
                &storage,
                request(pre, absolute_paths),
                None,
                zip_extraction,
                false,
            )
            .await
            .map(|_| ()),
            DownloadPreAsset::AvatarWearable(pre) => import_avatar_wearable(
                &storage,
                request(pre, absolute_paths),
                None,
                zip_extraction,
                false,
            )
            .await
            .map(|_| ()),
            DownloadPreAsset::WorldObject(pre) => import_world_object(
                &storage,
                request(pre, absolute_paths),
                None,
                zip_extraction,
                false,
            )
            .await
            .map(|_| ()),
            DownloadPreAsset::OtherAsset(pre) => import_other_asset(
                &storage,
                request(pre, absolute_paths),
                None,
                zip_extraction,
                false,
            )
            .await
            .map(|_| ()),
        }
    }
}

/// The downloaded files live in the job folder, so they are always deleted after the import.
fn request<T: PreAsset>(pre_asset: T, absolute_paths: Vec<String>) -> AssetImportRequest<T> {
    AssetImportRequest {
        pre_asset,
        absolute_paths,
        delete_source: true,
    }
}
