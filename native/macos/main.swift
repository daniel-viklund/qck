import AppKit
import ApplicationServices
import Carbon

struct Binding: Decodable { let id: UInt32; let key: String; let modifiers: [String] }
struct Startup: Decodable { let bindings: [Binding]?; let error: String? }
struct Context: Decodable { let pid: UInt32; let window: UInt64 }
struct Action: Decodable { let kind: String; let value: String; let context: Context }
struct Failure: Error, CustomStringConvertible { let description: String; init(_ text: String) { description = text } }
func log(_ text: String) { fputs("qck: \(text)\n", stderr); fflush(stderr) }

// All AX messaging and window-order state live on the Rust action worker.
var windowOrders: [pid_t: [AXUIElement]] = [:]
func attribute(_ element: AXUIElement, _ name: String) throws -> CFTypeRef {
    var value: CFTypeRef?
    let status = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    guard status == .success, let value else { throw Failure("Read \(name): AX error \(status.rawValue)") }
    return value
}
func set(_ element: AXUIElement, _ name: String, _ value: CFTypeRef) throws {
    let status = AXUIElementSetAttributeValue(element, name as CFString, value)
    guard status == .success else { throw Failure("Set \(name): AX error \(status.rawValue)") }
}
func focused(_ app: AXUIElement) throws -> AXUIElement {
    let value = try attribute(app, kAXFocusedWindowAttribute)
    guard CFGetTypeID(value) == AXUIElementGetTypeID() else { throw Failure("Focused element is not a window") }
    return unsafeBitCast(value, to: AXUIElement.self)
}
func windowRect(_ window: AXUIElement) throws -> CGRect {
    let p = try attribute(window, kAXPositionAttribute), s = try attribute(window, kAXSizeAttribute)
    guard CFGetTypeID(p) == AXValueGetTypeID(), CFGetTypeID(s) == AXValueGetTypeID() else { throw Failure("Window geometry is unavailable") }
    var point = CGPoint.zero, size = CGSize.zero
    guard AXValueGetValue(unsafeBitCast(p, to: AXValue.self), .cgPoint, &point), AXValueGetValue(unsafeBitCast(s, to: AXValue.self), .cgSize, &size) else { throw Failure("Invalid window geometry") }
    return CGRect(origin: point, size: size)
}
func snapped(_ area: CGRect, _ direction: String) -> CGRect {
    switch direction {
    case "left": return CGRect(x: area.minX, y: area.minY, width: floor(area.width / 2), height: area.height)
    case "right": let half = floor(area.width / 2); return CGRect(x: area.minX + half, y: area.minY, width: area.width - half, height: area.height)
    default: return area
    }
}
func focus(_ window: AXUIElement, app: AXUIElement, pid: pid_t) throws {
    if (try? attribute(window, kAXMinimizedAttribute)) as? Bool == true { try set(window, kAXMinimizedAttribute, kCFBooleanFalse) }
    let activated = DispatchQueue.main.sync { NSRunningApplication(processIdentifier: pid)?.activate(options: []) ?? false }
    guard activated else { throw Failure("macOS refused to activate process \(pid)") }
    let status = AXUIElementPerformAction(window, kAXRaiseAction as CFString)
    guard status == .success else { throw Failure("Raise window: AX error \(status.rawValue)") }
    // Some apps make AXFocusedWindow read-only; raising is the primary operation.
    _ = AXUIElementSetAttributeValue(app, kAXFocusedWindowAttribute as CFString, window)
}
func perform(_ action: Action) throws {
    if action.kind == "app" {
        let identifier = action.value
        let running = DispatchQueue.main.sync { NSRunningApplication.runningApplications(withBundleIdentifier: identifier).first }
        if let running {
            let app = AXUIElementCreateApplication(running.processIdentifier)
            AXUIElementSetMessagingTimeout(app, 0.25)
            if AXIsProcessTrusted(), let windows = try? attribute(app, kAXWindowsAttribute) as? [AXUIElement],
               let target = windows.first(where: { ((try? attribute($0, kAXSubroleAttribute)) as? String) == kAXStandardWindowSubrole }) {
                do {
                    try focus(target, app: app, pid: running.processIdentifier)
                } catch {
                    // The window may have disappeared or become inaccessible since discovery.
                    openOrReopenApplication(identifier: identifier, reason: "window focus failed: \(error)")
                }
            } else {
                openOrReopenApplication(identifier: identifier, reason: "running without an accessible standard window")
            }
        } else {
            openOrReopenApplication(identifier: identifier, reason: "not running")
        }
        return
    }
    guard AXIsProcessTrusted() else { throw Failure("Enable Qck in System Settings → Privacy & Security → Accessibility") }
    let pid = pid_t(action.context.pid)
    guard pid > 0 else { throw Failure("No focused application at shortcut press") }
    let stillFocused = DispatchQueue.main.sync { NSWorkspace.shared.frontmostApplication?.processIdentifier == pid }
    guard stillFocused else { throw Failure("Focus changed before action; skipped") }
    let app = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(app, 0.25)
    if action.kind == "snap" {
        let window = try focused(app)
        let rect = try windowRect(window)
        let screens: [CGRect] = DispatchQueue.main.sync {
            let top = NSScreen.screens.first?.frame.maxY ?? 0
            return NSScreen.screens.map { s in let f = s.visibleFrame; return CGRect(x: f.minX, y: top - f.maxY, width: f.width, height: f.height) }
        }
        guard let screen = screens.max(by: { a, b in let x = rect.intersection(a), y = rect.intersection(b); return x.width * x.height < y.width * y.height }) else { throw Failure("No display work area") }
        let destination = snapped(screen, action.value)
        let assistiveTechnologyEnabled = DispatchQueue.main.sync {
            NSWorkspace.shared.isVoiceOverEnabled || NSWorkspace.shared.isSwitchControlEnabled
        }
        try withImmediateWindowAdjustment(
            assistiveTechnologyEnabled: assistiveTechnologyEnabled,
            readEnhancedUI: { (try? attribute(app, "AXEnhancedUserInterface")) as? Bool },
            writeEnhancedUI: { enabled in
                AXUIElementSetAttributeValue(app, "AXEnhancedUserInterface" as CFString,
                                             enabled ? kCFBooleanTrue : kCFBooleanFalse) == .success
            },
            report: log
        ) {
            var point = destination.origin, size = destination.size
            try set(window, kAXPositionAttribute, AXValueCreate(.cgPoint, &point)!)
            try set(window, kAXSizeAttribute, AXValueCreate(.cgSize, &size)!)
            try set(window, kAXPositionAttribute, AXValueCreate(.cgPoint, &point)!)
        }
    } else if action.kind == "cycle" {
        guard let all = try attribute(app, kAXWindowsAttribute) as? [AXUIElement] else { throw Failure("Application did not provide a window list") }
        let windows = all.filter { ((try? attribute($0, kAXSubroleAttribute)) as? String) == kAXStandardWindowSubrole }
        guard !windows.isEmpty else { throw Failure("No standard windows to cycle for process \(pid)") }
        // Keep AX element identity; titles and z-order are not stable identities.
        var order = (windowOrders[pid] ?? []).filter { old in windows.contains { CFEqual(old, $0) } }
        for window in windows where !order.contains(where: { CFEqual($0, window) }) { order.append(window) }
        // Bound retained AX references even as applications come and go.
        if windowOrders.count > 64 { windowOrders.removeAll() }
        windowOrders[pid] = order
        let current = try? focused(app)
        let index = current.flatMap { current in order.firstIndex { CFEqual($0, current) } }
        // Missing/nonstandard focus starts at the first window, rather than silently returning.
        if index == nil { log("Focused window absent from cycle; choosing first standard window") }
        try focus(order[index.map { ($0 + 1) % order.count } ?? 0], app: app, pid: pid)
    } else { throw Failure("Unknown action: \(action.kind)") }
}
func nativeAction(_ json: UnsafePointer<CChar>?) -> Int32 {
    guard let json else { return 1 }
    do { let action = try JSONDecoder().decode(Action.self, from: Data(String(cString: json).utf8)); try perform(action); return 0 }
    catch { log(String(describing: error)); return 1 }
}

let keyCodes: [String: UInt32] = ["a":0,"s":1,"d":2,"f":3,"h":4,"g":5,"z":6,"x":7,"c":8,"v":9,"b":11,"q":12,"w":13,"e":14,"r":15,"y":16,"t":17,"1":18,"2":19,"3":20,"4":21,"6":22,"5":23,"9":25,"7":26,"8":28,"0":29,"o":31,"u":32,"i":34,"p":35,"return":36,"l":37,"j":38,"k":40,"n":45,"m":46,"comma":43,"tab":48,"space":49,"escape":53]
var hotkeys: [EventHotKeyRef] = []
var eventHandler: EventHandlerRef?
func register(_ bindings: [Binding]) throws {
    var eventType = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
    let installed = InstallEventHandler(GetApplicationEventTarget(), { _, event, _ in
        var id = EventHotKeyID()
        guard let event, GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil, MemoryLayout<EventHotKeyID>.size, nil, &id) == noErr else { return OSStatus(eventNotHandledErr) }
        let pid = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0
        _ = qck_dispatch(id.id, UInt32(max(0, pid)))
        return noErr
    }, 1, &eventType, nil, &eventHandler)
    guard installed == noErr else { throw Failure("Install hotkey handler: \(installed)") }
    for binding in bindings {
        guard let code = keyCodes[binding.key] else { throw Failure("Unsupported key: \(binding.key)") }
        let mods = binding.modifiers.reduce(UInt32(0)) { value, mod in value | ["ctrl": UInt32(controlKey), "alt": UInt32(optionKey), "cmd": UInt32(cmdKey), "shift": UInt32(shiftKey)][mod, default: 0] }
        var ref: EventHotKeyRef?
        let status = RegisterEventHotKey(code, mods, EventHotKeyID(signature: 0x51434B31, id: binding.id), GetApplicationEventTarget(), 0, &ref)
        guard status == noErr, let ref else {
            for registered in hotkeys { UnregisterEventHotKey(registered) }; hotkeys.removeAll()
            throw Failure("Cannot register \((binding.modifiers + [binding.key]).joined(separator: "+")) (\(status)). Quit Keyhac/Hammerspoon or change the binding.")
        }
        hotkeys.append(ref)
    }
}

class AppDelegate: NSObject, NSApplicationDelegate {
    var item: NSStatusItem?
    let path: String
    init(path: String) { self.path = path }
    func applicationDidFinishLaunching(_ notification: Notification) {
        item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item?.button?.title = "qck"
        let menu = NSMenu()
        for (title, selector) in [("Open config…", #selector(openConfig)), ("Enable Accessibility…", #selector(accessibility)), ("Open log…", #selector(openLog)), ("Quit Qck", #selector(quit))] {
            let entry = NSMenuItem(title: title, action: selector, keyEquivalent: ""); entry.target = self; menu.addItem(entry)
        }
        item?.menu = menu
        do {
            let source = try String(contentsOfFile: path, encoding: .utf8)
            guard let response = source.withCString({ qck_start($0, nativeAction) }) else { throw Failure("Core failed to start") }
            defer { qck_free(response) }
            let startup = try JSONDecoder().decode(Startup.self, from: Data(String(cString: response).utf8))
            if let error = startup.error { throw Failure(error) }
            try register(startup.bindings ?? [])
            log("Ready: \(hotkeys.count) shortcuts; Accessibility=\(AXIsProcessTrusted())")
            if !AXIsProcessTrusted() { item?.button?.title = "qck !" }
        } catch {
            log(String(describing: error)); item?.button?.title = "qck !"
            let alert = NSAlert(); alert.messageText = "Qck could not start"; alert.informativeText = String(describing: error); alert.runModal()
        }
    }
    @objc func openConfig() { NSWorkspace.shared.open(URL(fileURLWithPath: path)) }
    @objc func openLog() { NSWorkspace.shared.open(URL(fileURLWithPath: logPath)) }
    @objc func accessibility() {
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        _ = AXIsProcessTrustedWithOptions(options)
        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!)
    }
    @objc func quit() { NSApplication.shared.terminate(nil) }
    func applicationWillTerminate(_ notification: Notification) { for ref in hotkeys { UnregisterEventHotKey(ref) } }
}

let args = CommandLine.arguments
if args.contains("--self-test") {
    let area = CGRect(x: -1440, y: 25, width: 1441, height: 875)
    let left = snapped(area, "left"), right = snapped(area, "right")
    precondition(left.maxX == right.minX && right.maxX == area.maxX && left.minX == area.minX)
    print("Native geometry tests passed"); exit(0)
}
if args.contains("--diagnose") {
    print("Accessibility: \(AXIsProcessTrusted())")
    print("Frontmost PID: \(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0)")
    print("Displays: \(NSScreen.screens.count)")
    exit(0)
}
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
if args.contains("--smoke-test") {
    do {
        try register([Binding(id: 1, key: "9", modifiers: ["ctrl", "cmd", "shift"])])
        for ref in hotkeys { UnregisterEventHotKey(ref) }
        print("Native hotkey registration and cleanup passed"); exit(0)
    } catch { log(String(describing: error)); exit(1) }
}
let configDir = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".config/qck")
try FileManager.default.createDirectory(at: configDir, withIntermediateDirectories: true)
let defaultConfig = configDir.appendingPathComponent("config.lua")
if !FileManager.default.fileExists(atPath: defaultConfig.path), let bundled = Bundle.main.url(forResource: "config", withExtension: "lua") { try FileManager.default.copyItem(at: bundled, to: defaultConfig) }
let logDirectory = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Logs/Qck")
try FileManager.default.createDirectory(at: logDirectory, withIntermediateDirectories: true)
let logPath = logDirectory.appendingPathComponent("qck.log").path
// Keep the local diagnostic log bounded between launches.
if let size = (try? FileManager.default.attributesOfItem(atPath: logPath)[.size]) as? NSNumber, size.intValue > 2_000_000 { try? FileManager.default.removeItem(atPath: logPath) }
freopen(logPath, "a", stderr)
if NSRunningApplication.runningApplications(withBundleIdentifier: "dev.qck.app").count > 1 { log("Already running"); exit(1) }
let delegate = AppDelegate(path: args.count > 2 && args[1] == "--config" ? args[2] : defaultConfig.path)
app.delegate = delegate
app.run()
