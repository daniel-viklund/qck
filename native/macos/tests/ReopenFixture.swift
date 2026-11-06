import AppKit

// Disposable app that intentionally starts running without a window.
final class Fixture: NSObject, NSApplicationDelegate {
    var window: NSWindow?
    func record(_ event: String) {
        let state: [String: Any] = ["event": event, "pid": ProcessInfo.processInfo.processIdentifier, "visible": window?.isVisible ?? false]
        let data = try! JSONSerialization.data(withJSONObject: state)
        try! data.write(to: Bundle.main.bundleURL.appendingPathExtension("state"), options: .atomic)
    }
    func applicationDidFinishLaunching(_ notification: Notification) { record("launched") }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 300, height: 100), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window?.title = "Qck reopen test"
        window?.makeKeyAndOrderFront(nil)
        record("reopened")
        return false
    }
}
@main struct Main {
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let fixture = Fixture()
        app.delegate = fixture
        withExtendedLifetime(fixture) { app.run() }
    }
}
