//! Services shared with Tauri commands through managed state.

use std::path::PathBuf;
use std::sync::Arc;

use crate::artwork::ArtworkStore;
use crate::audio::waveform::WaveformCache;
use crate::database::Database;
use crate::dsp::service::DspService;
use crate::error::AppResult;
use crate::library::LibraryHandle;
use crate::player::{InsertMode, PlayerCommand, PlayerHandle};

pub struct AppState {
    pub player: PlayerHandle,
    pub library: LibraryHandle,
    pub artwork: Arc<ArtworkStore>,
    pub dsp: Arc<DspService>,
    pub waveforms: Arc<WaveformCache>,
    pub db: Database,
}

impl AppState {
    /// Queues files and folders opened from outside the library views (the
    /// open dialogs, drag and drop, the command line). Files that are in the
    /// library keep their library identity. Returns how many were queued.
    pub async fn open_paths(&self, paths: Vec<PathBuf>, mode: InsertMode) -> AppResult<usize> {
        let tracks = self.library.resolve_files(self.artwork.clone(), paths).await?;
        let count = tracks.len();
        self.player.command(PlayerCommand::Insert { tracks, mode, start: 0 }).await?;
        Ok(count)
    }
}
