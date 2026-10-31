use downloader::{DownloadErrorKind, SiteKind};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::definitions::import_request::{
    PreAvatar, PreAvatarWearable, PreOtherAsset, PreWorldObject,
};

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, specta::Type)]
pub enum DownloadJobStatus {
    Queued,
    Downloading,
    Importing,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadJobStatus {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            DownloadJobStatus::Queued
                | DownloadJobStatus::Downloading
                | DownloadJobStatus::Importing
        )
    }
}

#[derive(Serialize, Debug, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJob {
    pub id: Uuid,
    pub asset_name: String,
    pub status: DownloadJobStatus,
    /// Site of the first link.
    pub site: SiteKind,
    pub current_file: Option<String>,
    /// 1-based index over all files of the job.
    pub file_index: u32,
    /// Known once every line has been listed.
    pub file_count: Option<u32>,
    pub downloaded_bytes: u64,
    /// Size of the current file, `None` when unknown.
    pub total_bytes: Option<u64>,
    pub error: Option<String>,
    pub error_kind: Option<DownloadErrorKind>,
    /// Input line that failed to download; `None` for import failures.
    pub failed_line: Option<u32>,
    pub created_at: i64,
}

#[derive(Deserialize, Debug, Clone, specta::Type)]
pub enum DownloadPreAsset {
    Avatar(PreAvatar),
    AvatarWearable(PreAvatarWearable),
    WorldObject(PreWorldObject),
    OtherAsset(PreOtherAsset),
}

impl DownloadPreAsset {
    pub fn name(&self) -> &str {
        match self {
            DownloadPreAsset::Avatar(pre) => &pre.description.name,
            DownloadPreAsset::AvatarWearable(pre) => &pre.description.name,
            DownloadPreAsset::WorldObject(pre) => &pre.description.name,
            DownloadPreAsset::OtherAsset(pre) => &pre.description.name,
        }
    }
}

#[derive(Deserialize, Debug, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadImportRequest {
    pub links_text: String,
    pub pre_asset: DownloadPreAsset,
}

#[derive(Serialize, Debug, Clone, specta::Type, tauri_specta::Event)]
pub struct DownloadJobUpdated(pub DownloadJob);
