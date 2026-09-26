//! `art://` URI scheme: serves cached artwork by id to the webview.

use tauri::http::{Response, StatusCode, header};

use super::ArtworkStore;

/// Sent if a request arrives before the store exists (during startup).
pub fn unavailable() -> Response<Vec<u8>> {
    Response::builder().status(StatusCode::SERVICE_UNAVAILABLE).body(Vec::new()).unwrap()
}

/// Resolves `art://localhost/<id>` against the artwork store.
///
/// Ids are content hashes, so responses are immutable and cached forever.
pub fn respond(store: &ArtworkStore, uri_path: &str) -> Response<Vec<u8>> {
    let id = uri_path.trim_start_matches('/');
    let Some(bytes) = store.file_for(id).and_then(|p| std::fs::read(p).ok()) else {
        return Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()).unwrap();
    };
    let mime = image::guess_format(&bytes).map(|f| f.to_mime_type()).unwrap_or("application/octet-stream");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(bytes)
        .unwrap()
}
