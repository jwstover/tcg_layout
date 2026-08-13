use crate::types::LayoutParams;
use crate::ui::{CardSizeOption, PageSizeOption};
use anyhow::{anyhow, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub fn default_marvel_champions_dir() -> PathBuf {
    dirs::picture_dir()
        .unwrap_or_else(|| PathBuf::from("~/Pictures"))
        .join("Marvel Champions")
}

const APP_NAME: &str = "tcg_layout";
const SETTINGS_FILE: &str = "settings.json";
const KEYRING_SERVICE: &str = "tcg_layout";
const OPENAI_KEY_USERNAME: &str = "openai_api_key";
const GOOGLE_DRIVE_KEY_USERNAME: &str = "google_drive_api_key";

/// Builds a fresh keyring entry and writes (or, for an empty key, deletes)
/// the password. Building a new `Entry` here (rather than reusing one held
/// on `self`) keeps this free of any borrow on the manager, so it can run
/// on a `spawn_blocking` thread independent of the caller's lifetime.
fn save_key_to_keyring(username: &str, api_key: &str) -> Result<()> {
    let entry = Entry::new(KEYRING_SERVICE, username)?;
    if api_key.trim().is_empty() {
        match entry.delete_password() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow!("Failed to delete key from keyring: {}", e)),
        }
    } else {
        entry.set_password(api_key)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub layout_params: LayoutParams,
    pub page_size_option: PageSizeOption,
    pub card_size_option: CardSizeOption,
    #[serde(default)]
    pub marvel_champions_dir: Option<PathBuf>,
    #[serde(default)]
    pub google_drive_folder_id: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            layout_params: LayoutParams::default(),
            page_size_option: PageSizeOption::A4,
            card_size_option: CardSizeOption::Poker,
            marvel_champions_dir: None,
            google_drive_folder_id: None,
        }
    }
}

pub struct SettingsManager {
    settings_path: PathBuf,
    keyring_entry: Entry,
    google_drive_keyring_entry: Entry,
}

impl SettingsManager {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow!("Could not find config directory"))?
            .join(APP_NAME);

        // Create config directory if it doesn't exist
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)?;
        }

        let settings_path = config_dir.join(SETTINGS_FILE);
        let keyring_entry = Entry::new(KEYRING_SERVICE, OPENAI_KEY_USERNAME)?;
        let google_drive_keyring_entry = Entry::new(KEYRING_SERVICE, GOOGLE_DRIVE_KEY_USERNAME)?;

        Ok(Self {
            settings_path,
            keyring_entry,
            google_drive_keyring_entry,
        })
    }

    pub fn load_settings(&self) -> AppSettings {
        match self.try_load_settings() {
            Ok(settings) => settings,
            Err(e) => {
                log::warn!("Failed to load settings, using defaults: {e}");
                AppSettings::default()
            }
        }
    }

    fn try_load_settings(&self) -> Result<AppSettings> {
        if !self.settings_path.exists() {
            return Ok(AppSettings::default());
        }

        let contents = fs::read_to_string(&self.settings_path)?;
        let settings: AppSettings = serde_json::from_str(&contents)?;
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let json = serde_json::to_string_pretty(settings)?;
        fs::write(&self.settings_path, json)?;
        log::debug!("Settings saved to {:?}", self.settings_path);
        Ok(())
    }

    pub fn load_openai_api_key(&self) -> Option<String> {
        match self.keyring_entry.get_password() {
            Ok(password) => Some(password),
            Err(keyring::Error::NoEntry) => {
                log::debug!("No OpenAI API key found in keyring");
                None
            }
            Err(e) => {
                log::warn!("Failed to load OpenAI API key from keyring: {e}");
                None
            }
        }
    }

    /// Persists the key to the OS keyring on a background thread. Keyring
    /// backends (e.g. the Secret Service D-Bus API on Linux) can block for a
    /// long time — sometimes on a system prompt (gnome-keyring's
    /// gcr-prompter) that isn't visible or focused — so this must never run
    /// on the UI thread.
    pub fn save_openai_api_key(&self, api_key: &str) {
        let api_key = api_key.to_string();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = save_key_to_keyring(OPENAI_KEY_USERNAME, &api_key) {
                log::error!("Failed to save OpenAI API key: {e}");
            } else {
                log::debug!("OpenAI API key saved to keyring");
            }
        });
    }

    pub fn delete_openai_api_key(&self) -> Result<()> {
        match self.keyring_entry.delete_password() {
            Ok(()) => {
                log::debug!("OpenAI API key deleted from keyring");
                Ok(())
            }
            Err(keyring::Error::NoEntry) => {
                log::debug!("No OpenAI API key to delete from keyring");
                Ok(())
            }
            Err(e) => Err(anyhow!("Failed to delete API key from keyring: {}", e)),
        }
    }

    pub fn load_google_drive_api_key(&self) -> Option<String> {
        match self.google_drive_keyring_entry.get_password() {
            Ok(password) => Some(password),
            Err(keyring::Error::NoEntry) => {
                log::debug!("No Google Drive API key found in keyring");
                None
            }
            Err(e) => {
                log::warn!("Failed to load Google Drive API key from keyring: {e}");
                None
            }
        }
    }

    /// See [`Self::save_openai_api_key`] for why this runs off the UI thread.
    pub fn save_google_drive_api_key(&self, api_key: &str) {
        let api_key = api_key.to_string();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = save_key_to_keyring(GOOGLE_DRIVE_KEY_USERNAME, &api_key) {
                log::error!("Failed to save Google Drive API key: {e}");
            } else {
                log::debug!("Google Drive API key saved to keyring");
            }
        });
    }

    pub fn get_settings_path(&self) -> &PathBuf {
        &self.settings_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_settings_manager() -> (SettingsManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let settings_path = temp_dir.path().join(SETTINGS_FILE);
        let keyring_entry = Entry::new("test_tcg_layout", "test_key").unwrap();
        let google_drive_keyring_entry =
            Entry::new("test_tcg_layout", "test_google_drive_key").unwrap();

        let manager = SettingsManager {
            settings_path,
            keyring_entry,
            google_drive_keyring_entry,
        };

        (manager, temp_dir)
    }

    #[test]
    fn test_default_settings() {
        let settings = AppSettings::default();
        assert_eq!(settings.layout_params, LayoutParams::default());
        assert_eq!(settings.page_size_option, PageSizeOption::A4);
        assert_eq!(settings.card_size_option, CardSizeOption::Poker);
    }

    #[test]
    fn test_save_and_load_settings() {
        let (manager, _temp_dir) = create_test_settings_manager();

        let mut settings = AppSettings::default();
        settings.layout_params.target_dpi = 600;
        settings.page_size_option = PageSizeOption::USLetter;
        settings.card_size_option = CardSizeOption::Bridge;

        // Save settings
        manager.save_settings(&settings).unwrap();

        // Load settings
        let loaded_settings = manager.load_settings();
        assert_eq!(loaded_settings.layout_params.target_dpi, 600);
        assert_eq!(loaded_settings.page_size_option, PageSizeOption::USLetter);
        assert_eq!(loaded_settings.card_size_option, CardSizeOption::Bridge);
    }

    #[test]
    fn test_load_nonexistent_settings() {
        let (manager, _temp_dir) = create_test_settings_manager();

        // Should return default settings when file doesn't exist
        let loaded_settings = manager.load_settings();
        assert_eq!(loaded_settings.layout_params, LayoutParams::default());
        assert_eq!(loaded_settings.page_size_option, PageSizeOption::A4);
        assert_eq!(loaded_settings.card_size_option, CardSizeOption::Poker);
    }

    #[test]
    fn test_settings_backwards_compatible_without_sharpen_fields() {
        // Settings saved by an older version won't have the sharpen fields;
        // they should deserialize with defaults
        let settings = AppSettings::default();
        let mut value = serde_json::to_value(&settings).unwrap();
        let params = value["layout_params"].as_object_mut().unwrap();
        params.remove("sharpen_amount");
        params.remove("sharpen_radius");
        params.remove("sharpen_threshold");
        params.remove("enable_sharpen");

        let loaded: AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.layout_params.sharpen_amount, 1.0);
        assert_eq!(loaded.layout_params.sharpen_radius, 0.7);
        assert_eq!(loaded.layout_params.sharpen_threshold, 0.02);
        assert!(!loaded.layout_params.enable_sharpen);
    }

    #[test]
    fn test_settings_backwards_compatible_keeps_existing_sharpen_amount() {
        // A settings file from the version that had only `sharpen_amount`
        // should keep that amount and pick up defaults for the new knobs
        let settings = AppSettings::default();
        let mut value = serde_json::to_value(&settings).unwrap();
        let params = value["layout_params"].as_object_mut().unwrap();
        params.insert("sharpen_amount".to_string(), serde_json::json!(2.25));
        params.insert("enable_sharpen".to_string(), serde_json::json!(true));
        params.remove("sharpen_radius");
        params.remove("sharpen_threshold");

        let loaded: AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.layout_params.sharpen_amount, 2.25);
        assert!(loaded.layout_params.enable_sharpen);
        assert_eq!(loaded.layout_params.sharpen_radius, 0.7);
        assert_eq!(loaded.layout_params.sharpen_threshold, 0.02);
    }

    #[test]
    fn test_settings_backwards_compatible_without_duplex_fields() {
        // Settings saved by an older version won't have the duplex fields;
        // they should deserialize with defaults
        let settings = AppSettings::default();
        let mut value = serde_json::to_value(&settings).unwrap();
        let params = value["layout_params"].as_object_mut().unwrap();
        params.remove("enable_duplex");
        params.remove("flip_edge");
        params.remove("back_offset");
        params.remove("default_back_path");

        let loaded: AppSettings = serde_json::from_value(value).unwrap();
        assert!(!loaded.layout_params.enable_duplex);
        assert_eq!(
            loaded.layout_params.flip_edge,
            crate::types::FlipEdge::LongEdge
        );
        assert_eq!(loaded.layout_params.back_offset, (0.0, 0.0));
        assert_eq!(loaded.layout_params.default_back_path, None);
    }

    #[test]
    fn test_settings_backwards_compatible_without_printer_margin_fields() {
        // Settings saved before printer margins existed must load with the
        // feature off, so an existing layout is unchanged by the upgrade
        let settings = AppSettings::default();
        let mut value = serde_json::to_value(&settings).unwrap();
        let params = value["layout_params"].as_object_mut().unwrap();
        params.remove("printer_margins");
        params.remove("enable_printer_margins");

        let loaded: AppSettings = serde_json::from_value(value).unwrap();
        assert!(!loaded.layout_params.enable_printer_margins);
        assert_eq!(
            loaded.layout_params.effective_printer_margins(),
            crate::types::Margins::uniform(0.0)
        );
        // The stored value is still a usable starting point for the UI
        assert!(loaded.layout_params.printer_margins.top > 0.0);
    }

    #[test]
    fn test_settings_round_trip_printer_margins() {
        let mut settings = AppSettings::default();
        settings.layout_params.enable_printer_margins = true;
        settings.layout_params.printer_margins = crate::types::Margins {
            top: 3.0,
            right: 3.2,
            bottom: 12.7,
            left: 3.4,
        };

        let json = serde_json::to_string(&settings).unwrap();
        let loaded: AppSettings = serde_json::from_str(&json).unwrap();

        assert!(loaded.layout_params.enable_printer_margins);
        assert_eq!(
            loaded.layout_params.printer_margins,
            settings.layout_params.printer_margins
        );
    }

    #[test]
    fn test_settings_serialization() {
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(
            settings.layout_params.page_size,
            deserialized.layout_params.page_size
        );
        assert_eq!(
            settings.layout_params.card_size,
            deserialized.layout_params.card_size
        );
        assert_eq!(
            settings.layout_params.target_dpi,
            deserialized.layout_params.target_dpi
        );
    }
}
