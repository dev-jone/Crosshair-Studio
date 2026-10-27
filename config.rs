use crate::crosshair::{CrosshairAppearance, CrosshairMode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_CONFIG: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InputDevice {
    #[default]
    Controller,
    Mnk,
    Both,
}

impl InputDevice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Controller => "Controller",
            Self::Mnk => "MNK",
            Self::Both => "Both",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerButton {
    A,
    B,
    X,
    Y,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    Back,
    Start,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LeftStick,
    RightStick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "device", content = "input", rename_all = "snake_case")]
pub enum AdsBinding {
    Keyboard { virtual_key: u32 },
    Mouse(MouseButton),
    Controller(ControllerButton),
}

impl AdsBinding {
    pub fn display_name(self) -> String {
        match self {
            Self::Keyboard { virtual_key } => keyboard_name(virtual_key),
            Self::Mouse(button) => match button {
                MouseButton::Left => "Mouse Left".to_owned(),
                MouseButton::Right => "Mouse Right".to_owned(),
                MouseButton::Middle => "Mouse Middle".to_owned(),
                MouseButton::X1 => "Mouse 4".to_owned(),
                MouseButton::X2 => "Mouse 5".to_owned(),
            },
            Self::Controller(button) => match button {
                ControllerButton::A => "A".to_owned(),
                ControllerButton::B => "B".to_owned(),
                ControllerButton::X => "X".to_owned(),
                ControllerButton::Y => "Y".to_owned(),
                ControllerButton::LeftShoulder => "LB".to_owned(),
                ControllerButton::RightShoulder => "RB".to_owned(),
                ControllerButton::LeftTrigger => "LT".to_owned(),
                ControllerButton::RightTrigger => "RT".to_owned(),
                ControllerButton::Back => "Back".to_owned(),
                ControllerButton::Start => "Start".to_owned(),
                ControllerButton::DpadUp => "D-pad Up".to_owned(),
                ControllerButton::DpadDown => "D-pad Down".to_owned(),
                ControllerButton::DpadLeft => "D-pad Left".to_owned(),
                ControllerButton::DpadRight => "D-pad Right".to_owned(),
                ControllerButton::LeftStick => "Left Stick".to_owned(),
                ControllerButton::RightStick => "Right Stick".to_owned(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub schema_version: u32,
    pub setup_completed: bool,
    pub input_device: InputDevice,
    pub controller_binding: Option<AdsBinding>,
    pub mnk_binding: Option<AdsBinding>,
    pub crosshair_mode: CrosshairMode,
    pub invert_dynamic_visibility: bool,
    pub crosshair_appearance: CrosshairAppearance,
    pub overlay_enabled: bool,
    #[serde(flatten)]
    pub extra_settings: BTreeMap<String, serde_json::Value>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            setup_completed: false,
            input_device: InputDevice::default(),
            controller_binding: None,
            mnk_binding: None,
            crosshair_mode: CrosshairMode::default(),
            invert_dynamic_visibility: false,
            crosshair_appearance: CrosshairAppearance::default(),
            overlay_enabled: false,
            extra_settings: BTreeMap::new(),
        }
    }
}

pub fn config_file_path() -> Result<PathBuf, String> {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .map(|path| path.join("AppData").join("Roaming"))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join(".config"))
        })
        .ok_or_else(|| "no application configuration directory is available".to_owned())?;

    Ok(root.join("Crosshair Studio").join("config.json"))
}

pub fn load_config() -> Result<(PathBuf, AppConfig), String> {
    let path = config_file_path()?;
    match fs::read_to_string(&path) {
        Ok(contents) => {
            let mut config: AppConfig = serde_json::from_str(&contents)
                .map_err(|error| format!("Could not load settings: {error}"))?;
            config.sanitize();
            Ok((path, config))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok((path, AppConfig::default()))
        }
        Err(error) => Err(format!("Could not read settings: {error}")),
    }
}

pub fn save_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    let mut config = config.clone();
    config.sanitize();
    let contents = serde_json::to_string_pretty(&config)
        .map_err(|error| format!("could not encode settings: {error}"))?;
    let parent = path
        .parent()
        .ok_or_else(|| "configuration file has no parent directory".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create configuration directory: {error}"))?;
    let temp_path = path.with_file_name(format!(
        ".config-{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temp_path, contents)
        .map_err(|error| format!("could not write temporary configuration: {error}"))?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("could not replace configuration file: {error}"));
    }
    Ok(())
}

impl AppConfig {
    pub fn sanitize(&mut self) {
        self.schema_version = self.schema_version.max(1);
        self.setup_completed |= self.bindings_valid();
        self.crosshair_appearance.sanitize();
    }

    pub fn bindings_valid(&self) -> bool {
        match self.input_device {
            InputDevice::Controller => self.controller_binding.is_some(),
            InputDevice::Mnk => self.mnk_binding.is_some(),
            InputDevice::Both => self.controller_binding.is_some() && self.mnk_binding.is_some(),
        }
    }
}

fn keyboard_name(virtual_key: u32) -> String {
    match virtual_key {
        0x20 => "Space".to_owned(),
        0x09 => "Tab".to_owned(),
        0x0D => "Enter".to_owned(),
        0x08 => "Backspace".to_owned(),
        0x10 | 0xA0 | 0xA1 => "Shift".to_owned(),
        0x11 | 0xA2 | 0xA3 => "Ctrl".to_owned(),
        0x12 | 0xA4 | 0xA5 => "Alt".to_owned(),
        0x14 => "Caps Lock".to_owned(),
        0x25 => "Left Arrow".to_owned(),
        0x26 => "Up Arrow".to_owned(),
        0x27 => "Right Arrow".to_owned(),
        0x28 => "Down Arrow".to_owned(),
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(virtual_key).unwrap_or('?').to_string(),
        0x60..=0x69 => format!("Numpad {}", virtual_key - 0x60),
        0x70..=0x87 => format!("F{}", virtual_key - 0x6F),
        _ => format!("Key 0x{virtual_key:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_default_editor_fields_without_losing_bindings_or_extra_keys() {
        let mut config: AppConfig = serde_json::from_str(
            r#"{
                "input_device": "both",
                "controller_binding": {
                    "device": "controller",
                    "input": "left_trigger"
                },
                "mnk_binding": {
                    "device": "keyboard",
                    "input": { "virtual_key": 69 }
                },
                "editor_theme": "green"
            }"#,
        )
        .expect("legacy configuration should load");
        config.sanitize();

        assert_eq!(config.input_device, InputDevice::Both);
        assert_eq!(
            config.controller_binding,
            Some(AdsBinding::Controller(ControllerButton::LeftTrigger))
        );
        assert_eq!(
            config.mnk_binding,
            Some(AdsBinding::Keyboard { virtual_key: 69 })
        );
        assert_eq!(config.schema_version, 1);
        assert!(config.setup_completed);
        assert_eq!(config.crosshair_mode, CrosshairMode::Dynamic);
        assert!(!config.invert_dynamic_visibility);
        assert_eq!(config.extra_settings["editor_theme"], "green");

        let saved = serde_json::to_value(config).expect("legacy settings should serialize");
        assert_eq!(saved["schema_version"], 1);
        assert_eq!(saved["setup_completed"], true);
        assert_eq!(saved["editor_theme"], "green");
    }

    #[test]
    fn editor_settings_use_explicit_and_compatible_json_fields() {
        let mut config = AppConfig::default();
        config.controller_binding = Some(AdsBinding::Controller(ControllerButton::LeftTrigger));
        config.mnk_binding = Some(AdsBinding::Keyboard { virtual_key: 69 });
        config.crosshair_mode = CrosshairMode::Static;
        config.invert_dynamic_visibility = true;
        config.crosshair_appearance.size = 18.0;
        config.overlay_enabled = true;
        config.sanitize();

        let value = serde_json::to_value(&config).expect("settings should serialize");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["setup_completed"], true);
        assert_eq!(value["crosshair_mode"], "static");
        assert_eq!(value["invert_dynamic_visibility"], true);
        assert_eq!(value["crosshair_appearance"]["size"], 18.0);
        assert_eq!(value["overlay_enabled"], true);

        let restored: AppConfig =
            serde_json::from_value(value).expect("settings should deserialize");
        assert_eq!(restored.schema_version, 1);
        assert!(restored.setup_completed);
        assert_eq!(restored.crosshair_mode, CrosshairMode::Static);
        assert!(restored.invert_dynamic_visibility);
        assert_eq!(restored.crosshair_appearance.size, 18.0);
        assert!(restored.overlay_enabled);
    }

    #[test]
    fn saved_settings_round_trip_through_file() {
        let path = std::env::temp_dir().join(format!(
            "crosshair-studio-config-test-{}.json",
            std::process::id()
        ));
        let mut config = AppConfig::default();
        config.crosshair_appearance.rotation_degrees = 45.0;

        save_config(&path, &config).expect("settings should save");
        let saved = fs::read_to_string(&path).expect("saved file should be readable");
        let restored: AppConfig = serde_json::from_str(&saved).expect("saved settings should load");
        fs::remove_file(path).expect("test settings file should be removed");

        assert_eq!(restored.crosshair_appearance.rotation_degrees, 45.0);
    }

    #[test]
    fn invalid_appearance_values_are_clamped_before_persistence() {
        let mut config = AppConfig::default();
        config.crosshair_appearance.size = f32::NAN;
        config.crosshair_appearance.opacity = 5.0;
        config.sanitize();

        assert_eq!(config.crosshair_appearance.size, 12.0);
        assert_eq!(config.crosshair_appearance.opacity, 1.0);
    }
}
