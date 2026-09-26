//! Services shared with Tauri commands through managed state.

use crate::database::Database;
use crate::player::PlayerHandle;

pub struct AppState {
    pub player: PlayerHandle,
    pub db: Database,
}
