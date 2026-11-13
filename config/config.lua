-- Runs once at startup. Callbacks execute on a dedicated worker, never an input hook.
local mac = qck.platform == "macos"
local window_mod = mac and "ctrl+alt+" or "ctrl+win+"
local app_mod = mac and "ctrl+cmd+" or "ctrl+alt+"

qck.bind(window_mod .. "h", function() qck.window.snap("left") end)
qck.bind(window_mod .. "l", function() qck.window.snap("right") end)
qck.bind(window_mod .. "k", function() qck.window.snap("full") end)
qck.bind(window_mod .. "j", function() qck.window.cycle_app() end)

-- App launchers: focus an existing window, restore it if minimized, or launch.
-- Mac targets are bundle identifiers or app names (with or without .app).
-- Windows targets are executable names or
-- full paths (use [[C:\path\app.exe]]). Windows discovers .exe paths at startup.
-- Select a profile per machine with QCK_PROFILE. Unset/default selects mac or
-- windows automatically. Machine profiles override that platform's app bindings.
-- Use false to disable a shortcut, or add any new key in the same format.
local profiles = {
    mac = {
        f = { macos = "com.apple.finder" },
        h = { macos = "com.bitwarden.desktop" },
        j = { macos = "org.mozilla.firefox" },
        k = { macos = "com.github.wez.wezterm" },
        l = { macos = "com.hnc.Discord" },
        r = { macos = "com.blackmagic-design.DaVinciResolve" },
        s = { macos = "com.spotify.client" },
        -- comma = { macos = "your.app.bundle.id" },
    },
    windows = {
        f = { windows = "explorer.exe" },
        h = { windows = "Bitwarden.exe" },
        j = { windows = "chrome.exe" },
        k = { windows = "wezterm-gui.exe" },
        l = { windows = "Discord.exe" },
        s = { windows = "Spotify.exe" },
        -- comma = { windows = [[C:\path\app.exe]] },
    },
    mac_work = {
        f = { macos = "com.apple.finder" },
        h = { macos = "KeepassXC" },
        y = { macos = "com.bitwarden.desktop" },
        k = { macos = "com.github.wez.wezterm" },
        s = { macos = "com.spotify.client" },
        j = { macos = "Microsoft Edge" },
        i = { macos = "Royal TSX" },
        o = { macos = "Microsoft Outlook" },
        d = { macos = "Microsoft To Do"},
        l = {macos = "Microsoft Teams" },
        comma = { macos = "GitHub Copilot" },
    },
}
local base = mac and "mac" or "windows"
local profile = qck.profile or "default"
if profile == "default" then profile = base end
local overrides = assert(profiles[profile], "Unknown QCK_PROFILE: " .. profile)
local apps = {}
for key, target in pairs(profiles[base]) do apps[key] = target end
for key, target in pairs(overrides) do
    if target == false then apps[key] = nil else apps[key] = target end
end

-- Sort bindings so shortcut IDs and diagnostic logs stay stable across starts.
local keys = {}
for key in pairs(apps) do keys[#keys + 1] = key end
table.sort(keys)
for _, key in ipairs(keys) do
    local target = apps[key][qck.platform]
    if target then
        qck.bind(app_mod .. key, function() qck.app.focus_or_launch(target) end)
    end
end
