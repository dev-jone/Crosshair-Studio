use crate::config::{AdsBinding, AppConfig, ControllerButton, InputDevice, MouseButton};
use crate::crosshair::{CrosshairMode, crosshair_visible};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicIsize, AtomicU32, Ordering},
    mpsc,
};

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::crosshair::{CrosshairGeometry, Point, Segment};
    use std::ptr::{null, null_mut};
    use std::thread;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetStockObject, NULL_PEN,
        PAINTSTRUCT, Polygon, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow, SelectObject,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
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
        CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GetMessageW, GetSystemMetrics, HWND_TOPMOST, LWA_ALPHA, LWA_COLORKEY, MSG, PeekMessageW,
        PostMessageW, PostQuitMessage, PostThreadMessageW, RegisterClassW, SW_HIDE,
        SW_SHOWNOACTIVATE, SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow,
        TranslateMessage, WM_APP, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_TIMER,
        WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_EX_TRANSPARENT, WS_POPUP,
    };

    const OVERLAY_SIZE: i32 = 900;
    const OVERLAY_MESSAGE: u32 = WM_APP + 23;
    const OVERLAY_TIMER: usize = 1;
    const TIMER_INTERVAL_MS: u32 = 20;
    const COLOR_KEY: u32 = 0x00FF00FF;
    static SHARED_SNAPSHOT: Mutex<Option<Arc<Mutex<Snapshot>>>> = Mutex::new(None);

    #[derive(Clone)]
    struct Snapshot {
        config: AppConfig,
        status: Arc<Mutex<String>>,
    }

    pub struct OverlayService {
        snapshot: Arc<Mutex<Snapshot>>,
        hwnd: Arc<AtomicIsize>,
        thread_id: Arc<AtomicU32>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl OverlayService {
        pub fn start(config: AppConfig) -> Result<Self, String> {
            let status = Arc::new(Mutex::new("Starting overlay...".to_owned()));
            let snapshot = Arc::new(Mutex::new(Snapshot {
                config,
                status: status.clone(),
            }));
            let hwnd = Arc::new(AtomicIsize::new(0));
            let thread_id = Arc::new(AtomicU32::new(0));
            let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
            {
                let mut shared = SHARED_SNAPSHOT
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if shared.is_some() {
                    return Err("Overlay is already running.".to_owned());
                }
                *shared = Some(snapshot.clone());
            }

            let thread_snapshot = snapshot.clone();
            let thread_hwnd = hwnd.clone();
            let thread_id_copy = thread_id.clone();
            let thread = match thread::Builder::new()
                .name("crosshair-overlay".to_owned())
                .spawn(move || {
                    if let Err(error) =
                        overlay_thread(thread_snapshot, thread_hwnd, thread_id_copy, &ready_sender)
                    {
                        set_status(&status, format!("Overlay error: {error}"));
                    }
                }) {
                Ok(thread) => thread,
                Err(error) => {
                    clear_shared();
                    return Err(format!("Could not start overlay thread: {error}"));
                }
            };

            match ready_receiver.recv_timeout(Duration::from_secs(3)) {
                Ok(Ok(())) => {
                    set_status(
                        &snapshot
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .status,
                        "Overlay enabled; Dynamic mode uses configured ADS bindings.".to_owned(),
                    );
                    Ok(Self {
                        snapshot,
                        hwnd,
                        thread_id,
                        thread: Some(thread),
                    })
                }
                Ok(Err(error)) => {
                    let _ = thread.join();
                    clear_shared();
                    Err(error)
                }
                Err(error) => {
                    let id = thread_id.load(Ordering::Acquire);
                    if id != 0 {
                        unsafe {
                            PostThreadMessageW(id, 0x0012, 0, 0);
                        }
                    }
                    let _ = thread.join();
                    clear_shared();
                    Err(format!("Overlay initialization did not finish: {error}"))
                }
            }
        }

        pub fn update(&self, config: AppConfig) -> Result<(), String> {
            self.snapshot
                .lock()
                .map_err(|_| "Overlay state lock was poisoned.".to_owned())?
                .config = config;
            let hwnd = self.hwnd.load(Ordering::Acquire) as HWND;
            if hwnd.is_null() {
                return Err("Overlay window is no longer available.".to_owned());
            }
            let posted = unsafe { PostMessageW(hwnd, OVERLAY_MESSAGE, 0, 0) };
            if posted == 0 {
                return Err("Could not send settings to the overlay window.".to_owned());
            }
            Ok(())
        }

        pub fn status(&self) -> String {
            self.snapshot
                .lock()
                .map(|snapshot| {
                    snapshot
                        .status
                        .lock()
                        .map(|status| status.clone())
                        .unwrap_or_else(|_| "Overlay status unavailable.".to_owned())
                })
                .unwrap_or_else(|_| "Overlay status unavailable.".to_owned())
        }
    }

    impl Drop for OverlayService {
        fn drop(&mut self) {
            let hwnd = self.hwnd.load(Ordering::Acquire) as HWND;
            if !hwnd.is_null() {
                unsafe {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                }
            } else {
                let id = self.thread_id.load(Ordering::Acquire);
                if id != 0 {
                    unsafe {
                        PostThreadMessageW(id, 0x0012, 0, 0);
                    }
                }
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            clear_shared();
        }
    }

    fn overlay_thread(
        snapshot: Arc<Mutex<Snapshot>>,
        hwnd_out: Arc<AtomicIsize>,
        thread_id_out: Arc<AtomicU32>,
        ready: &mpsc::SyncSender<Result<(), String>>,
    ) -> Result<(), String> {
        unsafe {
            if SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_null() {
                let error =
                    "Could not enable per-monitor DPI awareness for the overlay.".to_owned();
                let _ = ready.send(Err(error.clone()));
                return Err(error);
            }
            let mut first_message = MSG::default();
            PeekMessageW(&mut first_message, null_mut(), 0, 0, 0);
            let thread_id = windows_sys::Win32::System::Threading::GetCurrentThreadId();
            thread_id_out.store(thread_id, Ordering::Release);

            let class_name: Vec<u16> = "CrosshairStudioOverlay\0".encode_utf16().collect();
            let instance = GetModuleHandleW(null());
            if instance.is_null() {
                let error = "Could not access the application instance.".to_owned();
                let _ = ready.send(Err(error.clone()));
                return Err(error);
            }
            let class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(overlay_window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: null_mut(),
                hCursor: null_mut(),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: class_name.as_ptr(),
            };
            if RegisterClassW(&class) == 0
                && windows_sys::Win32::Foundation::GetLastError()
                    != windows_sys::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS
            {
                let error = "Could not register the transparent overlay window.".to_owned();
                let _ = ready.send(Err(error.clone()));
                return Err(error);
            }

            let window = CreateWindowExW(
                WS_EX_TOPMOST
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                class_name.as_ptr(),
                WS_POPUP,
                0,
                0,
                OVERLAY_SIZE,
                OVERLAY_SIZE,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if window.is_null() {
                let error = "Windows could not create the transparent overlay.".to_owned();
                let _ = ready.send(Err(error.clone()));
                return Err(error);
            }

            hwnd_out.store(window as isize, Ordering::Release);
            if SetLayeredWindowAttributes(window, COLOR_KEY, 255, LWA_COLORKEY | LWA_ALPHA) == 0 {
                let error =
                    "Windows rejected the layered overlay transparency settings.".to_owned();
                let _ = ready.send(Err(error.clone()));
                DestroyWindow(window);
                return Err(error);
            }

            let config = snapshot
                .lock()
                .map_err(|_| "Overlay state lock was poisoned.".to_owned())?
                .config
                .clone();
            let x = (GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSCREEN)
                - OVERLAY_SIZE)
                / 2;
            let y = (GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CYSCREEN)
                - OVERLAY_SIZE)
                / 2;
            SetWindowPos(
                window,
                HWND_TOPMOST,
                x,
                y,
                OVERLAY_SIZE,
                OVERLAY_SIZE,
                windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            );
            apply_snapshot(window, &config);
            RedrawWindow(window, null(), null_mut(), RDW_INVALIDATE | RDW_UPDATENOW);
            let _ = ready.send(Ok(()));

            let mut message = MSG::default();
            loop {
                let result = GetMessageW(&mut message, null_mut(), 0, 0);
                if result <= 0 {
                    if result == -1 {
                        return Err("Overlay message loop failed.".to_owned());
                    }
                    break;
                }
                if message.message == OVERLAY_MESSAGE {
                    let config = snapshot
                        .lock()
                        .map_err(|_| "Overlay state lock was poisoned.".to_owned())?
                        .config
                        .clone();
                    apply_snapshot(window, &config);
                } else if message.message == WM_TIMER && message.wParam == OVERLAY_TIMER {
                    refresh_ads_state(window, &snapshot);
                } else {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            hwnd_out.store(0, Ordering::Release);
            Ok(())
        }
    }

    fn apply_snapshot(window: HWND, config: &AppConfig) {
        let ads_enabled = config.crosshair_mode == CrosshairMode::Dynamic;
        unsafe {
            if ads_enabled && config.bindings_valid() {
                SetTimer(window, OVERLAY_TIMER, TIMER_INTERVAL_MS, None);
            } else {
                windows_sys::Win32::UI::WindowsAndMessaging::KillTimer(window, OVERLAY_TIMER);
            }
            let alpha = (config.crosshair_appearance.opacity * 255.0).round() as u8;
            SetLayeredWindowAttributes(window, COLOR_KEY, alpha, LWA_COLORKEY | LWA_ALPHA);
            let ads_held = if ads_enabled {
                read_ads_state(config).0
            } else {
                false
            };
            let visible = config.overlay_enabled
                && crosshair_visible(
                    config.crosshair_mode,
                    ads_held,
                    config.invert_dynamic_visibility,
                );
            ShowWindow(window, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
            RedrawWindow(window, null(), null_mut(), RDW_INVALIDATE | RDW_UPDATENOW);
        }
    }

    fn refresh_ads_state(window: HWND, shared: &Arc<Mutex<Snapshot>>) {
        let Ok(snapshot) = shared.lock() else { return };
        let config = &snapshot.config;
        if config.crosshair_mode != CrosshairMode::Dynamic || !config.overlay_enabled {
            return;
        }
        let (ads_held, controller_available) = read_ads_state(config);
        let visible = crosshair_visible(
            config.crosshair_mode,
            ads_held,
            config.invert_dynamic_visibility,
        );
        set_status(
            &snapshot.status,
            if config.input_device != InputDevice::Mnk
                && !controller_available
                && config.controller_binding.is_some()
            {
                "Overlay running; XInput controller not detected.".to_owned()
            } else if config.mnk_binding.is_none() && config.controller_binding.is_none() {
                "Overlay running; no ADS binding is configured.".to_owned()
            } else {
                "Overlay running; ADS state uses configured bindings.".to_owned()
            },
        );
        unsafe {
            ShowWindow(window, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }

    fn read_ads_state(config: &AppConfig) -> (bool, bool) {
        let needs_controller =
            config.input_device != InputDevice::Mnk && config.controller_binding.is_some();
        let mut controller_available = false;
        let mut controller_down = false;
        if needs_controller {
            for index in 0..4 {
                let mut state = XINPUT_STATE::default();
                if unsafe { XInputGetState(index, &mut state) } == 0 {
                    controller_available = true;
                    if config
                        .controller_binding
                        .is_some_and(|binding| binding_pressed(binding, Some(state.Gamepad)))
                    {
                        controller_down = true;
                    }
                }
            }
        }

        let mnk_down = config.input_device != InputDevice::Controller
            && config
                .mnk_binding
                .is_some_and(|binding| binding_pressed(binding, None));
        (
            controller_down || mnk_down,
            controller_available || !needs_controller,
        )
    }

    fn binding_pressed(binding: AdsBinding, controller: Option<XINPUT_GAMEPAD>) -> bool {
        match binding {
            AdsBinding::Keyboard { virtual_key } => unsafe {
                GetAsyncKeyState(virtual_key as i32) < 0
            },
            AdsBinding::Mouse(button) => {
                let key = match button {
                    MouseButton::Left => VK_LBUTTON,
                    MouseButton::Right => VK_RBUTTON,
                    MouseButton::Middle => VK_MBUTTON,
                    MouseButton::X1 => VK_XBUTTON1,
                    MouseButton::X2 => VK_XBUTTON2,
                };
                unsafe { GetAsyncKeyState(i32::from(key)) < 0 }
            }
            AdsBinding::Controller(button) => controller.is_some_and(|state| match button {
                ControllerButton::A => state.wButtons & XINPUT_GAMEPAD_A != 0,
                ControllerButton::B => state.wButtons & XINPUT_GAMEPAD_B != 0,
                ControllerButton::X => state.wButtons & XINPUT_GAMEPAD_X != 0,
                ControllerButton::Y => state.wButtons & XINPUT_GAMEPAD_Y != 0,
                ControllerButton::LeftShoulder => {
                    state.wButtons & XINPUT_GAMEPAD_LEFT_SHOULDER != 0
                }
                ControllerButton::RightShoulder => {
                    state.wButtons & XINPUT_GAMEPAD_RIGHT_SHOULDER != 0
                }
                ControllerButton::LeftTrigger => {
                    state.bLeftTrigger >= XINPUT_GAMEPAD_TRIGGER_THRESHOLD as u8
                }
                ControllerButton::RightTrigger => {
                    state.bRightTrigger >= XINPUT_GAMEPAD_TRIGGER_THRESHOLD as u8
                }
                ControllerButton::Back => state.wButtons & XINPUT_GAMEPAD_BACK != 0,
                ControllerButton::Start => state.wButtons & XINPUT_GAMEPAD_START != 0,
                ControllerButton::DpadUp => state.wButtons & XINPUT_GAMEPAD_DPAD_UP != 0,
                ControllerButton::DpadDown => state.wButtons & XINPUT_GAMEPAD_DPAD_DOWN != 0,
                ControllerButton::DpadLeft => state.wButtons & XINPUT_GAMEPAD_DPAD_LEFT != 0,
                ControllerButton::DpadRight => state.wButtons & XINPUT_GAMEPAD_DPAD_RIGHT != 0,
                ControllerButton::LeftStick => state.wButtons & XINPUT_GAMEPAD_LEFT_THUMB != 0,
                ControllerButton::RightStick => state.wButtons & XINPUT_GAMEPAD_RIGHT_THUMB != 0,
            }),
        }
    }

    unsafe extern "system" fn overlay_window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = unsafe { BeginPaint(window, &mut paint) };
                if !dc.is_null() {
                    unsafe {
                        paint_crosshair(dc);
                    }
                    unsafe {
                        EndPaint(window, &paint);
                    }
                }
                0
            }
            WM_CLOSE => {
                unsafe {
                    DestroyWindow(window);
                }
                0
            }
            WM_DESTROY => {
                unsafe {
                    PostQuitMessage(0);
                }
                0
            }
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }

    unsafe fn paint_crosshair(dc: windows_sys::Win32::Graphics::Gdi::HDC) {
        let mut config = match SHARED_SNAPSHOT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(|shared| shared.lock().ok().map(|snapshot| snapshot.config.clone()))
        {
            Some(config) => config,
            None => return,
        };
        config.sanitize();
        let brush = unsafe { CreateSolidBrush(COLOR_KEY) };
        let rect = RECT {
            left: 0,
            top: 0,
            right: OVERLAY_SIZE,
            bottom: OVERLAY_SIZE,
        };
        unsafe {
            FillRect(dc, &rect, brush);
            DeleteObject(brush);
        }

        let geometry = config.crosshair_appearance.geometry();
        let center = Point {
            x: OVERLAY_SIZE as f32 / 2.0,
            y: OVERLAY_SIZE as f32 / 2.0,
        };
        let null_pen = unsafe { GetStockObject(NULL_PEN) };
        let previous_pen = unsafe { SelectObject(dc, null_pen) };
        if geometry.outline_thickness > 0.0 {
            unsafe {
                draw_arms(dc, geometry, center, true);
            }
        }
        unsafe {
            draw_arms(dc, geometry, center, false);
        }
        unsafe {
            SelectObject(dc, previous_pen);
        }
    }

    unsafe fn draw_arms(
        dc: windows_sys::Win32::Graphics::Gdi::HDC,
        geometry: CrosshairGeometry,
        center: Point,
        outline: bool,
    ) {
        let color = if outline {
            geometry.outline_color
        } else {
            geometry.color
        };
        let brush = unsafe { CreateSolidBrush(colorref(color.red, color.green, color.blue)) };
        let previous_brush = unsafe { SelectObject(dc, brush) };
        let width = geometry.thickness
            + if outline {
                geometry.outline_thickness * 2.0
            } else {
                0.0
            };
        for arm in geometry.arms {
            let points = segment_polygon(arm, center, width);
            unsafe {
                Polygon(dc, points.as_ptr(), points.len() as i32);
            }
        }
        unsafe {
            SelectObject(dc, previous_brush);
            DeleteObject(brush);
        }
    }

    fn colorref(red: u8, green: u8, blue: u8) -> u32 {
        u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
    }

    fn segment_polygon(segment: Segment, center: Point, width: f32) -> [POINT; 4] {
        let start_x = center.x + segment.start.x;
        let start_y = center.y + segment.start.y;
        let end_x = center.x + segment.end.x;
        let end_y = center.y + segment.end.y;
        let dx = end_x - start_x;
        let dy = end_y - start_y;
        let length = dx.hypot(dy).max(f32::EPSILON);
        let px = -dy / length * width / 2.0;
        let py = dx / length * width / 2.0;
        [
            POINT {
                x: (start_x + px).round() as i32,
                y: (start_y + py).round() as i32,
            },
            POINT {
                x: (end_x + px).round() as i32,
                y: (end_y + py).round() as i32,
            },
            POINT {
                x: (end_x - px).round() as i32,
                y: (end_y - py).round() as i32,
            },
            POINT {
                x: (start_x - px).round() as i32,
                y: (start_y - py).round() as i32,
            },
        ]
    }

    fn set_status(status: &Mutex<String>, value: String) {
        if let Ok(mut status) = status.lock() {
            *status = value;
        }
    }

    fn clear_shared() {
        *SHARED_SNAPSHOT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    pub use OverlayService as Service;
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub struct Service;

    impl Service {
        pub fn start(_config: AppConfig) -> Result<Self, String> {
            Err("Native crosshair overlay is available on Windows only.".to_owned())
        }

        pub fn update(&self, _config: AppConfig) -> Result<(), String> {
            Err("Native crosshair overlay is available on Windows only.".to_owned())
        }

        pub fn status(&self) -> String {
            "Native crosshair overlay is available on Windows only.".to_owned()
        }
    }
}

pub use platform::Service as OverlayService;
