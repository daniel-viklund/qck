use qck::{Action, Context, Runtime};
use std::{
    collections::HashMap,
    ffi::c_void,
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::null_mut,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::Threading::*,
    UI::{HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn last_error(action: &str) -> String {
    format!("{action}: {}", std::io::Error::last_os_error())
}
struct Registrations(Vec<i32>);
impl Drop for Registrations {
    fn drop(&mut self) {
        for id in &self.0 {
            unsafe {
                UnregisterHotKey(null_mut(), *id);
            }
        }
    }
}

pub fn run(path: Option<&str>) -> Result<(), String> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let config = if let Some(path) = path {
        PathBuf::from(path)
    } else {
        let directory =
            PathBuf::from(std::env::var_os("APPDATA").ok_or("APPDATA is unavailable")?).join("qck");
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let path = directory.join("config.lua");
        if !path.exists() {
            std::fs::write(&path, include_str!("../config/config.lua"))
                .map_err(|e| e.to_string())?;
        }
        path
    };
    let source = std::fs::read_to_string(&config).map_err(|e| e.to_string())?;
    let mut backend = Backend::default();
    let runtime = Runtime::start(source, "windows".into(), move |action| {
        backend.perform(action)
    })?;
    let mut registrations = Registrations(Vec::new());
    for binding in &runtime.bindings {
        let mut mods = MOD_NOREPEAT;
        for modifier in &binding.modifiers {
            mods |= match modifier.as_str() {
                "ctrl" => MOD_CONTROL,
                "alt" => MOD_ALT,
                "shift" => MOD_SHIFT,
                "win" => MOD_WIN,
                _ => return Err("Unknown modifier".into()),
            };
        }
        let key = match binding.key.as_str() {
            "comma" => VK_OEM_COMMA as u32,
            "space" => VK_SPACE as u32,
            "escape" => VK_ESCAPE as u32,
            "return" => VK_RETURN as u32,
            "tab" => VK_TAB as u32,
            other => other.to_ascii_uppercase().as_bytes()[0] as u32,
        };
        if unsafe { RegisterHotKey(null_mut(), binding.id as i32, mods, key) } == 0 {
            return Err(last_error(&format!(
                "Cannot register {}+{}; quit Keyhac or change config",
                binding.modifiers.join("+"),
                binding.key
            )));
        }
        registrations.0.push(binding.id as i32);
    }
    eprintln!(
        "qck: ready with {} shortcuts. Config: {}. Ctrl+C to quit.",
        registrations.0.len(),
        config.display()
    );
    unsafe {
        let mut message: MSG = zeroed();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result == -1 {
                return Err(last_error("GetMessage"));
            }
            if result == 0 {
                break;
            }
            if message.message == WM_HOTKEY {
                let window = GetForegroundWindow();
                let mut pid = 0;
                GetWindowThreadProcessId(window, &mut pid);
                // Only a nonblocking queue write. No scripts or window enumeration here.
                runtime.dispatch(
                    message.wParam as u32,
                    Context {
                        pid,
                        window: window as usize as u64,
                    },
                );
            }
        }
    }
    Ok(())
}

unsafe extern "system" fn collect(window: HWND, parameter: LPARAM) -> i32 {
    unsafe {
        if IsWindowVisible(window) == 0
            || !GetWindow(window, GW_OWNER).is_null()
            || GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_TOOLWINDOW as isize != 0
        {
            return 1;
        }
        let mut cloaked: u32 = 0;
        DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut _ as *mut c_void,
            size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return 1;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        let windows = &mut *(parameter as *mut Vec<(usize, u32)>);
        windows.push((window as usize, pid));
    }
    1
}
fn windows() -> Result<Vec<(usize, u32)>, String> {
    let mut result = Vec::new();
    if unsafe { EnumWindows(Some(collect), &mut result as *mut _ as LPARAM) } == 0 {
        return Err(last_error("EnumWindows"));
    }
    Ok(result)
}
fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32768];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(handle);
        if ok == 0 {
            None
        } else {
            Some(String::from_utf16_lossy(&buffer[..length as usize]))
        }
    }
}
fn activate(window: HWND) -> Result<(), String> {
    unsafe {
        if IsIconic(window) != 0 {
            ShowWindowAsync(window, SW_RESTORE);
        }
        if SetForegroundWindow(window) == 0 {
            return Err(
                "Windows refused window activation (foreground restrictions or elevated target)"
                    .into(),
            );
        }
    }
    Ok(())
}
#[derive(Default)]
struct Backend {
    orders: HashMap<u32, Vec<usize>>,
}
impl Backend {
    fn perform(&mut self, action: Action) -> Result<(), String> {
        if action.kind == "app" {
            let desired = action.value.to_lowercase();
            let full_path = desired.contains('\\') || desired.contains('/');
            for (window, pid) in windows()? {
                if let Some(path) = process_path(pid) {
                    let name = if full_path {
                        path.as_str()
                    } else {
                        path.rsplit('\\').next().unwrap_or(&path)
                    };
                    if name.eq_ignore_ascii_case(&desired) {
                        return activate(window as HWND);
                    }
                }
            }
            let target = wide(&action.value);
            let operation = wide("open");
            let result = unsafe {
                ShellExecuteW(
                    null_mut(),
                    operation.as_ptr(),
                    target.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                )
            } as isize;
            return if result > 32 {
                Ok(())
            } else {
                Err(format!(
                    "Cannot launch {} (ShellExecute {result}); use its full executable path",
                    action.value
                ))
            };
        }
        let window = action.context.window as usize as HWND;
        unsafe {
            if window.is_null() || IsWindow(window) == 0 {
                return Err("The original focused window no longer exists".into());
            }
            let mut pid = 0;
            GetWindowThreadProcessId(window, &mut pid);
            if pid != action.context.pid || GetForegroundWindow() != window {
                return Err("Focus changed before action; skipped".into());
            }
        }
        match action.kind.as_str() {
            "snap" => snap(window, &action.value),
            "cycle" => {
                let pid = action.context.pid;
                let live: Vec<_> = windows()?
                    .into_iter()
                    .filter(|(_, p)| *p == pid)
                    .map(|(w, _)| w)
                    .collect();
                if live.is_empty() {
                    return Err("No standard windows for the focused application".into());
                }
                if self.orders.len() > 64 {
                    self.orders.clear();
                }
                let order = self.orders.entry(pid).or_default();
                order.retain(|w| live.contains(w));
                for w in live {
                    if !order.contains(&w) {
                        order.push(w);
                    }
                }
                let index = order.iter().position(|w| *w == window as usize);
                if index.is_none() {
                    eprintln!("qck: focused window absent from cycle; selecting first window");
                }
                activate(order[index.map(|i| (i + 1) % order.len()).unwrap_or(0)] as HWND)
            }
            _ => Err("Unknown action".into()),
        }
    }
}
fn snap(window: HWND, direction: &str) -> Result<(), String> {
    unsafe {
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return Err(last_error("Monitor work area"));
        }
        let area = info.rcWork;
        let width = area.right - area.left;
        let (x, w) = match direction {
            "left" => (area.left, width / 2),
            "right" => (area.left + width / 2, width - width / 2),
            _ => (area.left, width),
        };
        // Visible DWM frame excludes the resize borders. Preserve those outside the work area.
        let mut rect: RECT = zeroed();
        let mut frame: RECT = zeroed();
        let mut margins = (0, 0, 0, 0);
        if IsZoomed(window) == 0
            && GetWindowRect(window, &mut rect) != 0
            && DwmGetWindowAttribute(
                window,
                DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                &mut frame as *mut _ as *mut c_void,
                size_of::<RECT>() as u32,
            ) == 0
        {
            margins = (
                frame.left - rect.left,
                frame.top - rect.top,
                rect.right - frame.right,
                rect.bottom - frame.bottom,
            );
        }
        if IsZoomed(window) != 0 || IsIconic(window) != 0 {
            ShowWindowAsync(window, SW_RESTORE);
        }
        if SetWindowPos(
            window,
            null_mut(),
            x - margins.0,
            area.top - margins.1,
            w + margins.0 + margins.2,
            area.bottom - area.top + margins.1 + margins.3,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        ) == 0
        {
            return Err(last_error("SetWindowPos"));
        }
    }
    Ok(())
}
