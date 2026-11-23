import AppKit
import ApplicationServices
import ScreenCaptureKit

enum BrowserPageError: Error { case unavailable }
func browserDocumentURLsMatch(_ expected: String, _ observed: String) -> Bool {
  func valid(_ value: String) -> Bool {
    guard !value.isEmpty, value.utf8.count <= 4096,
      !value.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) || CharacterSet.whitespacesAndNewlines.contains($0) || $0 == "\\" }),
      let parts = URLComponents(string: value), ["http", "https"].contains(parts.scheme ?? ""),
      parts.host?.isEmpty == false, parts.user == nil, parts.password == nil,
      let serialized = parts.url?.absoluteString
    else { return false }
    return serialized.utf8.elementsEqual(value.utf8)
  }
  return expected.utf8.elementsEqual(observed.utf8) && valid(expected) && valid(observed)
}
struct BrowserDocumentObservation<Identity: Equatable>: Equatable {
  let pid: Int32
  let window: UInt32
  let identity: Identity
  let url: String
  static func == (lhs: Self, rhs: Self) -> Bool {
    lhs.pid == rhs.pid && lhs.window == rhs.window && lhs.identity == rhs.identity
      && lhs.url.utf8.elementsEqual(rhs.url.utf8)
  }
}
struct BrowserPageWindow {
  let pid: Int32
  let id: UInt32
  let bounds: CGRect
  let eligible: Bool
}
func uniqueBrowserCaptureWindow(pid: Int32, id: UInt32, axBounds: CGRect, windows: [BrowserPageWindow]) -> Bool {
  guard id > 0, [axBounds.minX,axBounds.minY,axBounds.width,axBounds.height].allSatisfy({$0.isFinite}), axBounds.width > 0, axBounds.height > 0 else { return false }
  let matches = windows.filter {$0.pid == pid && $0.eligible && $0.bounds == axBounds}
  return matches.count == 1 && matches.first?.id == id
}
// This is observational, not an atomic browser-tab transaction. The capture
// result is never published when an observed ownership/document/URL changes.
func verifiedBrowserCapture<Identity: Equatable, Output>(
  pid: Int32, window: UInt32, expected: String, cancelled: () -> Bool,
  observe: () throws -> BrowserDocumentObservation<Identity>,
  capture: (BrowserDocumentObservation<Identity>) async throws -> Output
) async throws -> Output {
  guard !cancelled(), browserDocumentURLsMatch(expected, expected) else { throw BrowserPageError.unavailable }
  let first = try observe()
  guard first.pid == pid, first.window == window, browserDocumentURLsMatch(expected, first.url),
    !cancelled(), try observe() == first
  else { throw BrowserPageError.unavailable }
  let result = try await capture(first)
  guard !cancelled(), try observe() == first else { throw BrowserPageError.unavailable }
  return result
}
private struct AXDocumentIdentity: Equatable {
  let window: AXUIElement
  let document: AXUIElement
  static func == (lhs: Self, rhs: Self) -> Bool {
    CFEqual(lhs.window, rhs.window) && CFEqual(lhs.document, rhs.document)
  }
}
private final class BrowserDocumentReader {
  let pid: Int32
  let bundle: String
  let windowID: UInt32
  let deadline = ProcessInfo.processInfo.systemUptime + 1.8
  init(pid: Int32, bundle: String, window: UInt32) {
    self.pid = pid; self.bundle = bundle; self.windowID = window
  }
  func check() throws -> Float {
    let remaining = deadline - ProcessInfo.processInfo.systemUptime
    guard remaining > 0, !Task.isCancelled else { throw BrowserPageError.unavailable }
    return Float(min(0.08, remaining))
  }
  func prepare(_ element: AXUIElement) throws {
    guard AXUIElementSetMessagingTimeout(element, try check()) == .success else { throw BrowserPageError.unavailable }
    var owner: pid_t = 0
    guard AXUIElementGetPid(element, &owner) == .success, owner == pid else { throw BrowserPageError.unavailable }
  }
  func attribute(_ element: AXUIElement, _ name: String, optional: Bool = false) throws -> CFTypeRef? {
    try prepare(element)
    var value: CFTypeRef?
    let status = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    if optional && (status == .attributeUnsupported || status == .noValue) { return nil }
    guard status == .success else { throw BrowserPageError.unavailable }
    return value
  }
  func element(_ value: CFTypeRef?) throws -> AXUIElement {
    guard let value, CFGetTypeID(value) == AXUIElementGetTypeID() else { throw BrowserPageError.unavailable }
    return value as! AXUIElement
  }
  func children(_ element: AXUIElement, remaining: Int) throws -> [AXUIElement] {
    try prepare(element)
    var count = 0
    let status = AXUIElementGetAttributeValueCount(element, kAXChildrenAttribute as CFString, &count)
    if status == .attributeUnsupported || status == .noValue { return [] }
    guard status == .success, count >= 0, count <= remaining else { throw BrowserPageError.unavailable }
    if count == 0 { return [] }
    try prepare(element)
    var raw: CFArray?
    guard AXUIElementCopyAttributeValues(element, kAXChildrenAttribute as CFString, 0, count, &raw) == .success,
      let rows = raw as? [AXUIElement], rows.count == count
    else { throw BrowserPageError.unavailable }
    return rows
  }
  func frame(_ window: AXUIElement) throws -> CGRect {
    let position = try attribute(window, kAXPositionAttribute)
    let size = try attribute(window, kAXSizeAttribute)
    guard let position, let size, CFGetTypeID(position) == AXValueGetTypeID(), CFGetTypeID(size) == AXValueGetTypeID()
    else { throw BrowserPageError.unavailable }
    var point = CGPoint.zero; var dimensions = CGSize.zero
    guard AXValueGetValue(position as! AXValue, .cgPoint, &point), AXValueGetValue(size as! AXValue, .cgSize, &dimensions),
      [point.x,point.y,dimensions.width,dimensions.height].allSatisfy({$0.isFinite}), dimensions.width > 0, dimensions.height > 0
    else { throw BrowserPageError.unavailable }
    return CGRect(origin:point,size:dimensions)
  }
  func verifyWindow(_ window: AXUIElement) throws {
    let bounds = try frame(window)
    guard let app = NSRunningApplication(processIdentifier:pid), !app.isTerminated, app.isActive, app.bundleIdentifier == bundle,
      let rows = CGWindowListCopyWindowInfo([.optionOnScreenOnly,.excludeDesktopElements],kCGNullWindowID) as? [[String:Any]]
    else { throw BrowserPageError.unavailable }
    let candidates = rows.prefix(4096).compactMap { row -> BrowserPageWindow? in
      guard let owner = row[kCGWindowOwnerPID as String] as? Int32,
        let id = row[kCGWindowNumber as String] as? UInt32,
        let raw = row[kCGWindowBounds as String] as? [String:Any], let candidate = CGRect(dictionaryRepresentation:raw as CFDictionary)
      else { return nil }
      let eligible = row[kCGWindowLayer as String] as? Int == 0 && (row[kCGWindowAlpha as String] as? Double ?? 1) > 0 && (row[kCGWindowSharingState as String] as? Int ?? 0) != 0
      return BrowserPageWindow(pid:owner,id:id,bounds:candidate,eligible:eligible)
    }
    guard uniqueBrowserCaptureWindow(pid:pid,id:windowID,axBounds:bounds,windows:candidates) else { throw BrowserPageError.unavailable }
  }
  func observe() throws -> BrowserDocumentObservation<AXDocumentIdentity> {
    _ = try check()
    guard AXIsProcessTrusted(), CGPreflightScreenCaptureAccess(), pid != ProcessInfo.processInfo.processIdentifier else { throw BrowserPageError.unavailable }
    let app = AXUIElementCreateApplication(pid)
    let window = try element(attribute(app,kAXFocusedWindowAttribute))
    guard try attribute(window,kAXRoleAttribute) as? String == kAXWindowRole,
      (try attribute(window,kAXMinimizedAttribute,optional:true) as? Bool) != true
    else { throw BrowserPageError.unavailable }
    try verifyWindow(window)
    // Protected focus is a capture refusal; no text/value of that field is read.
    var focused = try element(attribute(app,kAXFocusedUIElementAttribute))
    var reachedWindow = false
    for _ in 0..<16 {
      let role = try attribute(focused,kAXRoleAttribute) as? String ?? ""
      let subrole = try attribute(focused,kAXSubroleAttribute,optional:true) as? String ?? ""
      guard !role.lowercased().contains("secure"), !subrole.lowercased().contains("secure"),
        (try attribute(focused,"AXProtectedContent",optional:true) as? Bool) != true
      else { throw BrowserPageError.unavailable }
      if CFEqual(focused,window) { reachedWindow = true; break }
      focused = try element(attribute(focused,kAXParentAttribute))
    }
    guard reachedWindow else { throw BrowserPageError.unavailable }
    var queue: [(AXUIElement,Int)] = [(window,0)]
    var cursor = 0; var documents: [AXUIElement] = []
    while cursor < queue.count {
      let (node,depth) = queue[cursor]; cursor += 1
      guard depth <= 12, queue.count <= 192 else { throw BrowserPageError.unavailable }
      let role = try attribute(node,kAXRoleAttribute) as? String ?? ""
      if role == "AXWebArea" {
        // Do not inspect descendants: a link/iframe URL is never a page proof.
        if (try attribute(node,"AXHidden",optional:true) as? Bool) != true { documents.append(node) }
        continue
      }
      for child in try children(node,remaining:192-queue.count) { queue.append((child,depth+1)) }
    }
    guard documents.count == 1, let document = documents.first,
      CFEqual(try element(attribute(document,kAXWindowAttribute)),window)
    else { throw BrowserPageError.unavailable }
    // Chromium's AXWebArea accessibilityURL is AXTreeData.url; other elements'
    // AXURL can instead be a link target. Never use an ancestor's first URL.
    // chromium/src/ui/accessibility/platform/ax_platform_node_cocoa.mm
    let raw = try attribute(document,kAXURLAttribute)
    let url = (raw as? URL)?.absoluteString ?? raw as? String ?? ""
    guard url.utf8.count <= 4096 else { throw BrowserPageError.unavailable }
    return BrowserDocumentObservation(pid:pid,window:windowID,identity:AXDocumentIdentity(window:window,document:document),url:url)
  }
}

private final class BrowserPageOperation {
  var task: Task<Void,Never>?
  deinit { task?.cancel() }
}
// A cancelled async OS operation may not return promptly. Keep admission held
// until its task actually finishes, rather than admitting an unbounded queue of
// cancelled ScreenCaptureKit/AX work after the Rust caller's deadline.
final class BrowserPageFlight {
  private let lock = NSLock()
  private var active = false
  func acquire() -> Bool {
    lock.lock(); defer { lock.unlock() }
    guard !active else { return false }
    active = true; return true
  }
  func release() {
    lock.lock(); defer { lock.unlock() }
    active = false
  }
}
private let browserPageFlight = BrowserPageFlight()
@_cdecl("pickle_browser_page_start") public func pickleBrowserPageStart(
  _ input: UnsafePointer<CChar>, _ callback: @escaping PageCallback, _ context: UnsafeMutableRawPointer?
) -> UnsafeMutableRawPointer? {
  guard AXIsProcessTrusted(), CGPreflightScreenCaptureAccess(),
    let bytes = String(cString:input).data(using:.utf8),
    let value = try? JSONSerialization.jsonObject(with:bytes) as? [String:Any],
    let source = value["source"] as? [String:Any], let pid = source["source_pid"] as? Int32,
    let window = source["source_window"] as? UInt32, let bundle = source["app_id"] as? String,
    let expected = value["documentURL"] as? String, browserDocumentURLsMatch(expected,expected)
  else { callback(context,failure("Screen context is unavailable for this browser capture.")); return nil }
  guard browserPageFlight.acquire() else {
    callback(context,failure("Screen context is unavailable for this browser capture.")); return nil
  }
  let operation = BrowserPageOperation()
  operation.task = Task {
    defer { browserPageFlight.release() }
    do {
      let reader = BrowserDocumentReader(pid:pid,bundle:bundle,window:window)
      let image = try await verifiedBrowserCapture(pid:pid,window:window,expected:expected,cancelled:{Task.isCancelled},observe:{try reader.observe()}) { stamp in
        let content = try await SCShareableContent.excludingDesktopWindows(true,onScreenWindowsOnly:true)
        guard let source = content.windows.first(where:{$0.windowID == window && $0.owningApplication?.processID == pid && $0.owningApplication?.bundleIdentifier == bundle}) else { throw BrowserPageError.unavailable }
        let filter = SCContentFilter(desktopIndependentWindow:source)
        let config = try pageCaptureConfiguration(contentRect:filter.contentRect,pointPixelScale:filter.pointPixelScale)
        guard !Task.isCancelled, try reader.observe() == stamp else { throw BrowserPageError.unavailable }
        return try await SCScreenshotManager.captureImage(contentFilter:filter,configuration:config)
      }
      try Task.checkCancellation()
      let worker = Task.detached {try processPage(image)}
      let result = try await withTaskCancellationHandler(operation:{try await worker.value},onCancel:{worker.cancel()})
      try Task.checkCancellation()
      callback(context,response(result))
    } catch { callback(context,failure("Screen context could not be verified for this browser capture. Continue with the browser text.")) }
  }
  return Unmanaged.passRetained(operation).toOpaque()
}
@_cdecl("pickle_browser_page_release") public func pickleBrowserPageRelease(_ handle: UnsafeMutableRawPointer) {
  let operation = Unmanaged<BrowserPageOperation>.fromOpaque(handle).takeRetainedValue()
  operation.task?.cancel()
}
