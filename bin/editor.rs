use crosshair_studio::config::{AppConfig, config_file_path, load_config, save_config};
use crosshair_studio::crosshair::{CrosshairMode, RgbColor, crosshair_visible};
use crosshair_studio::overlay::OverlayService;
use crosshair_studio::renderer::CrosshairRenderer;
use iced::widget::{button, canvas, column, container, row, scrollable, slider, space, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};
use std::path::PathBuf;

const COLOR_BACKGROUND: Color = Color::from_rgb8(0x16, 0x16, 0x16);
const COLOR_PANEL: Color = Color::from_rgb8(0x1D, 0x1D, 0x1D);
const COLOR_TEXT: Color = Color::from_rgb8(0xE7, 0xE7, 0xE7);
const COLOR_TEXT_SECONDARY: Color = Color::from_rgb8(0xC1, 0xC1, 0xC1);
const COLOR_ACCENT: Color = Color::from_rgb8(0x3D, 0x6E, 0x4A);
const COLOR_ACCENT_HOVER: Color = Color::from_rgb8(0x3D, 0xA0, 0x57);
const COLOR_BORDER: Color = Color::from_rgb8(0x32, 0x32, 0x32);

pub fn main() -> iced::Result {
    iced::application(Editor::new, update, view)
        .title("Crosshair Studio")
        .theme(|_: &Editor| Theme::Dark)
        .window(iced::window::Settings {
            size: iced::Size::new(940.0, 760.0),
            exit_on_close_request: false,
            ..Default::default()
        })
        .subscription(|_| iced::window::close_requests().map(Message::CloseRequested))
        .run()
}

struct Editor {
    config_path: Option<PathBuf>,
    config: AppConfig,
    config_save_blocked: bool,
    simulated_ads_held: bool,
    status: String,
    overlay_status: String,
    overlay_service: Option<OverlayService>,
}

impl Editor {
    fn new() -> Self {
        let (config_path, config, config_save_blocked, status) = match load_config() {
            Ok((path, config)) => {
                let status = if config.bindings_valid() {
                    "Settings loaded.".to_owned()
                } else {
                    "Required ADS bindings are missing. Use Setup to configure them.".to_owned()
                };
                (Some(path), config, false, status)
            }
            Err(error) => (
                config_file_path().ok(),
                AppConfig::default(),
                true,
                format!("{error}. Existing settings were not changed."),
            ),
        };
        let overlay_was_requested = config.overlay_enabled;
        let mut editor = Self {
            config_path,
            config,
            config_save_blocked,
            simulated_ads_held: false,
            status,
            overlay_status: "Overlay disabled.".to_owned(),
            overlay_service: None,
        };
        if overlay_was_requested {
            editor.start_overlay();
        }
        editor
    }

    fn persist(&mut self) {
        if self.config_save_blocked {
            self.status =
                "Settings were not saved because the existing config could not be loaded."
                    .to_owned();
            return;
        }
        self.config.setup_completed = self.config.bindings_valid();
        self.config.schema_version = 1;
        let Some(path) = &self.config_path else {
            self.status = "Settings cannot be saved: config path unavailable.".to_owned();
            return;
        };
        match save_config(path, &self.config) {
            Ok(()) => self.status = "Settings saved.".to_owned(),
            Err(error) => self.status = format!("Save failed: {error}"),
        }
    }

    fn start_overlay(&mut self) {
        if !self.config.bindings_valid() {
            self.overlay_status =
                "Overlay not started: configure the required ADS bindings in Setup.".to_owned();
            return;
        }
        match OverlayService::start(self.config.clone()) {
            Ok(service) => {
                self.overlay_status = service.status();
                self.overlay_service = Some(service);
            }
            Err(error) => {
                self.overlay_status = format!("Overlay failed: {error}");
            }
        }
    }

    fn sync_overlay(&mut self) {
        if let Some(service) = &self.overlay_service
            && let Err(error) = service.update(self.config.clone())
        {
            self.overlay_status = format!("Overlay update failed: {error}");
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    SetMode(CrosshairMode),
    ToggleInvert,
    ToggleOutline,
    ToggleSimulatedAds,
    ToggleOverlay,
    SetRed(f32),
    SetGreen(f32),
    SetBlue(f32),
    SetSize(f32),
    SetThickness(f32),
    SetGap(f32),
    SetOpacity(f32),
    SetOutlineThickness(f32),
    SetOutlineRed(f32),
    SetOutlineGreen(f32),
    SetOutlineBlue(f32),
    SetXOffset(f32),
    SetYOffset(f32),
    SetRotation(f32),
    Save,
    OpenSetup,
    CloseRequested(iced::window::Id),
}

fn update(state: &mut Editor, message: Message) -> iced::Task<Message> {
    let appearance = &mut state.config.crosshair_appearance;
    let save_after_change = match message {
        Message::SetMode(mode) => {
            state.config.crosshair_mode = mode;
            true
        }
        Message::ToggleInvert => {
            state.config.invert_dynamic_visibility = !state.config.invert_dynamic_visibility;
            true
        }
        Message::ToggleOutline => {
            appearance.outline_enabled = !appearance.outline_enabled;
            true
        }
        Message::ToggleSimulatedAds => {
            state.simulated_ads_held = !state.simulated_ads_held;
            false
        }
        Message::ToggleOverlay => {
            if state.config.overlay_enabled {
                state.config.overlay_enabled = false;
                state.overlay_service = None;
                state.overlay_status = "Overlay disabled.".to_owned();
            } else if !state.config.bindings_valid() {
                state.overlay_status =
                    "Configure the required ADS bindings in Setup before enabling the overlay."
                        .to_owned();
                return iced::Task::none();
            } else {
                state.config.overlay_enabled = true;
                state.start_overlay();
            }
            state.config.sanitize();
            state.persist();
            state.sync_overlay();
            return iced::Task::none();
        }
        Message::SetRed(value) => {
            appearance.color.red = value.round() as u8;
            false
        }
        Message::SetGreen(value) => {
            appearance.color.green = value.round() as u8;
            false
        }
        Message::SetBlue(value) => {
            appearance.color.blue = value.round() as u8;
            false
        }
        Message::SetSize(value) => {
            appearance.size = value;
            false
        }
        Message::SetThickness(value) => {
            appearance.thickness = value;
            false
        }
        Message::SetGap(value) => {
            appearance.center_gap = value;
            false
        }
        Message::SetOpacity(value) => {
            appearance.opacity = value / 100.0;
            false
        }
        Message::SetOutlineThickness(value) => {
            appearance.outline_thickness = value;
            false
        }
        Message::SetOutlineRed(value) => {
            appearance.outline_color.red = value.round() as u8;
            false
        }
        Message::SetOutlineGreen(value) => {
            appearance.outline_color.green = value.round() as u8;
            false
        }
        Message::SetOutlineBlue(value) => {
            appearance.outline_color.blue = value.round() as u8;
            false
        }
        Message::SetXOffset(value) => {
            appearance.x_offset = value;
            false
        }
        Message::SetYOffset(value) => {
            appearance.y_offset = value;
            false
        }
        Message::SetRotation(value) => {
            appearance.rotation_degrees = value;
            false
        }
        Message::Save => {
            state.config.sanitize();
            state.sync_overlay();
            state.persist();
            return iced::Task::none();
        }
        Message::OpenSetup => {
            if let Err(error) = launch_setup() {
                state.status = error;
            }
            return iced::Task::none();
        }
        Message::CloseRequested(id) => {
            if state.overlay_service.is_some() {
                state.status =
                    "Overlay remains active. Use Exit app to close both editor and overlay."
                        .to_owned();
                return iced::Task::none();
            }
            return iced::window::close(id);
        }
    };
    state.config.sanitize();
    state.sync_overlay();
    if save_after_change {
        state.persist();
    }
    iced::Task::none()
}

fn view(state: &Editor) -> Element<'_, Message> {
    let appearance = state.config.crosshair_appearance;
    let visible = crosshair_visible(
        state.config.crosshair_mode,
        state.simulated_ads_held,
        state.config.invert_dynamic_visibility,
    );
    let preview: Element<'_, Message> = canvas::Canvas::new(CrosshairRenderer {
        appearance,
        visible,
    })
    .width(Length::Fill)
    .height(Length::Fixed(330.0))
    .into();

    let preview_panel = card(
        column![
            row![
                text("Live Preview").size(15).color(COLOR_TEXT),
                space::horizontal(),
                action_button(
                    if state.simulated_ads_held {
                        "Simulated ADS Held"
                    } else {
                        "Simulated ADS Released"
                    },
                    Message::ToggleSimulatedAds,
                    state.simulated_ads_held,
                ),
            ]
            .align_y(Alignment::Center),
            text("Simulation only — this does not represent real input.")
                .size(10)
                .color(COLOR_TEXT_SECONDARY),
            container(preview)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgb8(0x12, 0x12, 0x12))),
                    border: Border::default().color(COLOR_BORDER).rounded(6.0),
                    ..Default::default()
                }),
            text(if visible {
                "Crosshair visible"
            } else {
                "Crosshair hidden by Dynamic visibility"
            })
            .size(10)
            .color(COLOR_TEXT_SECONDARY),
        ]
        .spacing(10),
    );

    let appearance_panel = card(
        column![
            text("Crosshair Appearance").size(15).color(COLOR_TEXT),
            color_controls(
                "Color",
                appearance.color,
                Message::SetRed,
                Message::SetGreen,
                Message::SetBlue
            ),
            setting_slider(
                "Size",
                appearance.size,
                1.0..=100.0,
                " px",
                Message::SetSize
            ),
            setting_slider(
                "Thickness",
                appearance.thickness,
                1.0..=20.0,
                " px",
                Message::SetThickness
            ),
            setting_slider(
                "Center gap",
                appearance.center_gap,
                0.0..=50.0,
                " px",
                Message::SetGap
            ),
            setting_slider(
                "Opacity",
                appearance.opacity * 100.0,
                5.0..=100.0,
                "%",
                Message::SetOpacity
            ),
            row![
                text("Outline").size(11).color(COLOR_TEXT_SECONDARY),
                space::horizontal(),
                action_button(
                    if appearance.outline_enabled {
                        "Enabled"
                    } else {
                        "Disabled"
                    },
                    Message::ToggleOutline,
                    appearance.outline_enabled,
                ),
            ]
            .align_y(Alignment::Center),
            setting_slider(
                "Outline thickness",
                appearance.outline_thickness,
                0.0..=10.0,
                " px",
                Message::SetOutlineThickness,
            ),
            color_controls(
                "Outline color",
                appearance.outline_color,
                Message::SetOutlineRed,
                Message::SetOutlineGreen,
                Message::SetOutlineBlue,
            ),
        ]
        .spacing(11),
    );

    let position_panel = card(
        column![
            text("Position & Geometry").size(15).color(COLOR_TEXT),
            setting_slider(
                "X offset",
                appearance.x_offset,
                -250.0..=250.0,
                " px",
                Message::SetXOffset
            ),
            setting_slider(
                "Y offset",
                appearance.y_offset,
                -250.0..=250.0,
                " px",
                Message::SetYOffset
            ),
            setting_slider(
                "Rotation",
                appearance.rotation_degrees,
                -180.0..=180.0,
                "°",
                Message::SetRotation,
            ),
        ]
        .spacing(11),
    );

    let visibility_panel = card(
        column![
            text("Visibility Behavior").size(15).color(COLOR_TEXT),
            row![
                text("Mode").size(11).color(COLOR_TEXT_SECONDARY),
                space::horizontal(),
                action_button(
                    "Static",
                    Message::SetMode(CrosshairMode::Static),
                    state.config.crosshair_mode == CrosshairMode::Static,
                ),
                action_button(
                    "Dynamic",
                    Message::SetMode(CrosshairMode::Dynamic),
                    state.config.crosshair_mode == CrosshairMode::Dynamic,
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            if state.config.crosshair_mode == CrosshairMode::Dynamic {
                action_button(
                    if state.config.invert_dynamic_visibility {
                        "Invert visibility: On"
                    } else {
                        "Invert visibility: Off"
                    },
                    Message::ToggleInvert,
                    state.config.invert_dynamic_visibility,
                )
            } else {
                button(text("Inversion preserved while Static").size(10))
                    .style(button_style(false, false))
                    .on_press(Message::SetMode(CrosshairMode::Static))
                    .into()
            },
            text("Static stays visible. Dynamic follows simulated ADS state here; runtime input is not connected yet.")
                .size(10)
                .color(COLOR_TEXT_SECONDARY),
        ]
        .spacing(10),
    );

    let overlay_panel = card(
        column![
            text("Overlay").size(15).color(COLOR_TEXT),
            text("Native overlay stays separate from this editor preview.")
                .size(10)
                .color(COLOR_TEXT_SECONDARY),
            action_button(
                if state.config.overlay_enabled {
                    "Disable overlay"
                } else {
                    "Enable overlay"
                },
                Message::ToggleOverlay,
                state.config.overlay_enabled,
            ),
            text(
                state
                    .overlay_service
                    .as_ref()
                    .map(OverlayService::status)
                    .unwrap_or_else(|| state.overlay_status.clone())
            )
            .size(10)
            .color(COLOR_TEXT_SECONDARY),
        ]
        .spacing(8),
    );

    let status = row![
        text(&state.status).size(10).color(COLOR_TEXT_SECONDARY),
        space::horizontal(),
        action_button("Open setup", Message::OpenSetup, false),
    ]
    .align_y(Alignment::Center);

    container(
        scrollable(
            column![
                row![
                    column![
                        text("Crosshair Studio").size(23).color(COLOR_TEXT),
                        text("Crosshair editor")
                            .size(12)
                            .color(COLOR_TEXT_SECONDARY),
                    ]
                    .spacing(3),
                    space::horizontal(),
                    status,
                ]
                .align_y(Alignment::Center),
                row![
                    column![preview_panel, visibility_panel, overlay_panel].spacing(12),
                    column![appearance_panel, position_panel].spacing(12),
                ]
                .spacing(12)
                .align_y(Alignment::Start),
            ]
            .spacing(14)
            .padding(16),
        )
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(COLOR_BACKGROUND)),
        text_color: Some(COLOR_TEXT),
        ..Default::default()
    })
    .into()
}

fn color_controls<'a>(
    label: &'static str,
    color: RgbColor,
    on_red: fn(f32) -> Message,
    on_green: fn(f32) -> Message,
    on_blue: fn(f32) -> Message,
) -> Element<'a, Message> {
    column![
        text(format!(
            "{label}  #{:02X}{:02X}{:02X}",
            color.red, color.green, color.blue
        ))
        .size(11)
        .color(COLOR_TEXT_SECONDARY),
        color_slider("R", color.red, on_red),
        color_slider("G", color.green, on_green),
        color_slider("B", color.blue, on_blue),
    ]
    .spacing(4)
    .into()
}

fn color_slider(
    value_label: &'static str,
    value: u8,
    on_change: fn(f32) -> Message,
) -> Element<'static, Message> {
    row![
        text(value_label)
            .size(10)
            .width(Length::Fixed(15.0))
            .color(COLOR_TEXT_SECONDARY),
        slider(0.0..=255.0, f32::from(value), on_change)
            .on_release(Message::Save)
            .step(1.0_f32),
        text(value.to_string())
            .size(10)
            .width(Length::Fixed(28.0))
            .color(COLOR_TEXT_SECONDARY),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn setting_slider(
    label: &'static str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &'static str,
    on_change: fn(f32) -> Message,
) -> Element<'static, Message> {
    row![
        text(label)
            .size(11)
            .width(Length::Fixed(112.0))
            .color(COLOR_TEXT_SECONDARY),
        slider(range, value, on_change)
            .on_release(Message::Save)
            .step(0.1_f32),
        text(format!("{value:.1}{suffix}"))
            .size(10)
            .width(Length::Fixed(48.0))
            .color(COLOR_TEXT),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn action_button(
    label: &'static str,
    message: Message,
    selected: bool,
) -> Element<'static, Message> {
    button(text(label).size(10))
        .padding([6, 10])
        .style(button_style(selected, false))
        .on_press(message)
        .into()
}

fn card<'a>(content: impl Into<Element<'a, Message>>) -> container::Container<'a, Message> {
    container(content)
        .width(Length::Fill)
        .padding(14)
        .style(|_| container::Style {
            background: Some(Background::Color(COLOR_PANEL)),
            border: Border::default().color(COLOR_BORDER).rounded(7.0),
            ..Default::default()
        })
}

fn button_style(
    selected: bool,
    primary: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_, status| {
        let accent = selected || primary;
        let background = if accent && status == button::Status::Hovered {
            COLOR_ACCENT_HOVER
        } else if accent {
            COLOR_ACCENT
        } else {
            COLOR_BACKGROUND
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: COLOR_TEXT,
            border: Border::default().color(COLOR_BORDER).rounded(5.0),
            ..Default::default()
        }
    }
}

fn launch_setup() -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Could not locate application: {error}"))?;
    let directory = current_exe
        .parent()
        .ok_or_else(|| "Application directory is unavailable.".to_owned())?;
    let setup = if cfg!(windows) {
        let named = directory.join("CS setup.exe");
        if named.exists() {
            named
        } else {
            directory.join("CS_setup.exe")
        }
    } else {
        directory.join("CS_setup")
    };
    std::process::Command::new(setup)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not launch setup: {error}"))
}
