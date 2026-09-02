use std::cell::RefCell;
use std::ffi::c_void;

use warpui_core::platform::{StatusItem, StatusItemEntry};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    ExtractIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CREATESTRUCTW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyMenu, DestroyWindow, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW, HICON,
    HWND_MESSAGE, IDI_APPLICATION, LoadIconW, MF_SEPARATOR, MF_STRING, PostMessageW,
    RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CONTEXTMENU,
    WM_DESTROY, WM_LBUTTONUP, WM_NCCREATE, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};
use windows::core::{PCWSTR, w};
use winit::event_loop::EventLoopProxy;

use crate::windowing::winit::app::CustomEvent;

const STATUS_ITEM_CALLBACK_MESSAGE: u32 = WM_APP + 1;
const STATUS_ITEM_ID: u32 = 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const WINDOW_CLASS_NAME: PCWSTR = w!("SpiritStatusItem");
const FIRST_MENU_COMMAND_ID: usize = 1;

struct StatusItemState {
    proxy: EventLoopProxy<CustomEvent>,
    status_item: RefCell<StatusItem>,
}

impl StatusItemState {
    fn trigger(&self, action: &'static str, argument: String) {
        let _ = self
            .proxy
            .send_event(CustomEvent::StatusItemActionTriggered(action, argument));
    }
}

pub struct StatusItemHandle {
    hwnd: HWND,
    icon: HICON,
}

impl StatusItemHandle {
    pub fn update(&mut self, status_item: StatusItem) {
        let tooltip = status_item.tooltip.clone();
        // SAFETY: `GWLP_USERDATA` holds the `StatusItemState` the window owns until `WM_DESTROY`.
        unsafe {
            let state = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *const StatusItemState;
            let Some(state) = state.as_ref() else {
                return;
            };
            *state.status_item.borrow_mut() = status_item;
            let mut data = notify_icon_data(self.hwnd);
            data.uFlags = NIF_TIP;
            write_tooltip(&mut data.szTip, &tooltip);
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }
}

impl Drop for StatusItemHandle {
    fn drop(&mut self) {
        let data = notify_icon_data(self.hwnd);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            let _ = DestroyWindow(self.hwnd);
            let _ = DestroyIcon(self.icon);
        }
    }
}

pub fn install(
    status_item: StatusItem,
    app_id: &str,
    proxy: EventLoopProxy<CustomEvent>,
) -> Option<StatusItemHandle> {
    let tooltip = status_item.tooltip.clone();
    let window_name = wide(app_id);
    let state = Box::into_raw(Box::new(StatusItemState {
        proxy,
        status_item: RefCell::new(status_item),
    }));
    // SAFETY: Win32 calls on the event loop thread. The state pointer is handed to the window,
    // which owns it until `WM_DESTROY`.
    unsafe {
        let Ok(module) = GetModuleHandleW(PCWSTR::null()) else {
            drop(Box::from_raw(state));
            return None;
        };
        let instance = HINSTANCE::from(module);
        register_window_class(instance);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            WINDOW_CLASS_NAME,
            PCWSTR::from_raw(window_name.as_ptr()),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            Some(state as *const c_void),
        ) {
            Ok(hwnd) => hwnd,
            Err(err) => {
                log::warn!("Unable to create the status item window: {err}");
                drop(Box::from_raw(state));
                return None;
            }
        };
        let icon = application_icon(instance);
        let mut data = notify_icon_data(hwnd);
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = STATUS_ITEM_CALLBACK_MESSAGE;
        data.hIcon = icon;
        write_tooltip(&mut data.szTip, &tooltip);
        if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            log::warn!("Unable to add the status item to the notification area");
            let _ = DestroyWindow(hwnd);
            let _ = DestroyIcon(icon);
            return None;
        }
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
        Some(StatusItemHandle { hwnd, icon })
    }
}

unsafe fn register_window_class(instance: HINSTANCE) {
    let class = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: instance,
        lpszClassName: WINDOW_CLASS_NAME,
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&class);
    }
}

unsafe fn application_icon(instance: HINSTANCE) -> HICON {
    let exe_path = std::env::current_exe()
        .ok()
        .map(|path| wide(&path.to_string_lossy()));
    let extracted = exe_path.map(|exe_path| unsafe {
        ExtractIconW(Some(instance), PCWSTR::from_raw(exe_path.as_ptr()), 0)
    });
    match extracted {
        Some(icon) if icon.0 as isize > 1 => icon,
        _ => unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() },
    }
}

fn notify_icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: STATUS_ITEM_ID,
        ..Default::default()
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn write_tooltip(destination: &mut [u16; 128], tooltip: &str) {
    let units: Vec<u16> = tooltip.encode_utf16().take(destination.len() - 1).collect();
    destination[..units.len()].copy_from_slice(&units);
    destination[units.len()] = 0;
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: `GWLP_USERDATA` only ever holds the `StatusItemState` pointer stored in
    // `WM_NCCREATE`, and it is cleared before being freed in `WM_DESTROY`.
    unsafe {
        match message {
            WM_NCCREATE => {
                let create = lparam.0 as *const CREATESTRUCTW;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            STATUS_ITEM_CALLBACK_MESSAGE => {
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const StatusItemState;
                if let Some(state) = state.as_ref() {
                    match lparam.0 as u32 & 0xffff {
                        WM_LBUTTONUP | NIN_SELECT | NIN_KEYSELECT => {
                            let primary_action = state.status_item.borrow().primary_action();
                            if let Some((action, argument)) = primary_action {
                                state.trigger(action, argument);
                            }
                        }
                        WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd, state),
                        _ => {}
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                let state = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut StatusItemState;
                if !state.is_null() {
                    drop(Box::from_raw(state));
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

unsafe fn show_menu(hwnd: HWND, state: &StatusItemState) {
    // SAFETY: plain Win32 menu calls on the event loop thread; every label buffer outlives the
    // `AppendMenuW` call that copies it.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let mut actions = Vec::new();
        for entry in &state.status_item.borrow().entries {
            match entry {
                StatusItemEntry::Action {
                    label,
                    action,
                    argument,
                } => {
                    actions.push((*action, argument.clone()));
                    let label = wide(label);
                    let _ = AppendMenuW(
                        menu,
                        MF_STRING,
                        FIRST_MENU_COMMAND_ID + actions.len() - 1,
                        PCWSTR::from_raw(label.as_ptr()),
                    );
                }
                StatusItemEntry::Separator => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
            }
        }
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        let selected = (command.0 as usize)
            .checked_sub(FIRST_MENU_COMMAND_ID)
            .and_then(|index| actions.get(index));
        if let Some((action, argument)) = selected {
            state.trigger(action, argument.clone());
        }
    }
}
