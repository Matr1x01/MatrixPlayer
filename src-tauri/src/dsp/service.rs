//! DSP settings and presets: loading, applying and saving them.
//!
//! Settings change many times a second while a slider is dragged. Each
//! change reaches the audio thread immediately; writing to the database is
//! debounced so a drag costs one write.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use super::DspControl;
use super::processor::DspStatus;
use super::settings::{DspSettings, Preset, builtin_presets};
use crate::database::{Database, settings};
use crate::error::{AppError, AppResult};

const SETTINGS_KEY: &str = "dsp.settings";
const PRESETS_KEY: &str = "dsp.presets";
const SAVE_DELAY: Duration = Duration::from_millis(600);
const MAX_USER_PRESETS: usize = 200;

/// Everything the Equalizer and Audio / DSP views show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DspState {
    pub settings: DspSettings,
    /// The user's presets, then the built-in ones.
    pub presets: Vec<Preset>,
    pub status: DspStatus,
}

/// The file format of exported presets.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresetFile {
    app: String,
    version: u32,
    presets: Vec<Preset>,
}

pub struct DspService {
    control: Arc<DspControl>,
    db: Database,
    user_presets: Mutex<Vec<Preset>>,
    dirty: AtomicBool,
    wake: Arc<Notify>,
}

impl DspService {
    /// Loads the saved settings, applies them, and starts the save task.
    pub async fn start(db: Database, control: Arc<DspControl>) -> Arc<Self> {
        let saved: DspSettings = settings::get(&db, SETTINGS_KEY).await.ok().flatten().unwrap_or_default();
        let user: Vec<Preset> = settings::get(&db, PRESETS_KEY).await.ok().flatten().unwrap_or_default();
        control.apply(&saved.migrate().sanitize());
        let service = Arc::new(Self {
            control,
            db,
            user_presets: Mutex::new(user.into_iter().map(|p| Preset { builtin: false, ..p }).collect()),
            dirty: AtomicBool::new(false),
            wake: Arc::new(Notify::new()),
        });
        let (weak, wake) = (Arc::downgrade(&service), service.wake.clone());
        tokio::spawn(async move {
            loop {
                wake.notified().await;
                tokio::time::sleep(SAVE_DELAY).await;
                let Some(service) = weak.upgrade() else { break };
                service.flush().await;
            }
        });
        service
    }

    pub fn control(&self) -> &Arc<DspControl> {
        &self.control
    }

    pub fn state(&self) -> DspState {
        DspState { settings: self.control.settings(), presets: self.presets(), status: self.control.status() }
    }

    pub fn presets(&self) -> Vec<Preset> {
        let mut all = self.user_presets.lock().unwrap().clone();
        all.extend(builtin_presets());
        all
    }

    /// Applies new settings and schedules a save. Returns them as applied.
    pub fn update(&self, settings: DspSettings) -> DspSettings {
        // Whatever the UI sends is already in the current format.
        let settings = DspSettings { version: super::settings::SETTINGS_VERSION, ..settings }.sanitize();
        self.control.apply(&settings);
        self.dirty.store(true, Ordering::SeqCst);
        self.wake.notify_one();
        settings
    }

    /// Resets everything but the equalizer to the defaults.
    pub fn restore_defaults(&self) -> DspSettings {
        let current = self.control.settings();
        let defaults = DspSettings {
            eq_enabled: current.eq_enabled,
            slot: current.slot,
            bypass: current.bypass,
            a: current.a,
            b: current.b,
            preset: current.preset,
            ..DspSettings::default()
        };
        self.update(defaults)
    }

    /// Saves the active curve as a user preset (replacing one of the same
    /// name) and marks it as the loaded preset.
    pub async fn save_preset(&self, name: &str) -> AppResult<DspState> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 {
            return Err(AppError::InvalidArgument("preset names are 1–60 characters".into()));
        }
        if builtin_presets().iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
            return Err(AppError::InvalidArgument(format!("“{name}” is a built-in preset; choose another name")));
        }
        let mut settings = self.control.settings();
        let preset = Preset { name: name.to_owned(), curve: settings.curve().clone(), builtin: false };
        {
            let mut user = self.user_presets.lock().unwrap();
            match user.iter_mut().find(|p| p.name.eq_ignore_ascii_case(name)) {
                Some(existing) => *existing = preset,
                None => user.insert(0, preset),
            }
        }
        self.save_presets().await?;
        settings.preset = Some(name.to_owned());
        self.update(settings);
        Ok(self.state())
    }

    pub async fn delete_preset(&self, name: &str) -> AppResult<DspState> {
        self.user_presets.lock().unwrap().retain(|p| p.name != name);
        self.save_presets().await?;
        let mut settings = self.control.settings();
        if settings.preset.as_deref() == Some(name) {
            settings.preset = None;
            self.update(settings);
        }
        Ok(self.state())
    }

    /// Writes the user's presets to a JSON file. Returns how many.
    pub async fn export_presets(&self, path: &Path) -> AppResult<usize> {
        let presets = self.user_presets.lock().unwrap().clone();
        let file = PresetFile { app: "MatrixPlayer".into(), version: 1, presets };
        let json = serde_json::to_vec_pretty(&file).expect("presets serialise to JSON");
        tokio::fs::write(path, json).await?;
        Ok(file.presets.len())
    }

    /// Adds the presets in a JSON file, replacing same-named ones. Accepts
    /// an exported file or a bare list of presets.
    pub async fn import_presets(&self, path: &Path) -> AppResult<usize> {
        let bytes = tokio::fs::read(path).await?;
        let presets = serde_json::from_slice::<PresetFile>(&bytes)
            .map(|f| f.presets)
            .or_else(|_| serde_json::from_slice::<Vec<Preset>>(&bytes))
            .map_err(|_| AppError::InvalidArgument("the file does not contain MatrixPlayer presets".into()))?;
        let builtin = builtin_presets();
        let mut count = 0;
        {
            let mut user = self.user_presets.lock().unwrap();
            for preset in presets {
                let name = preset.name.trim().chars().take(60).collect::<String>();
                if name.is_empty() || builtin.iter().any(|b| b.name.eq_ignore_ascii_case(&name)) {
                    continue;
                }
                let curve = DspSettings { a: preset.curve, ..DspSettings::default() }.sanitize().a;
                let preset = Preset { name, curve, builtin: false };
                match user.iter_mut().find(|p| p.name.eq_ignore_ascii_case(&preset.name)) {
                    Some(existing) => *existing = preset,
                    None => user.push(preset),
                }
                count += 1;
            }
            user.truncate(MAX_USER_PRESETS);
        }
        self.save_presets().await?;
        Ok(count)
    }

    /// Writes pending settings now.
    pub async fn flush(&self) {
        if self.dirty.swap(false, Ordering::SeqCst)
            && let Err(e) = settings::set(&self.db, SETTINGS_KEY, &self.control.settings()).await
        {
            log::warn!("could not save DSP settings: {e}");
            self.dirty.store(true, Ordering::SeqCst);
        }
    }

    async fn save_presets(&self) -> AppResult<()> {
        let presets = self.user_presets.lock().unwrap().clone();
        settings::set(&self.db, PRESETS_KEY, &presets).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::DspChain;
    use crate::dsp::settings::{EqMode, ReplayGainMode};

    async fn service(db: &Database) -> (DspChain, Arc<DspService>) {
        gstreamer::init().unwrap();
        let chain = DspChain::build().unwrap();
        let service = DspService::start(db.clone(), chain.control()).await;
        (chain, service)
    }

    #[tokio::test]
    async fn settings_and_presets_survive_a_restart() {
        let db = Database::in_memory().await.unwrap();
        let (_chain, dsp) = service(&db).await;
        let mut s = dsp.state().settings;
        s.eq_enabled = true;
        s.a.mode = EqMode::Graphic10;
        s.a.graphic10[0] = 4.0;
        s.replay_gain = ReplayGainMode::Track;
        dsp.update(s);
        dsp.save_preset("Warm Headphone").await.unwrap();
        dsp.flush().await;

        let (_chain, again) = service(&db).await;
        let state = again.state();
        assert!(state.settings.eq_enabled);
        assert_eq!(state.settings.replay_gain, ReplayGainMode::Track);
        assert_eq!(state.settings.preset.as_deref(), Some("Warm Headphone"));
        assert_eq!(state.presets[0].name, "Warm Headphone");
        assert_eq!(state.presets[0].curve.graphic10[0], 4.0);
        assert!(state.presets.iter().any(|p| p.builtin && p.name == "Flat"));
    }

    #[tokio::test]
    async fn built_in_names_are_reserved_and_defaults_keep_the_eq() {
        let db = Database::in_memory().await.unwrap();
        let (_chain, dsp) = service(&db).await;
        assert!(dsp.save_preset("flat").await.is_err());

        let mut s = dsp.state().settings;
        s.eq_enabled = true;
        s.balance = 0.5;
        dsp.update(s);
        let restored = dsp.restore_defaults();
        assert!(restored.eq_enabled);
        assert_eq!(restored.balance, 0.0);
    }

    #[tokio::test]
    async fn presets_round_trip_through_a_file() {
        let db = Database::in_memory().await.unwrap();
        let (_chain, dsp) = service(&db).await;
        dsp.save_preset("Mine").await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("presets.json");
        assert_eq!(dsp.export_presets(&file).await.unwrap(), 1);

        let other = Database::in_memory().await.unwrap();
        let (_chain, fresh) = service(&other).await;
        assert_eq!(fresh.import_presets(&file).await.unwrap(), 1);
        assert_eq!(fresh.presets()[0].name, "Mine");
        std::fs::write(&file, b"{}").unwrap();
        assert!(fresh.import_presets(&file).await.is_err());
    }
}
