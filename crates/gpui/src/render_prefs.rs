use std::sync::OnceLock;

#[cfg(target_os = "linux")]
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AntialiasingMode {
    Default,
    Binary,
    Reduced,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AntialiasingPrefs {
    pub(crate) buffer: AntialiasingProfile,
    pub(crate) ui: AntialiasingProfile,
}

impl Default for AntialiasingPrefs {
    fn default() -> Self {
        Self {
            buffer: AntialiasingProfile::default_buffer(),
            ui: AntialiasingProfile::default_ui(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AntialiasingProfile {
    pub(crate) mode: AntialiasingMode,
    pub(crate) binary_threshold: u8,
    pub(crate) reduced_levels: u8,
    pub(crate) disable_subpixel_positioning: bool,
}

impl AntialiasingProfile {
    pub const fn default_buffer() -> Self {
        Self {
            mode: AntialiasingMode::Binary,
            binary_threshold: 96,
            reduced_levels: 4,
            disable_subpixel_positioning: true,
        }
    }

    pub const fn default_ui() -> Self {
        Self {
            mode: AntialiasingMode::Default,
            binary_threshold: 96,
            reduced_levels: 4,
            disable_subpixel_positioning: false,
        }
    }

    pub fn apply_env_overrides(&mut self, prefix: &str) {
        if let Some(value) = std::env::var(format!("{prefix}_MODE"))
            .ok()
            .and_then(|value| parse_antialiasing_mode(value.trim()))
        {
            self.mode = value;
        }

        if let Some(value) = std::env::var(format!("{prefix}_BINARY_THRESHOLD"))
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
        {
            self.binary_threshold = value.min(u8::MAX as u16) as u8;
        }

        if let Some(value) = std::env::var(format!("{prefix}_REDUCED_LEVELS"))
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
        {
            self.reduced_levels = clamp_reduced_levels(value as u8);
        }

        if std::env::var(format!("{prefix}_DISABLE_SUBPIXEL_POSITIONING"))
            .map(|value| value != "0")
            .unwrap_or(false)
        {
            self.disable_subpixel_positioning = true;
        }
    }

    pub fn apply_config(&mut self, config: &ProfileConfig) {
        if let Some(mode) = config.mode.as_deref().and_then(parse_antialiasing_mode) {
            self.mode = mode;
        }

        if let Some(threshold) = config.binary_threshold {
            self.binary_threshold = threshold;
        }

        if let Some(levels) = config.reduced_levels {
            self.reduced_levels = clamp_reduced_levels(levels);
        }

        if let Some(disable) = config.disable_subpixel_positioning {
            self.disable_subpixel_positioning = disable;
        }
    }
}

fn parse_antialiasing_mode(value: &str) -> Option<AntialiasingMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "default" | "aa" | "antialias" | "antialiasing" => Some(AntialiasingMode::Default),
        "binary" | "mono" | "monochrome" | "none" | "off" | "disable" | "disabled" | "noaa" => {
            Some(AntialiasingMode::Binary)
        }
        "reduced" | "low" | "steps" | "quantized" | "quantised" => Some(AntialiasingMode::Reduced),
        _ => None,
    }
}

fn clamp_reduced_levels(levels: u8) -> u8 {
    levels.clamp(2, 8)
}

#[cfg_attr(target_os = "linux", derive(Deserialize))]
#[derive(Default)]
pub(crate) struct ProfileConfig {
    #[cfg_attr(target_os = "linux", serde(default))]
    mode: Option<String>,
    #[cfg_attr(target_os = "linux", serde(default))]
    binary_threshold: Option<u8>,
    #[cfg_attr(target_os = "linux", serde(default))]
    reduced_levels: Option<u8>,
    #[cfg_attr(target_os = "linux", serde(default))]
    disable_subpixel_positioning: Option<bool>,
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{AntialiasingPrefs, ProfileConfig};
    use serde::Deserialize;
    use std::{env, fs, path::PathBuf};

    #[derive(Default, Deserialize)]
    struct AntialiasingConfig {
        #[serde(default)]
        buffer: Option<ProfileConfig>,
        #[serde(default)]
        ui: Option<ProfileConfig>,
    }

    pub(super) fn load_prefs() -> AntialiasingPrefs {
        let mut prefs = AntialiasingPrefs::default();

        prefs.buffer.apply_env_overrides("ZED_ANTIALIASING");
        prefs.ui.apply_env_overrides("ZED_UI_ANTIALIASING");

        if let Some(path) = antialiasing_config_path() {
            if let Ok(contents) = fs::read_to_string(path) {
                if let Ok(config) = serde_json::from_str::<AntialiasingConfig>(&contents) {
                    if let Some(buffer) = config.buffer {
                        prefs.buffer.apply_config(&buffer);
                    }
                    if let Some(ui) = config.ui {
                        prefs.ui.apply_config(&ui);
                    }
                }
            }
        }

        log::info!(
            "Antialiasing profiles loaded: buffer={:?}, ui={:?}",
            prefs.buffer,
            prefs.ui
        );

        prefs
    }

    fn antialiasing_config_path() -> Option<PathBuf> {
        let mut path = PathBuf::from(env::var_os("HOME")?);
        path.push(".config");
        path.push("zed");
        path.push("antialiasing.json");
        Some(path)
    }

    // clamp/parse helpers defined at top level
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::AntialiasingPrefs;

    pub(super) fn load_prefs() -> AntialiasingPrefs {
        AntialiasingPrefs::default()
    }
}

pub(crate) fn antialiasing_prefs() -> &'static AntialiasingPrefs {
    static PREFS: OnceLock<AntialiasingPrefs> = OnceLock::new();
    PREFS.get_or_init(platform::load_prefs)
}

pub(crate) fn buffer_antialiasing() -> &'static AntialiasingProfile {
    &antialiasing_prefs().buffer
}

pub(crate) fn ui_antialiasing() -> &'static AntialiasingProfile {
    &antialiasing_prefs().ui
}
