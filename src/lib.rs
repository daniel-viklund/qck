pub mod config_path;

use mlua::{Function, HookTriggers, Lua, LuaOptions, RegistryKey, StdLib, VmState};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, CString, c_char},
    rc::Rc,
    sync::{
        OnceLock,
        mpsc::{self, SyncSender},
    },
    time::Instant,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Binding {
    pub id: u32,
    pub key: String,
    pub modifiers: Vec<String>,
}

pub fn parse_binding(combo: &str, platform: &str, id: u32) -> Result<Binding, String> {
    let mut parts: Vec<_> = combo.split('+').map(|s| s.trim().to_lowercase()).collect();
    let key = parts.pop().ok_or("Empty shortcut")?;
    if !((key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric())
        || ["comma", "space", "escape", "return", "tab"].contains(&key.as_str()))
    {
        return Err(format!("Unsupported key: {key}"));
    }
    let mut modifiers = Vec::new();
    for part in parts {
        let normalized = match part.as_str() {
            "ctrl" | "control" => "ctrl",
            "alt" | "option" => "alt",
            "shift" => "shift",
            "cmd" | "command" if platform == "macos" => "cmd",
            "win" if platform == "windows" => "win",
            _ => return Err(format!("Unsupported modifier {part} on {platform}")),
        }
        .to_string();
        if modifiers.contains(&normalized) {
            return Err("Duplicate modifier".into());
        }
        modifiers.push(normalized);
    }
    if modifiers.is_empty() {
        return Err("A global shortcut must have a modifier".into());
    }
    modifiers.sort();
    Ok(Binding { id, key, modifiers })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Context {
    pub pid: u32,
    pub window: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub kind: String,
    pub value: String,
    pub context: Context,
}
struct Event {
    id: u32,
    context: Context,
    at: Instant,
}
pub struct Runtime {
    sender: SyncSender<Event>,
    pub bindings: Vec<Binding>,
}

impl Runtime {
    /// Nonblocking: never execute Lua, enumerate windows, or log on the hotkey thread.
    pub fn dispatch(&self, id: u32, context: Context) -> bool {
        self.sender
            .try_send(Event {
                id,
                context,
                at: Instant::now(),
            })
            .is_ok()
    }
    pub fn start(
        source: String,
        platform: String,
        mut native: impl FnMut(Action) -> Result<(), String> + Send + 'static,
    ) -> Result<Self, String> {
        let (sender, events) = mpsc::sync_channel::<Event>(8);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("qck-actions".into())
            .spawn(move || {
                let context = Rc::new(RefCell::new(Context::default()));
                let current = context.clone();
                let loaded = Config::load(&source, &platform, move |kind, value| {
                    native(Action {
                        kind,
                        value,
                        context: current.borrow().clone(),
                    })
                });
                let config = match loaded {
                    Ok(config) => config,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                if ready_tx.send(Ok(config.bindings.clone())).is_err() {
                    return;
                }
                while let Ok(event) = events.recv() {
                    if event.at.elapsed().as_millis() > 500 {
                        eprintln!("qck: skipped stale shortcut {}", event.id);
                        continue;
                    }
                    *context.borrow_mut() = event.context;
                    let started = Instant::now();
                    if let Err(error) = config.invoke(event.id) {
                        eprintln!("qck: shortcut {} failed: {error}", event.id);
                    }
                    if started.elapsed().as_millis() > 50 {
                        eprintln!(
                            "qck: shortcut {} took {} ms",
                            event.id,
                            started.elapsed().as_millis()
                        );
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let bindings = ready_rx.recv().map_err(|e| e.to_string())??;
        Ok(Self { sender, bindings })
    }
}

struct Config {
    lua: Lua,
    bindings: Vec<Binding>,
    callbacks: Vec<RegistryKey>,
    fuel: Rc<Cell<u32>>,
}
impl Config {
    fn load(
        source: &str,
        platform: &str,
        native: impl FnMut(String, String) -> Result<(), String> + 'static,
    ) -> Result<Self, String> {
        let profile = std::env::var("QCK_PROFILE").unwrap_or_default();
        Self::load_with_profile(source, platform, &profile, native)
    }

    fn load_with_profile(
        source: &str,
        platform: &str,
        profile: &str,
        native: impl FnMut(String, String) -> Result<(), String> + 'static,
    ) -> Result<Self, String> {
        let load = || -> mlua::Result<Self> {
            // No io/os/package libraries: scripts cannot block the worker with shell or file IO.
            let lua = Lua::new_with(
                StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
                LuaOptions::default(),
            )?;
            lua.set_memory_limit(16 * 1024 * 1024)?;
            lua.globals().set("dofile", mlua::Value::Nil)?;
            lua.globals().set("loadfile", mlua::Value::Nil)?;
            let fuel = Rc::new(Cell::new(0u32));
            let counter = fuel.clone();
            lua.set_hook(
                HookTriggers::new().every_nth_instruction(10_000),
                move |_, _| {
                    counter.set(counter.get() + 1);
                    if counter.get() > 100 {
                        return Err(mlua::Error::external("Lua instruction budget exceeded"));
                    }
                    Ok(VmState::Continue)
                },
            )?;
            let entries = Rc::new(RefCell::new(Vec::<(Binding, RegistryKey)>::new()));
            let pending = entries.clone();
            let os = platform.to_owned();
            let configuring = Rc::new(Cell::new(true));
            let in_config = configuring.clone();
            let qck = lua.create_table()?;
            qck.set("platform", platform)?;
            qck.set(
                "profile",
                if profile.is_empty() {
                    "default"
                } else {
                    profile
                },
            )?;
            qck.set(
                "bind",
                lua.create_function(move |lua, (combo, callback): (String, Function)| {
                    if !in_config.get() {
                        return Err(mlua::Error::external(
                            "Bindings may only be declared at startup",
                        ));
                    }
                    let mut entries = pending.borrow_mut();
                    if entries.len() >= 256 {
                        return Err(mlua::Error::external("Too many shortcuts"));
                    }
                    let binding = parse_binding(&combo, &os, entries.len() as u32 + 1)
                        .map_err(mlua::Error::external)?;
                    if entries
                        .iter()
                        .any(|(b, _)| b.key == binding.key && b.modifiers == binding.modifiers)
                    {
                        return Err(mlua::Error::external(format!(
                            "Duplicate shortcut: {combo}"
                        )));
                    }
                    entries.push((binding, lua.create_registry_value(callback)?));
                    Ok(())
                })?,
            )?;
            let native = Rc::new(RefCell::new(native));
            for (table_name, methods) in [
                ("window", vec![("snap", "snap"), ("cycle_app", "cycle")]),
                ("app", vec![("focus_or_launch", "app")]),
            ] {
                let table = lua.create_table()?;
                for (method, kind) in methods {
                    let native = native.clone();
                    let configuring = configuring.clone();
                    table.set(
                        method,
                        lua.create_function(move |_, value: Option<String>| {
                            if configuring.get() {
                                return Err(mlua::Error::external(
                                    "Call actions inside a shortcut callback",
                                ));
                            }
                            let value = value.unwrap_or_default();
                            if kind == "snap"
                                && !["left", "right", "full"].contains(&value.as_str())
                            {
                                return Err(mlua::Error::external(
                                    "snap expects left, right, or full",
                                ));
                            }
                            if kind == "app" && value.is_empty() {
                                return Err(mlua::Error::external(
                                    "App identifier cannot be empty",
                                ));
                            }
                            native.borrow_mut()(kind.into(), value).map_err(mlua::Error::external)
                        })?,
                    )?;
                }
                qck.set(table_name, table)?;
            }
            lua.globals().set("qck", qck)?;
            lua.load(source).set_name("config.lua").exec()?;
            configuring.set(false);
            let (bindings, callbacks) = entries.borrow_mut().drain(..).unzip();
            Ok(Self {
                lua,
                bindings,
                callbacks,
                fuel,
            })
        };
        load().map_err(|e| e.to_string())
    }
    fn invoke(&self, id: u32) -> Result<(), String> {
        self.fuel.set(0);
        let key = self
            .callbacks
            .get(id.wrapping_sub(1) as usize)
            .ok_or("Unknown shortcut")?;
        self.lua
            .registry_value::<Function>(key)
            .and_then(|f| f.call::<()>(()))
            .map_err(|e| e.to_string())
    }
}

pub fn validate(source: &str, platform: &str) -> Result<Vec<Binding>, String> {
    Ok(Config::load(source, platform, |_, _| {
        Err("Validation cannot execute actions".into())
    })?
    .bindings)
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
type NativeCallback = extern "C" fn(*const c_char) -> i32;

/// Returns owned JSON; release with qck_free. Called once by the macOS host.
/// # Safety
/// source must be a valid UTF-8 NUL-terminated string. callback must remain valid until exit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qck_start(source: *const c_char, callback: NativeCallback) -> *mut c_char {
    let result = if source.is_null() {
        Err("Missing config".into())
    } else {
        match unsafe { CStr::from_ptr(source) }.to_str() {
            Err(e) => Err(e.to_string()),
            Ok(source) => Runtime::start(source.into(), "macos".into(), move |action| {
                let json = CString::new(serde_json::to_string(&action).unwrap()).unwrap();
                if callback(json.as_ptr()) == 0 {
                    Ok(())
                } else {
                    Err("Native action failed; see backend log".into())
                }
            })
            .and_then(|runtime| {
                let bindings = runtime.bindings.clone();
                RUNTIME
                    .set(runtime)
                    .map_err(|_| "Already started".to_string())?;
                Ok(bindings)
            }),
        }
    };
    let response = match result {
        Ok(bindings) => serde_json::json!({"bindings": bindings}),
        Err(error) => serde_json::json!({"error": error}),
    };
    CString::new(response.to_string()).unwrap().into_raw()
}
#[unsafe(no_mangle)]
pub extern "C" fn qck_dispatch(id: u32, pid: u32) -> bool {
    RUNTIME
        .get()
        .is_some_and(|r| r.dispatch(id, Context { pid, window: 0 }))
}
/// # Safety
/// ptr must be a pointer returned by qck_start, and may be freed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qck_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_native_action_does_not_block_dispatch() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut first = true;
        let runtime = Runtime::start(
            "qck.bind('ctrl+h', function() qck.window.snap('left') end)".into(),
            "macos".into(),
            move |_| {
                if first {
                    first = false;
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }
                Ok(())
            },
        )
        .unwrap();
        assert!(runtime.dispatch(1, Context::default()));
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        // Worker deliberately blocked: eight calls enqueue; the ninth returns immediately.
        for _ in 0..8 {
            assert!(runtime.dispatch(1, Context::default()));
        }
        assert!(!runtime.dispatch(1, Context::default()));
        release_tx.send(()).unwrap();
    }
    #[test]
    fn duplicate_aliases_are_rejected() {
        assert!(validate("qck.bind('ctrl+alt+h', function() end); qck.bind('option+control+h', function() end)", "macos").unwrap_err().contains("Duplicate"));
    }
    #[test]
    fn config_cannot_run_actions() {
        assert!(validate("qck.window.snap('left')", "macos").is_err());
    }
    #[test]
    fn runaway_script_is_stopped() {
        assert!(
            validate("while true do end", "macos")
                .unwrap_err()
                .contains("budget")
        );
    }
    #[test]
    fn callback_recovers_after_error() {
        let actions = Rc::new(RefCell::new(Vec::new()));
        let recorded = actions.clone();
        let config = Config::load("qck.bind('ctrl+h', function() while true do end end); qck.bind('ctrl+l', function() qck.window.snap('right') end)", "macos", move |kind, value| { recorded.borrow_mut().push((kind,value)); Ok(()) }).unwrap();
        assert!(config.invoke(1).is_err());
        config.invoke(2).unwrap();
        assert_eq!(actions.borrow().len(), 1);
    }
    #[test]
    fn profiles_override_add_and_disable_app_bindings() {
        let source = include_str!("../config/config.lua").replace(
            "local profiles = {",
            r#"local profiles = {
            test_profile = {
                j = { windows = "chrome.exe", macos = "com.google.Chrome" },
                l = false,
                b = { windows = "notepad.exe", macos = "com.apple.TextEdit" },
            },"#,
        );
        for (platform, expected) in [("windows", "chrome.exe"), ("macos", "com.google.Chrome")] {
            let actions = Rc::new(RefCell::new(Vec::new()));
            let recorded = actions.clone();
            let config =
                Config::load_with_profile(&source, platform, "test_profile", move |kind, value| {
                    recorded.borrow_mut().push((kind, value));
                    Ok(())
                })
                .unwrap();
            let baseline =
                Config::load_with_profile(&source, platform, "default", |_, _| Ok(())).unwrap();
            assert_eq!(config.bindings.len(), baseline.bindings.len());
            for id in 5..=config.bindings.len() as u32 {
                config.invoke(id).unwrap();
            }
            let actions = actions.borrow();
            assert!(actions.iter().any(|(_, target)| target == expected));
            assert!(
                !actions
                    .iter()
                    .any(|(_, target)| target.to_lowercase().contains("discord"))
            );
            assert!(
                actions
                    .iter()
                    .any(|(_, target)| target == "notepad.exe" || target == "com.apple.TextEdit")
            );
        }
        assert!(
            Config::load_with_profile(&source, "windows", "typo", |_, _| Ok(()))
                .err()
                .unwrap()
                .contains("Unknown QCK_PROFILE")
        );
    }

    #[test]
    fn example_validates_for_both_platforms() {
        for os in ["macos", "windows"] {
            Config::load_with_profile(include_str!("../config/config.lua"), os, "", |_, _| Ok(()))
                .unwrap();
        }
    }
}
