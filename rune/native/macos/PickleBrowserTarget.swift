import AppKit

// Metadata-only source binding. No NSApplication, AX, screen/audio permission,
// page URL, window title, selection, screenshot or audio is read here.
struct BrowserSourceApp {
  let pid: Int32
  let bundle: String
  let name: String
}
struct BrowserSourceWindow {
  let id: UInt32
  let pid: Int32
  let layer: Int
  let alpha: Double
  let bounds: CGRect
}
private func usableBrowserWindow(_ window: BrowserSourceWindow, pid: Int32) -> Bool {
  window.pid == pid && window.id > 0 && window.layer == 0 && window.alpha.isFinite && window.alpha > 0
    && [window.bounds.minX, window.bounds.minY, window.bounds.width, window.bounds.height].allSatisfy({ $0.isFinite })
    && window.bounds.width > 0 && window.bounds.height > 0
}
func bindBrowserSource(
  expected: String, ownPID: Int32, foreground: () -> BrowserSourceApp?,
  windows: () -> [BrowserSourceWindow]
) -> [String: Any]? {
  let supported: Set<String> = [
    "com.google.Chrome", "com.microsoft.edgemac", "com.brave.Browser", "company.thebrowser.Browser",
  ]
  guard supported.contains(expected), let app = foreground(), app.pid > 0, app.pid != ownPID,
    app.bundle == expected,
    let window = windows().first(where: { usableBrowserWindow($0, pid: app.pid) }),
    let current = foreground(), current.pid == app.pid, current.bundle == app.bundle,
    windows().contains(where: { $0.id == window.id && usableBrowserWindow($0, pid: app.pid) })
  else { return nil }
  return [
    "text": "", "app_name": app.name, "app_id": app.bundle, "source_pid": app.pid,
    "source_window": window.id, "bounds": NSNull(), "source_url": NSNull(),
  ]
}

private func browserForeground() -> BrowserSourceApp? {
  // Apple documents runningApplications and NSRunningApplication's atomic
  // properties as thread-safe. The caller runs on a bounded-wait worker.
  let active = NSWorkspace.shared.runningApplications.filter { $0.isActive && !$0.isTerminated }
  guard active.count == 1, let app = active.first, let bundle = app.bundleIdentifier else { return nil }
  return BrowserSourceApp(pid: app.processIdentifier, bundle: bundle, name: app.localizedName ?? "Browser")
}
private func browserWindows() -> [BrowserSourceWindow] {
  guard let rows = CGWindowListCopyWindowInfo(
    [.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]]
  else { return [] }
  return rows.prefix(4096).compactMap { row in
    guard let id = row[kCGWindowNumber as String] as? UInt32,
      let pid = row[kCGWindowOwnerPID as String] as? Int32,
      let layer = row[kCGWindowLayer as String] as? Int,
      let raw = row[kCGWindowBounds as String] as? [String: Any],
      let bounds = CGRect(dictionaryRepresentation: raw as CFDictionary)
    else { return nil }
    return BrowserSourceWindow(
      id: id, pid: pid, layer: layer, alpha: row[kCGWindowAlpha as String] as? Double ?? 1,
      bounds: bounds)
  }
}
@_cdecl("pickle_browser_target") public func pickleBrowserTarget(
  _ expected: UnsafePointer<CChar>
) -> UnsafeMutablePointer<CChar>? {
  guard let bound = bindBrowserSource(
    expected: String(cString: expected), ownPID: ProcessInfo.processInfo.processIdentifier,
    foreground: browserForeground, windows: browserWindows)
  else {
    return failure("The foreground browser could not be bound to a native source window. Return to that browser and send the reference again before listening.")
  }
  return response(bound)
}
