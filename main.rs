use crosshair_studio::config::{
    AdsBinding, AppConfig, ControllerButton, InputDevice, MouseButton, config_file_path,
    load_config as load_shared_config, save_config as save_shared_config,
};
use crosshair_studio::crosshair::{CrosshairMode, crosshair_visible};
use iced::widget::{button, column, container, row, space, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Subscription, Theme};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const COLOR_BACKGROUND: Color = Color::from_rgb8(0x16, 0x16, 0x16);
const COLOR_PANEL: Color = Color::from_rgb8(0x1D, 0x1D, 0x1D);
const COLOR_TEXT: Color = Color::from_rgb8(0xE7, 0xE7, 0xE7);
const COLOR_TEXT_SECONDARY: Color = Color::from_rgb8(0xC1, 0xC1, 0xC1);
const COLOR_ACCENT: Color = Color::from_rgb8(0x3D, 0x6E, 0x4A);
const COLOR_ACCENT_HOVER: Color = Color::from_rgb8(0x3D, 0xA0, 0x57);
const COLOR_BORDER: Color = Color::from_rgb8(0x32, 0x32, 0x32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BindingDevice {
    Controller,
    Mnk,
}

impl BindingDevice {
    fn instructions(self) -> &'static str {
        match self {
            Self::Controller => "Press a controller button or trigger...",
            Self::Mnk => "Press your ADS input...",
        }
    }
}

#[derive(Debug, Clone)]
struct MonitorInfo {
    description: String,
    refresh_rate: u32,
    fallback: bool,
}

impl MonitorInfo {
    fn display_text(&self) -> String {
        if self.fallback {
            "120 Hz (Fallback)".to_owned()
        } else {
            format!("{} Hz", self.refresh_rate)
        }
    }
}

struct SetupCS {
    config_path: Option<PathBuf>,
    config_save_blocked: bool,
    input_device: InputDevice,
    monitor: MonitorInfo,
    controller_binding: Option<AdsBinding>,
    mnk_binding: Option<AdsBinding>,
    controller_status: Option<String>,
    mnk_status: Option<String>,
    recording: Option<BindingDevice>,
    ads_held: bool,
    crosshair_mode: CrosshairMode,
    invert_dynamic_visibility: bool,
    crosshair_appearance: crosshair_studio::crosshair::CrosshairAppearance,
    overlay_enabled: bool,
    extra_settings: BTreeMap<String, serde_json::Value>,
    footer_status: Option<String>,
}

impl SetupCS {
    fn binding(&self, device: BindingDevice) -> Option<AdsBinding> {
        match device {
            BindingDevice::Controller => self.controller_binding,
            BindingDevice::Mnk => self.mnk_binding,
        }
    }

    fn status(&self, device: BindingDevice) -> Option<&str> {
        match device {
            BindingDevice::Controller => self.controller_status.as_deref(),
            BindingDevice::Mnk => self.mnk_status.as_deref(),
        }
    }

    fn set_status(&mut self, device: BindingDevice, status: String) {
        match device {
            BindingDevice::Controller => self.controller_status = Some(status),
            BindingDevice::Mnk => self.mnk_status = Some(status),
        }
    }

    fn set_binding(&mut self, device: BindingDevice, binding: AdsBinding) {
        match device {
            BindingDevice::Controller => self.controller_binding = Some(binding),
            BindingDevice::Mnk => self.mnk_binding = Some(binding),
        }
    }

    fn required_bindings_configured(&self) -> bool {
        match self.input_device {
            InputDevice::Controller => self.controller_binding.is_some(),
            InputDevice::Mnk => self.mnk_binding.is_some(),
            InputDevice::Both => self.controller_binding.is_some() && self.mnk_binding.is_some(),
        }
    }
}

impl SetupCS {
    fn new() -> Self {
        let (config_path, config, config_save_blocked, footer_status) = load_config();
        Self {
            config_path,
            config_save_blocked,
            input_device: config.input_device,
            monitor: detect_primary_monitor(),
            controller_binding: config.controller_binding,
            mnk_binding: config.mnk_binding,
            controller_status: None,
            mnk_status: None,
            recording: None,
            ads_held: false,
            crosshair_mode: config.crosshair_mode,
            invert_dynamic_visibility: config.invert_dynamic_visibility,
            crosshair_appearance: config.crosshair_appearance,
            overlay_enabled: config.overlay_enabled,
            extra_settings: config.extra_settings,
            footer_status,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    SelectInput(InputDevice),
    RecordBind(BindingDevice),
    BindCaptured(BindingDevice, Result<AdsBinding, String>),
    ToggleAds,
    SetCrosshairMode(CrosshairMode),
    ToggleInvertVisibility,
    RefreshDisplay,
    Continue,
}

#[derive(Clone, Default)]
struct CaptureControl {
    cancelled: Arc<AtomicBool>,
    #[cfg(windows)]
    thread_id: Arc<std::sync::atomic::AtomicU32>,
}

impl CaptureControl {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        #[cfg(windows)]
        windows_input::stop_capture_thread(self);
    }
}

struct CaptureGuard(CaptureControl);

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn main() -> iced::Result {
    iced::application(SetupCS::new, update, view)
        .title("Crosshair Studio")
        .theme(|_: &SetupCS| Theme::Dark)
        .window_size(iced::Size::new(420.0, 480.0))
        .subscription(subscription)
        .run()
}

fn update(state: &mut SetupCS, message: Message) {
    match message {
        Message::SelectInput(input_device) => {
            if let Some(recording) = state.recording.take() {
                state.set_status(recording, "Bind capture cancelled.".to_owned());
            }
            state.input_device = input_device;
            save_config(state);
        }
        Message::RecordBind(device) => {
            state.recording = Some(device);
            state.set_status(device, device.instructions().to_owned());
        }
        Message::BindCaptured(device, result) => {
            if state.recording != Some(device) {
                return;
            }
            state.recording = None;
            match result {
                Ok(binding) => {
                    state.set_status(device, format!("Bound to {}", binding.display_name()));
                    state.set_binding(device, binding);
                    save_config(state);
                }
                Err(error) => state.set_status(device, error),
            }
        }
        Message::ToggleAds => state.ads_held = !state.ads_held,
        Message::SetCrosshairMode(mode) => {
            state.crosshair_mode = mode;
            save_config(state);
        }
        Message::ToggleInvertVisibility => {
            state.invert_dynamic_visibility = !state.invert_dynamic_visibility;
            save_config(state);
        }
        Message::RefreshDisplay => state.monitor = detect_primary_monitor(),
        Message::Continue => {
            if state.required_bindings_configured() {
                save_config(state);
                if state.footer_status.is_none() {
                    state.footer_status = Some("Setup validated. Settings saved.".to_owned());
                }
            } else {
                state.footer_status = Some(missing_binding_message(state.input_device).to_owned());
            }
        }
    }
}

pub(crate) fn load_config() -> (Option<PathBuf>, AppConfig, bool, Option<String>) {
    let path = config_file_path().ok();
    match load_shared_config() {
        Ok((path, config)) => (Some(path), config, false, None),
        Err(error) => (path, AppConfig::default(), true, Some(error)),
    }
}

fn save_config(state: &mut SetupCS) {
    if state.config_save_blocked {
        return;
    }
    let Some(path) = &state.config_path else {
        state.footer_status = Some("Settings cannot be saved: no configuration path.".to_owned());
        return;
    };
    let mut config = AppConfig {
        schema_version: 1,
        setup_completed: state.required_bindings_configured(),
        input_device: state.input_device,
        controller_binding: state.controller_binding,
        mnk_binding: state.mnk_binding,
        crosshair_mode: state.crosshair_mode,
        invert_dynamic_visibility: state.invert_dynamic_visibility,
        crosshair_appearance: state.crosshair_appearance,
        overlay_enabled: state.overlay_enabled,
        extra_settings: state.extra_settings.clone(),
    };
    config.sanitize();

    let result = save_shared_config(path, &config);

    match result {
        Ok(()) => {
            state.footer_status = None;
        }
        Err(error) => {
            state.footer_status = Some(format!("Settings not saved: {error}"));
        }
    }
}

fn missing_binding_message(input_device: InputDevice) -> &'static str {
    match input_device {
        InputDevice::Controller => "Record a controller ADS bind before continuing.",
        InputDevice::Mnk => "Record an MNK ADS bind before continuing.",
        InputDevice::Both => "Record both ADS binds before continuing.",
    }
}

fn subscription(state: &SetupCS) -> Subscription<Message> {
    match state.recording {
        Some(BindingDevice::Controller) => Subscription::run(controller_capture_stream),
        Some(BindingDevice::Mnk) => Subscription::run(mnk_capture_stream),
        None => Subscription::none(),
    }
}

fn controller_capture_stream() -> impl iced::futures::Stream<Item = Message> {
    capture_stream(BindingDevice::Controller)
}

fn mnk_capture_stream() -> impl iced::futures::Stream<Item = Message> {
    capture_stream(BindingDevice::Mnk)
}

fn capture_stream(device: BindingDevice) -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(1, async move |mut output| {
        let (mut sender, mut receiver) = iced::futures::channel::mpsc::channel(1);
        let control = CaptureControl::default();
        let _guard = CaptureGuard(control.clone());

        let mut worker_sender = sender.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("setupcs-bind-capture".to_owned())
            .spawn(move || {
                let result = capture_bind(device, control);
                let _ = worker_sender.try_send(Message::BindCaptured(device, result));
            })
        {
            let _ = sender.try_send(Message::BindCaptured(
                device,
                Err(format!("Could not start input capture: {error}")),
            ));
        }

        use iced::futures::StreamExt;
        if let Some(message) = receiver.next().await {
            use iced::futures::SinkExt;
            let _ = output.send(message).await;
        }
    })
}

fn view(state: &SetupCS) -> Element<'_, Message> {
    let header = row![
        column![
            text("Crosshair Studio").size(19).color(COLOR_TEXT),
            text("Let's configure your setup")
                .size(11)
                .color(COLOR_TEXT_SECONDARY),
        ]
        .spacing(1),
        space::horizontal(),
    ]
    .align_y(Alignment::Start);

    let input_options = row![
        input_button(state, InputDevice::Controller),
        input_button(state, InputDevice::Mnk),
        input_button(state, InputDevice::Both),
    ]
    .spacing(6)
    .width(Length::Fill);

    let input_panel = card(
        column![
            text("What do you play on?").size(12).color(COLOR_TEXT),
            input_options,
        ]
        .spacing(8),
    );

    let mut binding_sections = row![].spacing(6);
    match state.input_device {
        InputDevice::Controller => {
            binding_sections = binding_sections.push(binding_section(
                state,
                BindingDevice::Controller,
                "Controller ADS Bind",
            ));
        }
        InputDevice::Mnk => {
            binding_sections =
                binding_sections.push(binding_section(state, BindingDevice::Mnk, "MNK ADS Bind"));
        }
        InputDevice::Both => {
            binding_sections = binding_sections
                .push(binding_section(
                    state,
                    BindingDevice::Controller,
                    "Controller ADS Bind",
                ))
                .push(binding_section(state, BindingDevice::Mnk, "MNK ADS Bind"));
        }
    }

    let bind_panel = card(binding_sections).width(Length::Fill);

    let monitor_panel = card(
        column![
            text("Display Refresh").size(12).color(COLOR_TEXT),
            text(format!("Display 1 — {}", state.monitor.display_text()))
                .size(11)
                .color(COLOR_TEXT),
            text(&state.monitor.description)
                .size(9)
                .color(COLOR_TEXT_SECONDARY),
            button(text("Re-detect display").size(9))
                .width(Length::Fill)
                .height(Length::Fixed(25.0))
                .padding(4)
                .style(button_style(false, false))
                .on_press(Message::RefreshDisplay),
        ]
        .spacing(5),
    )
    .width(Length::Fill);

    let settings_row = row![bind_panel, monitor_panel]
        .spacing(9)
        .width(Length::Fill);

    let ads_label = if state.ads_held {
        "ADS Held"
    } else {
        "ADS Released"
    };
    let mode_selector = row![
        text("Mode").size(10).color(COLOR_TEXT_SECONDARY),
        space::horizontal(),
        button(text("Static").size(9))
            .padding([4, 10])
            .style(button_style(
                state.crosshair_mode == CrosshairMode::Static,
                false,
            ))
            .on_press(Message::SetCrosshairMode(CrosshairMode::Static)),
        button(text("Dynamic").size(9))
            .padding([4, 10])
            .style(button_style(
                state.crosshair_mode == CrosshairMode::Dynamic,
                false,
            ))
            .on_press(Message::SetCrosshairMode(CrosshairMode::Dynamic)),
    ]
    .spacing(5)
    .align_y(Alignment::Center);

    let invert_control: Element<'_, Message> = if state.crosshair_mode == CrosshairMode::Dynamic {
        row![
            text("Invert Visibility")
                .size(9)
                .color(COLOR_TEXT_SECONDARY),
            space::horizontal(),
            button(
                text(if state.invert_dynamic_visibility {
                    "On"
                } else {
                    "Off"
                })
                .size(9)
            )
            .padding([3, 10])
            .style(button_style(state.invert_dynamic_visibility, false))
            .on_press(Message::ToggleInvertVisibility),
        ]
        .align_y(Alignment::Center)
        .into()
    } else {
        space::vertical().height(Length::Fixed(0.0)).into()
    };

    let crosshair_visible = crosshair_visible(
        state.crosshair_mode,
        state.ads_held,
        state.invert_dynamic_visibility,
    );
    let crosshair = column![
        row![
            space::horizontal().width(Length::Fixed(9.0)),
            crosshair_segment(2.0, 8.0, crosshair_visible),
            space::horizontal().width(Length::Fixed(9.0)),
        ]
        .align_y(Alignment::Center),
        row![
            crosshair_segment(8.0, 2.0, crosshair_visible),
            space::horizontal()
                .width(Length::Fixed(4.0))
                .height(Length::Fixed(4.0)),
            crosshair_segment(8.0, 2.0, crosshair_visible),
        ]
        .align_y(Alignment::Center),
        row![
            space::horizontal().width(Length::Fixed(9.0)),
            crosshair_segment(2.0, 8.0, crosshair_visible),
            space::horizontal().width(Length::Fixed(9.0)),
        ]
        .align_y(Alignment::Center),
    ]
    .align_x(Alignment::Center)
    .width(Length::Fill)
    .height(Length::Fill);
    let preview_surface = container(crosshair)
        .width(Length::Fill)
        .height(Length::Fixed(52.0))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(COLOR_BACKGROUND)),
            border: Border::default().color(COLOR_BORDER).rounded(4.0),
            ..Default::default()
        });

    let preview_panel = card(
        column![
            row![
                text("Live Preview").size(12).color(COLOR_TEXT),
                space::horizontal(),
                button(text(ads_label).size(9))
                    .padding([4, 10])
                    .style(button_style(state.ads_held, false))
                    .on_press(Message::ToggleAds),
            ]
            .align_y(Alignment::Center),
            mode_selector,
            invert_control,
            preview_surface,
            text("Preview state is simulated")
                .size(9)
                .color(COLOR_TEXT_SECONDARY),
        ]
        .spacing(4),
    );

    let footer = row![
        column![
            text(" ").size(8).color(COLOR_TEXT_SECONDARY),
            if let Some(status) = &state.footer_status {
                text(status).size(8).color(COLOR_TEXT_SECONDARY)
            } else {
                text(" ").size(8)
            },
        ]
        .spacing(2),
        space::horizontal(),
        button(
            row![text("Continue").size(11), text("→").size(14)]
                .spacing(10)
                .align_y(Alignment::Center),
        )
        .width(Length::Fixed(104.0))
        .height(Length::Fixed(33.0))
        .padding(6)
        .style(button_style(false, true))
        .on_press(Message::Continue),
    ]
    .align_y(Alignment::Center);

    container(
        column![
            header,
            input_panel,
            settings_row,
            preview_panel,
            space::vertical(),
            footer,
        ]
        .spacing(7)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(COLOR_BACKGROUND)),
        text_color: Some(COLOR_TEXT),
        ..Default::default()
    })
    .into()
}

fn input_button(state: &SetupCS, input: InputDevice) -> Element<'_, Message> {
    button(text(input.label()).size(10))
        .width(Length::Fill)
        .height(Length::Fixed(39.0))
        .padding(5)
        .style(button_style(state.input_device == input, false))
        .on_press(Message::SelectInput(input))
        .into()
}

fn crosshair_segment(width: f32, height: f32, visible: bool) -> Element<'static, Message> {
    container(
        space::horizontal()
            .width(Length::Fixed(width))
            .height(Length::Fixed(height)),
    )
    .style(move |_| container::Style {
        background: visible.then_some(Background::Color(COLOR_ACCENT_HOVER)),
        ..Default::default()
    })
    .into()
}

fn binding_section(
    state: &SetupCS,
    device: BindingDevice,
    heading: &'static str,
) -> Element<'static, Message> {
    let recording = state.recording == Some(device);
    let label = if recording {
        "Listening..."
    } else {
        "Record Bind"
    };
    let description = if recording {
        device.instructions().to_owned()
    } else if let Some(status) = state.status(device) {
        status.to_owned()
    } else if let Some(binding) = state.binding(device) {
        binding.display_name()
    } else {
        "Not configured".to_owned()
    };

    binding_section_with_status(heading, device, label.to_owned(), description, recording)
}

fn binding_section_with_status(
    heading: &'static str,
    device: BindingDevice,
    label: String,
    description: String,
    recording: bool,
) -> Element<'static, Message> {
    let start_button = button(text(label).size(10))
        .width(Length::Fill)
        .height(Length::Fixed(28.0))
        .padding(5)
        .style(button_style(false, false));
    let start_button = if recording {
        start_button
    } else {
        start_button.on_press(Message::RecordBind(device))
    };

    container(
        column![
            row![text(heading).size(10).color(COLOR_TEXT),].align_y(Alignment::Center),
            start_button,
            text(description).size(9).color(if recording {
                COLOR_ACCENT_HOVER
            } else {
                COLOR_TEXT_SECONDARY
            }),
        ]
        .spacing(4),
    )
    .width(Length::Fill)
    .padding(7)
    .style(|_| container::Style {
        background: Some(Background::Color(COLOR_BACKGROUND)),
        border: Border::default().color(COLOR_BORDER).rounded(4.0),
        ..Default::default()
    })
    .into()
}

fn card<'a>(content: impl Into<Element<'a, Message>>) -> container::Container<'a, Message> {
    container(content)
        .width(Length::Fill)
        .padding(8)
        .style(|_| container::Style {
            background: Some(Background::Color(COLOR_PANEL)),
            text_color: Some(COLOR_TEXT),
            border: Border::default().color(COLOR_BORDER).rounded(6.0),
            ..Default::default()
        })
}

fn button_style(
    selected: bool,
    primary: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_, status| {
        let accent = selected || primary;
        let hovered = status == button::Status::Hovered;
        let background = if accent && hovered {
            COLOR_ACCENT_HOVER
        } else if accent {
            COLOR_ACCENT
        } else {
            COLOR_BACKGROUND
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: COLOR_TEXT,
            border: Border::default()
                .color(if selected && hovered {
                    COLOR_ACCENT_HOVER
                } else {
                    COLOR_BORDER
                })
                .rounded(4.0),
            ..Default::default()
        }
    }
}

fn detect_primary_monitor() -> MonitorInfo {
    #[cfg(windows)]
    {
        windows_display::detect_primary_monitor()
    }

    #[cfg(not(windows))]
    {
        MonitorInfo {
            description: "Monitor detection is available on Windows.".to_owned(),
            refresh_rate: 120,
            fallback: true,
        }
    }
}

#[cfg(windows)]
mod windows_display {
    use super::MonitorInfo;
    use std::mem::size_of;
    use std::ptr::null;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateDCW, DISPLAY_DEVICE_PRIMARY_DEVICE, DISPLAY_DEVICEW, DeleteDC, EnumDisplayDevicesW,
        GetDeviceCaps, VREFRESH,
    };

    pub(super) fn detect_primary_monitor() -> MonitorInfo {
        let mut device = DISPLAY_DEVICEW::default();
        let mut display_number = 0;
        let mut found_primary = false;

        loop {
            let mut candidate = DISPLAY_DEVICEW::default();
            candidate.cb = size_of::<DISPLAY_DEVICEW>() as u32;
            let found = unsafe { EnumDisplayDevicesW(null(), display_number, &mut candidate, 0) };

            if found == 0 {
                break;
            }

            if candidate.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0 {
                device = candidate;
                found_primary = true;
                break;
            }

            display_number += 1;
        }

        if !found_primary {
            return fallback("Primary display could not be identified.");
        }

        let driver: Vec<u16> = "DISPLAY\0".encode_utf16().collect();
        let display_dc =
            unsafe { CreateDCW(driver.as_ptr(), device.DeviceName.as_ptr(), null(), null()) };

        if display_dc.is_null() {
            return fallback("Windows could not open the primary display.");
        }

        let refresh_rate = unsafe { GetDeviceCaps(display_dc, VREFRESH as i32) };
        unsafe {
            DeleteDC(display_dc);
        }

        if !(24..=1000).contains(&refresh_rate) {
            return fallback("Windows returned an invalid display refresh rate.");
        }

        let description = nul_terminated(&device.DeviceString)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Primary display".to_owned());

        MonitorInfo {
            description,
            refresh_rate: refresh_rate as u32,
            fallback: false,
        }
    }

    fn fallback(description: &str) -> MonitorInfo {
        MonitorInfo {
            description: description.to_owned(),
            refresh_rate: 120,
            fallback: true,
        }
    }

    fn nul_terminated(value: &[u16]) -> Option<String> {
        let end = value.iter().position(|character| *character == 0)?;
        Some(String::from_utf16_lossy(&value[..end]))
    }
}

#[cfg(windows)]
fn capture_bind(device: BindingDevice, control: CaptureControl) -> Result<AdsBinding, String> {
    windows_input::capture_bind(device, control)
}

#[cfg(not(windows))]
fn capture_bind(_device: BindingDevice, _control: CaptureControl) -> Result<AdsBinding, String> {
    Err("Global input binding is available on Windows only.".to_owned())
}

#[cfg(windows)]
mod windows_input {
    use super::{AdsBinding, BindingDevice, CaptureControl, ControllerButton, MouseButton};
    use std::cell::RefCell;
    use std::ptr::null_mut;
    use std::sync::mpsc;
    use std::time::Duration;
    use std::{mem::zeroed, thread};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LBUTTON, VK_LWIN, VK_MBUTTON, VK_MENU,
        VK_RBUTTON, VK_RWIN, VK_SHIFT, VK_XBUTTON1, VK_XBUTTON2,
    };
    use windows_sys::Win32::UI::Input::XboxController::{
        XINPUT_GAMEPAD, XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_B, XINPUT_GAMEPAD_BACK,
        XINPUT_GAMEPAD_DPAD_DOWN, XINPUT_GAMEPAD_DPAD_LEFT, XINPUT_GAMEPAD_DPAD_RIGHT,
        XINPUT_GAMEPAD_DPAD_UP, XINPUT_GAMEPAD_LEFT_SHOULDER, XINPUT_GAMEPAD_LEFT_THUMB,
        XINPUT_GAMEPAD_RIGHT_SHOULDER, XINPUT_GAMEPAD_RIGHT_THUMB, XINPUT_GAMEPAD_START,
        XINPUT_GAMEPAD_TRIGGER_THRESHOLD, XINPUT_GAMEPAD_X, XINPUT_GAMEPAD_Y, XINPUT_STATE,
        XInputGetState,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED,
        LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostQuitMessage,
        PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN,
        WM_SYSKEYDOWN, WM_XBUTTONDOWN,
    };

    thread_local! {
        static CAPTURE_SENDER: RefCell<Option<mpsc::Sender<Result<AdsBinding, String>>>> =
            const { RefCell::new(None) };
    }

    pub(super) fn capture_bind(
        device: BindingDevice,
        control: CaptureControl,
    ) -> Result<AdsBinding, String> {
        match device {
            BindingDevice::Mnk => capture_keyboard_mouse(control),
            BindingDevice::Controller => capture_controller(control),
        }
    }

    pub(super) fn stop_capture_thread(control: &CaptureControl) {
        let thread_id = control.thread_id.load(std::sync::atomic::Ordering::Acquire);
        if thread_id != 0 {
            unsafe {
                PostThreadMessageW(thread_id, 0x0012, 0, 0);
            }
        }
    }

    fn capture_keyboard_mouse(control: CaptureControl) -> Result<AdsBinding, String> {
        let (sender, receiver) = mpsc::channel();
        CAPTURE_SENDER.with(|slot| *slot.borrow_mut() = Some(sender));

        let current_thread = unsafe { GetCurrentThreadId() };
        control
            .thread_id
            .store(current_thread, std::sync::atomic::Ordering::Release);

        let mut queue_message = unsafe { zeroed::<MSG>() };
        unsafe {
            PeekMessageW(&mut queue_message, null_mut(), 0, 0, PM_NOREMOVE);
        }

        if control.is_cancelled() {
            unsafe {
                PostQuitMessage(0);
            }
        }

        wait_for_start_click_release(&control);
        if control.is_cancelled() {
            CAPTURE_SENDER.with(|slot| *slot.borrow_mut() = None);
            return Err("Bind capture cancelled.".to_owned());
        }

        thread::sleep(Duration::from_millis(80));
        let keyboard_hook =
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), null_mut(), 0) };
        if keyboard_hook.is_null() {
            CAPTURE_SENDER.with(|slot| *slot.borrow_mut() = None);
            return Err("Could not install the keyboard input listener.".to_owned());
        }

        let mouse_hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), null_mut(), 0) };
        if mouse_hook.is_null() {
            unsafe {
                UnhookWindowsHookEx(keyboard_hook);
            }
            CAPTURE_SENDER.with(|slot| *slot.borrow_mut() = None);
            return Err("Could not install the mouse input listener.".to_owned());
        }

        let mut message = MSG::default();
        let mut message_error = false;
        loop {
            if control.is_cancelled() {
                break;
            }
            let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
            if result == -1 {
                message_error = true;
                break;
            }
            if result == 0 {
                break;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        unsafe {
            UnhookWindowsHookEx(keyboard_hook);
            UnhookWindowsHookEx(mouse_hook);
        }
        CAPTURE_SENDER.with(|slot| *slot.borrow_mut() = None);

        match receiver.try_recv() {
            Ok(result) => result,
            Err(_) if message_error => {
                Err("The Windows input listener stopped unexpectedly.".to_owned())
            }
            Err(_) => Err("Bind capture cancelled.".to_owned()),
        }
    }

    fn wait_for_start_click_release(control: &CaptureControl) {
        while !control.is_cancelled() && mouse_button_is_down() {
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn mouse_button_is_down() -> bool {
        [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2]
            .into_iter()
            .any(|key| unsafe { GetAsyncKeyState(i32::from(key)) } < 0)
    }

    unsafe extern "system" fn keyboard_proc(code: i32, wparam: usize, lparam: isize) -> isize {
        if code == HC_ACTION as i32
            && (wparam as u32 == WM_KEYDOWN || wparam as u32 == WM_SYSKEYDOWN)
        {
            let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
            if event.flags & LLKHF_INJECTED == 0 {
                if event.vkCode == 0x1B {
                    finish_capture(Err("Bind capture cancelled.".to_owned()));
                    return 1;
                }
                if !shortcut_modifier_down()
                    && !matches!(event.vkCode, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
                {
                    finish_capture(Ok(AdsBinding::Keyboard {
                        virtual_key: event.vkCode,
                    }));
                    return 1;
                }
            }
        }

        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }

    fn shortcut_modifier_down() -> bool {
        [VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN, VK_SHIFT]
            .into_iter()
            .any(|key| unsafe { GetAsyncKeyState(i32::from(key)) } < 0)
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: usize, lparam: isize) -> isize {
        if code == HC_ACTION as i32 {
            let button = match wparam as u32 {
                WM_LBUTTONDOWN => Some(MouseButton::Left),
                WM_RBUTTONDOWN => Some(MouseButton::Right),
                WM_MBUTTONDOWN => Some(MouseButton::Middle),
                WM_XBUTTONDOWN => {
                    let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
                    match (event.mouseData >> 16) as u16 {
                        1 => Some(MouseButton::X1),
                        2 => Some(MouseButton::X2),
                        _ => None,
                    }
                }
                _ => None,
            };

            if let Some(button) = button {
                let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
                if event.flags & LLMHF_INJECTED == 0 {
                    finish_capture(Ok(AdsBinding::Mouse(button)));
                    return 1;
                }
            }
        }

        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }

    fn finish_capture(result: Result<AdsBinding, String>) {
        CAPTURE_SENDER.with(|slot| {
            if let Some(sender) = slot.borrow().as_ref() {
                let _ = sender.send(result);
            }
        });

        unsafe {
            PostQuitMessage(0);
        }
    }

    fn capture_controller(control: CaptureControl) -> Result<AdsBinding, String> {
        let mut previous = [None; 4];
        let mut connected = false;
        for (index, state) in previous.iter_mut().enumerate() {
            let mut current = XINPUT_STATE::default();
            if unsafe { XInputGetState(index as u32, &mut current) } == 0 {
                *state = Some(current);
                connected = true;
            }
        }

        if !connected {
            return Err(
                "No XInput controller found. Connect the GameSir in XInput mode and try again."
                    .to_owned(),
            );
        }

        let mut escape_was_down = false;
        loop {
            if control.is_cancelled() {
                return Err("Bind capture cancelled.".to_owned());
            }

            let escape_is_down = unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE)) } < 0;
            if escape_is_down && !escape_was_down {
                return Err("Bind capture cancelled.".to_owned());
            }
            escape_was_down = escape_is_down;

            let mut any_connected = false;
            for (index, old_state) in previous.iter_mut().enumerate() {
                let mut current = XINPUT_STATE::default();
                if unsafe { XInputGetState(index as u32, &mut current) } != 0 {
                    *old_state = None;
                    continue;
                }
                any_connected = true;

                if let Some(old) = old_state
                    && let Some(button) = button_transition(old.Gamepad, current.Gamepad)
                {
                    return Ok(AdsBinding::Controller(button));
                }
                *old_state = Some(current);
            }

            if !any_connected {
                return Err("The XInput controller disconnected while recording.".to_owned());
            }

            thread::sleep(Duration::from_millis(10));
        }
    }

    fn button_transition(old: XINPUT_GAMEPAD, current: XINPUT_GAMEPAD) -> Option<ControllerButton> {
        [
            (XINPUT_GAMEPAD_A, ControllerButton::A),
            (XINPUT_GAMEPAD_B, ControllerButton::B),
            (XINPUT_GAMEPAD_X, ControllerButton::X),
            (XINPUT_GAMEPAD_Y, ControllerButton::Y),
            (XINPUT_GAMEPAD_LEFT_SHOULDER, ControllerButton::LeftShoulder),
            (
                XINPUT_GAMEPAD_RIGHT_SHOULDER,
                ControllerButton::RightShoulder,
            ),
            (XINPUT_GAMEPAD_BACK, ControllerButton::Back),
            (XINPUT_GAMEPAD_START, ControllerButton::Start),
            (XINPUT_GAMEPAD_DPAD_UP, ControllerButton::DpadUp),
            (XINPUT_GAMEPAD_DPAD_DOWN, ControllerButton::DpadDown),
            (XINPUT_GAMEPAD_DPAD_LEFT, ControllerButton::DpadLeft),
            (XINPUT_GAMEPAD_DPAD_RIGHT, ControllerButton::DpadRight),
            (XINPUT_GAMEPAD_LEFT_THUMB, ControllerButton::LeftStick),
            (XINPUT_GAMEPAD_RIGHT_THUMB, ControllerButton::RightStick),
        ]
        .into_iter()
        .find_map(|(flag, button)| {
            (old.wButtons & flag == 0 && current.wButtons & flag != 0).then_some(button)
        })
        .or_else(|| {
            (old.bLeftTrigger < XINPUT_GAMEPAD_TRIGGER_THRESHOLD as u8
                && current.bLeftTrigger >= XINPUT_GAMEPAD_TRIGGER_THRESHOLD as u8)
                .then_some(ControllerButton::LeftTrigger)
        })
        .or_else(|| {
            (old.bRightTrigger < XINPUT_GAMEPAD_TRIGGER_THRESHOLD as u8
                && current.bRightTrigger >= XINPUT_GAMEPAD_TRIGGER_THRESHOLD as u8)
                .then_some(ControllerButton::RightTrigger)
        })
    }
}
