import AppKit

struct ResolvedApplication {
    let url: URL
    let identifier: String
}

struct ApplicationLookupError: Error, CustomStringConvertible {
    let description: String
}

func resolveApplication(_ target: String, directories: [URL] = [
    FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Applications"),
    URL(fileURLWithPath: "/Applications"),
    URL(fileURLWithPath: "/System/Applications"),
    URL(fileURLWithPath: "/System/Library/CoreServices"),
]) throws -> ResolvedApplication {
    func resolved(_ url: URL) throws -> ResolvedApplication {
        guard let identifier = Bundle(url: url)?.bundleIdentifier, !identifier.isEmpty else {
            throw ApplicationLookupError(description: "Application has no bundle identifier: \(url.path)")
        }
        return ResolvedApplication(url: url, identifier: identifier)
    }
    if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: target)
        ?? NSRunningApplication.runningApplications(withBundleIdentifier: target).first?.bundleURL {
        return try resolved(url)
    }

    let name = target.lowercased().hasSuffix(".app") ? target : target + ".app"
    var matches = Set<URL>()
    for directory in directories {
        guard FileManager.default.fileExists(atPath: directory.path) else { continue }
        var enumerationError: Error?
        guard let entries = FileManager.default.enumerator(
            at: directory, includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles, .skipsPackageDescendants],
            errorHandler: { _, error in enumerationError = error; return false }
        ) else {
            throw ApplicationLookupError(description: "Cannot search applications in \(directory.path)")
        }
        for case let url as URL in entries {
            if url.lastPathComponent.caseInsensitiveCompare(name) == .orderedSame,
               try url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory == true {
                matches.insert(url.resolvingSymlinksInPath())
            }
        }
        if let error = enumerationError { throw error }
    }
    guard let url = matches.first else {
        throw ApplicationLookupError(description: "Application not installed: \(target). Use a bundle identifier or an app name from Applications.")
    }
    guard matches.count == 1 else {
        let paths = matches.map(\.path).sorted().joined(separator: ", ")
        throw ApplicationLookupError(description: "Ambiguous application name \(target): \(paths). Use a bundle identifier instead.")
    }
    return try resolved(url)
}

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

func openOrReopenApplication(_ application: ResolvedApplication, reason: String) {
    DispatchQueue.main.async {
        let identifier = application.identifier
        fputs("qck: open/reopen \(identifier): \(reason)\n", stderr); fflush(stderr)
        openOrReopenApplication(at: application.url) { _, error in
            if let error {
                fputs("qck: open/reopen \(identifier) failed: \(error)\n", stderr); fflush(stderr)
            }
        }
    }
}
