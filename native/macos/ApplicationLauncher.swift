import AppKit

// Launch Services sends a reopen event when the app is already running, just
// like opening it from the Dock. Activation alone cannot recreate closed windows.
func openOrReopenApplication(at url: URL, completion: @escaping (NSRunningApplication?, Error?) -> Void) {
    DispatchQueue.main.async {
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        configuration.createsNewApplicationInstance = false
        NSWorkspace.shared.openApplication(at: url, configuration: configuration, completionHandler: completion)
    }
}

func openOrReopenApplication(identifier: String, reason: String) {
    DispatchQueue.main.async {
        guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: identifier) else {
            NSLog("Qck: application not installed: %@", identifier)
            fputs("qck: application not installed: \(identifier)\n", stderr); fflush(stderr)
            return
        }
        fputs("qck: open/reopen \(identifier): \(reason)\n", stderr); fflush(stderr)
        openOrReopenApplication(at: url) { _, error in
            if let error {
                fputs("qck: open/reopen \(identifier) failed: \(error)\n", stderr); fflush(stderr)
            }
        }
    }
}
