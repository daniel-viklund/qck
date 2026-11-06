# Qck

A small desktop shortcut app for macOS and Windows, configured in Lua. The first
version snaps windows, cycles windows of the focused process, and focuses or
launches applications.

Qck registers specific global shortcuts with Carbon on macOS and `RegisterHotKey`
on Windows. It has **no global keyboard hook, event tap, keyboard driver, or key
reinjection**. Ordinary typing does not enter Qck. The hotkey callback sends an
ID and foreground context to a bounded, nonblocking queue. Lua and native window
operations run on a separate worker. This avoids putting automation in the
typing path; it does not promise that OS window operations always succeed.

## Try it on macOS

Requires Rust, Xcode Command Line Tools, and macOS 13 or later. Build for the
current machine's architecture:

```sh
./scripts/build-macos.sh
open dist/Qck.app
```

Quit Keyhac and Hammerspoon before starting Qck to avoid shortcut conflicts.
Qck appears in the menu bar. Choose **Enable Accessibility…**, then enable Qck
in System Settings. Window actions need this permission; keyboard registration
does not. The app does not change other apps' settings or start automatically
at login.

Qck reads `config/config.lua` directly from the repository containing
`dist/Qck.app`. **Open config…** opens that shared file. Quit and reopen Qck
after edits; live reload is not implemented yet. **Open log…** shows
`~/Library/Logs/Qck/qck.log`, including failed actions and actions over 50 ms.

The development build is ad-hoc signed. Rebuilding may require granting
Accessibility again. Set `QCK_SIGN_IDENTITY` to a local signing identity for a
stable signed build. This is not a notarized distribution.

## Try it on Windows

For a prebuilt executable, open the repository's **Actions → Check** workflow,
select the latest successful run, and download **qck-windows-x64** under
Artifacts. Extract it and run `target\release\qck.exe` from PowerShell. Quit
Keyhac before testing. These are unsigned development builds.

To build from source instead:

Requires Rust with the MSVC toolchain and Visual Studio C++ Build Tools. Run on
the Windows host, not inside WSL:

```powershell
cargo build --release --locked
.\target\release\qck.exe
```

Qck reads `config/config.lua` directly from the repository (or extracted artifact)
containing `target\release\qck.exe`. Pass a different config as the first argument
if desired. This first Windows version runs in a console; errors
and slow actions appear there. Ctrl+C exits and releases the shortcuts. Restart
to load config changes. App targets are executable names or full paths; use a
full path if Windows cannot resolve a name such as `Spotify.exe`.

## Finding Windows app targets

Use the executable name, for example `chrome.exe` for Google Chrome. In Task
Manager, right-click an app process and choose **Go to details** to see its `.exe`
name, or **Open file location** to find the full executable path. The friendly
name shown in the Processes tab (such as “Google Chrome”) is not the target.
Chrome's multiple processes do not need separate bindings.

For example, replace the `j` entry in the `windows` profile to use Chrome:

```lua
j = { windows = "chrome.exe" },
```

If focusing works but launching does not, use the actual full path from your
machine, for example `windows = [[C:\Program Files\Google\Chrome\Application\chrome.exe]]`.
Lua's `[[...]]` syntax preserves Windows backslashes.

## Different bindings per machine

Keep all bindings in the repository's `config/config.lua`. Qck reads this file
directly; it does not copy it into a user config directory. Each machine can use
its own checkout of the same repository and select a profile with `QCK_PROFILE`.
Commit and pull config changes to share them between machines, then restart Qck.

The config stores the existing app bindings in `mac` and `windows` profiles,
selected automatically for the current platform. The `mac_work` profile overrides
the Mac defaults; add more named profiles as needed. Edit their tables to replace
or add app entries, or use `false` to disable a default binding:

```lua
mac_work = {
    j = { macos = "com.google.Chrome" },
    l = false, -- No Discord shortcut at work.
},
```

Choose the profile once on each Windows machine in PowerShell:

```powershell
[Environment]::SetEnvironmentVariable("QCK_PROFILE", "windows", "User")
$env:QCK_PROFILE = "windows" # Also select it in this terminal immediately.
.\target\release\qck.exe
```

Use the name of any profile you have defined in the shared config. Restart Qck
after changes; existing terminals retain their old environment, so open a fresh terminal or set
`$env:QCK_PROFILE` before starting it. On macOS, `launchctl setenv QCK_PROFILE mac_work`
selects a profile for subsequent GUI launches in the current login session;
repeat after login to keep selecting that profile.

`qck.profile` exposes `QCK_PROFILE` to Lua, defaulting to `"default"` when unset or
empty; the example maps `default` to `mac` on macOS and `windows` on Windows.
Profile names are case-sensitive. The example rejects unknown profiles
to catch typos. Each override replaces a whole key's app entry; include both
platform targets when sharing a profile across operating systems.

Config lookup on both platforms uses this order:

1. An explicit launch argument: `qck.exe <path>` on Windows or
   `qck --config <path>` on macOS.
2. The `QCK_CONFIG` environment variable, if set and nonempty.
3. The nearest `config/config.lua` found by walking up from the executable's
   directory. This works regardless of your terminal's current directory.

If you move the executable or Mac app outside the repository, set `QCK_CONFIG`
to the full path of the repository config. For example, on Windows:

```powershell
[Environment]::SetEnvironmentVariable("QCK_CONFIG", "D:\repos\qck\config\config.lua", "User")
$env:QCK_CONFIG = "D:\repos\qck\config\config.lua"
```

On macOS, use `launchctl setenv QCK_CONFIG /absolute/path/to/qck/config/config.lua`
for GUI launches in the current login session, or launch with `--config`.
`QCK_CONFIG` selects the file; `QCK_PROFILE` selects bindings inside that file.
An explicit path that cannot be read produces an error instead of falling back.
If no repository config is found, startup reports how to specify its path.

After rebuilding, old `%APPDATA%\qck\config.lua` and `~/.config/qck/config.lua`
files are no longer loaded automatically. Move any custom bindings you want to
keep into the repository config. Old files are left untouched. You can validate
without running actions using `qck.exe check config\config.lua windows` from the
repository root.

## Default shortcuts

| Action | macOS | Windows |
| --- | --- | --- |
| Left / right half | Ctrl+Option+H / L | Ctrl+Win+H / L |
| Fill current monitor's work area | Ctrl+Option+K | Ctrl+Win+K |
| Cycle focused process's windows | Ctrl+Option+J | Ctrl+Win+J |
| Focus / launch Finder or Explorer | Ctrl+Cmd+F | Ctrl+Alt+F |
| Focus / launch Bitwarden | Ctrl+Cmd+H | Ctrl+Alt+H |
| Focus / launch Firefox (Mac) or Chrome (Windows) | Ctrl+Cmd+J | Ctrl+Alt+J |
| Focus / launch WezTerm | Ctrl+Cmd+K | Ctrl+Alt+K |
| Focus / launch Discord | Ctrl+Cmd+L | Ctrl+Alt+L |
| Focus / launch DaVinci Resolve | Ctrl+Cmd+R | — |
| Focus / launch Spotify | Ctrl+Cmd+S | Ctrl+Alt+S |

Edit the `mac` or `windows` profile in your live config to customize launchers.
The earlier GitHub Copilot comma shortcut is left unbound because that app was not found
on this Mac; the table includes a commented example for adding it later.
Repository config edits take effect after restarting Qck. If you explicitly load
an older config, change its `app_mod` Windows value from `"ctrl+shift+win+"`
to `"ctrl+alt+"` to match the current shortcuts.

Windows app shortcuts use Ctrl+Alt. `RegisterHotKey` does not distinguish
left/right Alt, so these bindings can also match AltGr combinations on some
layouts. OS-reserved or already registered combinations may fail. Startup reports
the failed shortcut and releases partial registrations.
Mac letters refer to physical ANSI key positions (also shared by many ISO
layouts); Windows letters use virtual key codes. Arbitrary layout-aware key
names, side-specific modifiers, and ISO punctuation are not yet implemented.

### Windows Live Captions shortcut conflict

Windows can reserve Ctrl+Win+L for Live Captions, preventing Qck from starting.
A workaround verified on the development PC is to append uppercase `L` to the
`DisabledHotkeys` string value under
`HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced`,
then restart Explorer and Qck. Back up the original value and preserve any letters
already present. This also suppresses Explorer's Ctrl+Alt+Shift+Win+L registration;
restarting Explorer briefly interrupts the taskbar and can close folder windows.
Restore the original value (or remove it if it did not exist), then restart
Explorer to undo the change. This is a Windows setting, not something Qck changes
automatically, and behavior may differ between Windows builds.

## Lua API

```lua
local mac = qck.platform == "macos"
qck.bind(mac and "ctrl+cmd+b" or "ctrl+alt+b", function()
    qck.app.focus_or_launch(mac and "org.mozilla.firefox" or "firefox.exe")
end)

qck.bind(mac and "ctrl+alt+h" or "ctrl+win+h", function()
    qck.window.snap("left") -- "left", "right", or "full"
end)

qck.bind(mac and "ctrl+alt+j" or "ctrl+win+j", function()
    qck.window.cycle_app()
end)
```

`qck.platform` is `macos` or `windows`; `qck.profile` is the selected machine
profile (see above). Register bindings only at startup and
call actions inside callbacks. Supported keys: letters, digits, `comma`, `space`,
`escape`, `return`, `tab`. Supported modifiers: `ctrl`, `alt`, `shift`, and
`cmd` (Mac) or `win` (Windows); `control`, `option`, `command` are aliases.

Lua has base, table, string, math, and UTF-8 facilities. There are no `io`, `os`,
or `package` libraries. Scripts have a 16 MiB memory limit and an instruction
budget; a runaway callback reports an error and later shortcuts remain usable.
Native operations can still take time independently of that instruction budget.
At most eight pending shortcuts are retained; excess events are dropped, and
events waiting more than 500 ms are skipped instead of replaying old actions.

## Window behavior and current limits

- Snapping temporarily suspends `AXEnhancedUserInterface` when enabled, then
  restores it even if a frame write fails. This avoids Firefox's animated
  position/size writes interrupting each other and requiring a second shortcut.
  The optional attribute is left alone when VoiceOver or Switch Control is on,
  or when it is unsupported. Failures to change or restore it are logged.
- On macOS, a running app without an accessible standard window receives an
  open/reopen request through Launch Services, as it would when opened from the
  Dock. The same fallback runs if a discovered window can no longer be focused.
  This lets apps such as Spotify recreate a closed window without starting a
  second process. The app itself decides how to respond to reopening. These
  fallback requests and errors are recorded in the log.
- Cycling keeps discovery order using AX element identity / HWND, never titles
  or current z-order. Minimized windows are included when the platform exposes
  them. Missing/nonstandard focus starts at the first eligible window and logs
  the fallback.
- The action captures foreground process identity at keypress. Windows also
  captures the HWND. If foreground context changes before execution, the action
  is skipped. On Mac, the focused window within that process is read at execution.
- macOS Accessibility calls have a 250 ms messaging timeout. Applications can
  reject resizing or expose incomplete Accessibility information. Separate
  Spaces, native fullscreen, and modal dialogs remain app/OS dependent.
- Windows filters owned/tool/cloaked windows. Elevated applications, foreground
  restrictions, exclusive fullscreen games, multi-process apps, and virtual
  desktops can limit actions. There is no focus-stealing workaround or privilege
  elevation. Cycling is per process, not a cross-process app grouping.
- Windows uses asynchronous window positioning to avoid waiting for an app's
  window thread. A successful request is not confirmation that the target app
  accepted the final geometry. Minimum window sizes can prevent exact halves.
- No clipboard history, text expansion, key remapping, startup agent, or updater.

## Development and checks

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo run -- check config/config.lua macos
cargo run -- check config/config.lua windows
./scripts/build-macos.sh
dist/Qck.app/Contents/MacOS/qck --self-test
dist/Qck.app/Contents/MacOS/qck --smoke-test
dist/Qck.app/Contents/MacOS/qck --diagnose
./scripts/test-macos-reopen.sh
./scripts/test-macos-window-adjustment.sh
```

`--smoke-test` registers and releases Ctrl+Cmd+Shift+9 without running actions.
`--diagnose` reads Accessibility status, foreground PID, and display count.
Neither changes Accessibility settings. Tests cover configuration validation,
runaway Lua recovery, dispatch isolation, and native screen geometry. CI builds
the native targets on macOS and Windows; interactive behavior still needs testing
on each OS.

The reopen integration test briefly launches a disposable fixture app without
windows, uses the production launcher to reopen it, verifies that a visible
window appears in the same process, then quits the fixture. It needs a desktop
session but does not change Qck's Accessibility permission or touch Spotify.

`src/lib.rs` owns Lua and the action queue. `native/macos/main.swift` implements
the menu-bar app, Carbon registration, and Accessibility window operations.
`src/windows.rs` implements the Windows message loop and Win32 actions. The Mac
app statically links the Rust core through `native/macos/bridge.h`.
