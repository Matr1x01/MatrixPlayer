//! Interface preferences (theme, accent, …) persisted in the settings table.
//!
//! These only affect presentation, but they live in Rust so that they are
//! stored alongside every other setting and validated in one place: each
//! field is a closed enum, so the frontend cannot store arbitrary values.

use serde::{Deserialize, Serialize};

use crate::database::{Database, settings};
use crate::error::{AppError, AppResult};

const PREFIX: &str = "ui.";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    Graphite,
    Violet,
    Cobalt,
    Crimson,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Accent {
    Amber,
    Violet,
    Cobalt,
    Crimson,
    Jade,
    Teal,
    Rose,
    Mono,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Ambient {
    Off,
    #[default]
    Subtle,
    Immersive,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Density {
    #[default]
    Compact,
    Standard,
    Comfortable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UiPreferences {
    pub theme: Theme,
    /// `None` follows the theme's own accent.
    pub accent: Option<Accent>,
    pub ambient: Ambient,
    pub density: Density,
    pub sidebar_expanded: bool,
    pub reduce_motion: bool,
    /// Interface zoom, independent of the desktop scale factor.
    pub interface_scale: f64,
    /// Announce each new track with a desktop notification (while the
    /// window is in the background).
    pub track_notifications: bool,
    /// At startup, bring back the last queue, paused where it was.
    pub restore_session: bool,
    /// How long log files are kept, in days.
    pub log_retention_days: u32,
}

/// Allowed range for [`UiPreferences::interface_scale`] (Settings board).
pub const SCALE_RANGE: std::ops::RangeInclusive<f64> = 0.85..=1.5;
/// Allowed range for [`UiPreferences::log_retention_days`].
pub const RETENTION_RANGE: std::ops::RangeInclusive<u32> = 1..=90;

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            accent: None,
            ambient: Ambient::default(),
            density: Density::default(),
            sidebar_expanded: true,
            reduce_motion: false,
            interface_scale: 1.0,
            track_notifications: true,
            restore_session: true,
            log_retention_days: crate::logging::DEFAULT_RETENTION_DAYS,
        }
    }
}

/// A partial update. `accent: Some(None)` resets the accent to the theme's.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiPreferencesPatch {
    pub theme: Option<Theme>,
    #[serde(default, deserialize_with = "present")]
    pub accent: Option<Option<Accent>>,
    pub ambient: Option<Ambient>,
    pub density: Option<Density>,
    pub sidebar_expanded: Option<bool>,
    pub reduce_motion: Option<bool>,
    pub interface_scale: Option<f64>,
    pub track_notifications: Option<bool>,
    pub restore_session: Option<bool>,
    pub log_retention_days: Option<u32>,
}

/// Distinguishes a field sent as `null` (`Some(None)`) from one not sent at
/// all (`None`, via `#[serde(default)]`).
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

pub async fn load(db: &Database) -> AppResult<UiPreferences> {
    let stored = settings::get_prefixed(db, PREFIX).await?;
    // Unknown or invalid stored fields fall back to their defaults.
    let mut prefs: UiPreferences = serde_json::from_value(serde_json::Value::Object(stored)).unwrap_or_default();
    if !SCALE_RANGE.contains(&prefs.interface_scale) {
        prefs.interface_scale = 1.0;
    }
    if !RETENTION_RANGE.contains(&prefs.log_retention_days) {
        prefs.log_retention_days = crate::logging::DEFAULT_RETENTION_DAYS;
    }
    Ok(prefs)
}

pub async fn update(db: &Database, patch: UiPreferencesPatch) -> AppResult<UiPreferences> {
    let mut prefs = load(db).await?;
    if let Some(theme) = patch.theme {
        prefs.theme = theme;
        settings::set(db, "ui.theme", &theme).await?;
    }
    if let Some(accent) = patch.accent {
        prefs.accent = accent;
        settings::set(db, "ui.accent", &accent).await?;
    }
    if let Some(ambient) = patch.ambient {
        prefs.ambient = ambient;
        settings::set(db, "ui.ambient", &ambient).await?;
    }
    if let Some(density) = patch.density {
        prefs.density = density;
        settings::set(db, "ui.density", &density).await?;
    }
    if let Some(expanded) = patch.sidebar_expanded {
        prefs.sidebar_expanded = expanded;
        settings::set(db, "ui.sidebarExpanded", &expanded).await?;
    }
    if let Some(reduce) = patch.reduce_motion {
        prefs.reduce_motion = reduce;
        settings::set(db, "ui.reduceMotion", &reduce).await?;
    }
    if let Some(on) = patch.track_notifications {
        prefs.track_notifications = on;
        settings::set(db, "ui.trackNotifications", &on).await?;
    }
    if let Some(on) = patch.restore_session {
        prefs.restore_session = on;
        settings::set(db, "ui.restoreSession", &on).await?;
    }
    if let Some(scale) = patch.interface_scale {
        if !SCALE_RANGE.contains(&scale) {
            return Err(AppError::InvalidArgument(format!(
                "interface scale must be between {} and {}",
                SCALE_RANGE.start(),
                SCALE_RANGE.end()
            )));
        }
        prefs.interface_scale = scale;
        settings::set(db, "ui.interfaceScale", &scale).await?;
    }
    if let Some(days) = patch.log_retention_days {
        if !RETENTION_RANGE.contains(&days) {
            return Err(AppError::InvalidArgument(format!(
                "logs can be kept for {} to {} days",
                RETENTION_RANGE.start(),
                RETENTION_RANGE.end()
            )));
        }
        prefs.log_retention_days = days;
        settings::set(db, "ui.logRetentionDays", &days).await?;
    }
    Ok(prefs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn patches_merge_and_persist() {
        let db = Database::in_memory().await.unwrap();
        assert_eq!(load(&db).await.unwrap(), UiPreferences::default());

        let patch: UiPreferencesPatch = serde_json::from_str(r#"{"theme":"cobalt","accent":"jade"}"#).unwrap();
        update(&db, patch).await.unwrap();
        let patch: UiPreferencesPatch = serde_json::from_str(r#"{"sidebarExpanded":false}"#).unwrap();
        let prefs = update(&db, patch).await.unwrap();

        assert_eq!(prefs.theme, Theme::Cobalt);
        assert_eq!(prefs.accent, Some(Accent::Jade));
        assert!(!prefs.sidebar_expanded);
        assert_eq!(load(&db).await.unwrap(), prefs);

        let reset: UiPreferencesPatch = serde_json::from_str(r#"{"accent":null}"#).unwrap();
        assert_eq!(update(&db, reset).await.unwrap().accent, None);
    }

    #[tokio::test]
    async fn validates_interface_scale() {
        let db = Database::in_memory().await.unwrap();
        let bad: UiPreferencesPatch = serde_json::from_str(r#"{"interfaceScale":3.0}"#).unwrap();
        assert!(update(&db, bad).await.is_err());
        let good: UiPreferencesPatch = serde_json::from_str(r#"{"interfaceScale":1.25}"#).unwrap();
        assert_eq!(update(&db, good).await.unwrap().interface_scale, 1.25);
    }

    #[tokio::test]
    async fn validates_log_retention() {
        let db = Database::in_memory().await.unwrap();
        assert_eq!(load(&db).await.unwrap().log_retention_days, 7);
        for days in [0, 91] {
            let bad: UiPreferencesPatch =
                serde_json::from_value(serde_json::json!({ "logRetentionDays": days })).unwrap();
            assert!(update(&db, bad).await.is_err(), "{days}");
        }
        let good: UiPreferencesPatch = serde_json::from_str(r#"{"logRetentionDays":30}"#).unwrap();
        update(&db, good).await.unwrap();
        assert_eq!(load(&db).await.unwrap().log_retention_days, 30);
    }

    #[test]
    fn rejects_unknown_values_and_fields() {
        assert!(serde_json::from_str::<UiPreferencesPatch>(r#"{"theme":"neon"}"#).is_err());
        assert!(serde_json::from_str::<UiPreferencesPatch>(r#"{"volume":1}"#).is_err());
    }
}
