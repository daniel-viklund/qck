-- Runs once at startup. Callbacks execute on a dedicated worker, never an input hook.
local mac = qck.platform == "macos"
local window_mod = mac and "ctrl+alt+" or "ctrl+win+"
-- RegisterHotKey cannot distinguish left/right Alt. Avoid Ctrl+Alt / AltGr on Windows.
local app_mod = mac and "ctrl+cmd+" or "ctrl+shift+win+"

qck.bind(window_mod .. "h", function() qck.window.snap("left") end)
qck.bind(window_mod .. "l", function() qck.window.snap("right") end)
qck.bind(window_mod .. "k", function() qck.window.snap("full") end)
qck.bind(window_mod .. "j", function() qck.window.cycle_app() end)

-- App launchers: focus an existing window, restore it if minimized, or launch.
-- Edit this table to add/remove bindings. Mac targets are bundle identifiers;
-- Windows targets are executable names or full paths (use [[C:\path\app.exe]]).
local apps = {
    f = { macos = "com.apple.finder", windows = "explorer.exe" },
    h = { macos = "com.bitwarden.desktop", windows = "Bitwarden.exe" },
    j = { macos = "org.mozilla.firefox", windows = "firefox.exe" },
    k = { macos = "com.github.wez.wezterm", windows = "wezterm-gui.exe" },
    l = { macos = "com.hnc.Discord", windows = "Discord.exe" },
    r = { macos = "com.blackmagic-design.DaVinciResolve", windows = [[C:\Program Files\Blackmagic Design\DaVinci Resolve\Resolve.exe]] },
    s = { macos = "com.spotify.client", windows = "Spotify.exe" },
    -- GitHub Copilot's old comma binding is omitted: no installed app was found.
    -- comma = { macos = "your.app.bundle.id", windows = [[C:\path\app.exe]] },
}

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
