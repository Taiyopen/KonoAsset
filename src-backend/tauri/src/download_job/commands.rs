use std::sync::Arc;

use downloader::{ParsedLine, parse_text};
use tauri::{State, async_runtime::Mutex};
use uuid::Uuid;

use super::definitions::{DownloadImportRequest, DownloadJob};
use super::manager::DownloadJobManager;

#[tauri::command]
#[specta::specta]
pub fn parse_download_links(text: String) -> Vec<ParsedLine> {
    parse_text(&text)
}

#[tauri::command]
#[specta::specta]
pub async fn request_download_import(
    manager: State<'_, Arc<Mutex<DownloadJobManager>>>,
    request: DownloadImportRequest,
) -> Result<Uuid, String> {
    log::info!("Requesting download import: {}", request.pre_asset.name());
    manager.lock().await.enqueue(request)
}

#[tauri::command]
#[specta::specta]
pub async fn list_download_jobs(
    manager: State<'_, Arc<Mutex<DownloadJobManager>>>,
) -> Result<Vec<DownloadJob>, String> {
    Ok(manager.lock().await.list())
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_download_job(
    manager: State<'_, Arc<Mutex<DownloadJobManager>>>,
    id: Uuid,
) -> Result<(), String> {
    manager.lock().await.cancel(id).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_download_job(
    manager: State<'_, Arc<Mutex<DownloadJobManager>>>,
    id: Uuid,
    password: Option<String>,
) -> Result<(), String> {
    manager.lock().await.retry(id, password)
}

#[tauri::command]
#[specta::specta]
pub async fn dismiss_download_job(
    manager: State<'_, Arc<Mutex<DownloadJobManager>>>,
    id: Uuid,
) -> Result<(), String> {
    manager.lock().await.dismiss(id).await
}
