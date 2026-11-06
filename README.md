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

On first launch it copies the bundled config to `~/.config/qck/config.lua`,
without overwriting an existing file. **Open config…** opens this file. Quit and
reopen Qck after edits; live reload is not implemented yet. **Open log…** shows
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

The first run creates `%APPDATA%\qck\config.lua`. Pass a different config as the
first argument if desired. This first Windows version runs in a console; errors
and slow actions appear there. Ctrl+C exits and releases the shortcuts. Restart
to load config changes. App targets are executable names or full paths; use a
full path if Windows cannot resolve a name such as `Spotify.exe`.

## Default shortcuts

| Action | macOS | Windows |
| --- | --- | --- |
| Left / right half | Ctrl+Option+H / L | Ctrl+Win+H / L |
| Fill current monitor's work area | Ctrl+Option+K | Ctrl+Win+K |
| Cycle focused process's windows | Ctrl+Option+J | Ctrl+Win+J |
| Focus / launch Finder or Explorer | Ctrl+Cmd+F | Ctrl+Alt+F |
| Focus / launch Bitwarden | Ctrl+Cmd+H | Ctrl+Alt+H |
| Focus / launch Firefox | Ctrl+Cmd+J | Ctrl+Alt+J |
| Focus / launch WezTerm | Ctrl+Cmd+K | Ctrl+Alt+K |
| Focus / launch Discord | Ctrl+Cmd+L | Ctrl+Alt+L |
| Focus / launch DaVinci Resolve | Ctrl+Cmd+R | Ctrl+Alt+R |
| Focus / launch Spotify | Ctrl+Cmd+S | Ctrl+Alt+S |

Edit the `apps` table in your live config to customize launchers. The earlier
GitHub Copilot comma shortcut is left unbound because that app was not found
on this Mac; the table includes a commented example for adding it later.
Existing configs are never overwritten by the app, so changing the repository
example alone does not update your live bindings. If your Windows config came
from an earlier build, change its `app_mod` Windows value from `"ctrl+shift+win+"`
to `"ctrl+alt+"`, then restart Qck. Alternatively, test the supplied config directly:
`target\release\qck.exe config\config.lua`.

Windows app shortcuts use Ctrl+Alt. `RegisterHotKey` does not distinguish
left/right Alt, so these bindings can also match AltGr combinations on some
layouts. OS-reserved or already registered combinations may fail. Startup reports
the failed shortcut and releases partial registrations.
Mac letters refer to physical ANSI key positions (also shared by many ISO
layouts); Windows letters use virtual key codes. Arbitrary layout-aware key
names, side-specific modifiers, and ISO punctuation are not yet implemented.

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

`qck.platform` is `macos` or `windows`. Register bindings only at startup and
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
