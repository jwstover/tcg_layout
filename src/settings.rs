use crate::types::LayoutParams;
use crate::ui::{CardSizeOption, PageSizeOption};
use anyhow::{anyhow, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const APP_NAME: &str = "tcg_layout";
const SETTINGS_FILE: &str = "settings.json";
const KEYRING_SERVICE: &str = "tcg_layout";
const OPENAI_KEY_USERNAME: &str = "openai_api_key";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub layout_params: LayoutParams,
    pub page_size_option: PageSizeOption,
    pub card_size_option: CardSizeOption,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            layout_params: LayoutParams::default(),
            page_size_option: PageSizeOption::A4,
            card_size_option: CardSizeOption::Poker,
        }
    }
}

pub struct SettingsManager {
    settings_path: PathBuf,
    keyring_entry: Entry,
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

        Ok(Self {
            settings_path,
            keyring_entry,
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

    pub fn save_openai_api_key(&self, api_key: &str) -> Result<()> {
        if api_key.trim().is_empty() {
            // Delete the key if empty string is provided
            match self.keyring_entry.delete_password() {
                Ok(()) => log::debug!("OpenAI API key deleted from keyring"),
                Err(keyring::Error::NoEntry) => {
                    log::debug!("No OpenAI API key to delete from keyring")
                }
                Err(e) => return Err(anyhow!("Failed to delete API key from keyring: {}", e)),
            }
        } else {
            self.keyring_entry.set_password(api_key)?;
            log::debug!("OpenAI API key saved to keyring");
        }
        Ok(())
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

        let manager = SettingsManager {
            settings_path,
            keyring_entry,
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
