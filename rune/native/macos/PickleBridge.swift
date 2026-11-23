import AppKit
import ApplicationServices
import CoreVideo
import LocalAuthentication
import ScreenCaptureKit
import Security
import Vision

// Every returned C string is owned by the caller and released with pickle_string_free.
// The bridge never creates an NSApplication or installs an application delegate.
func response(_ value: Any) -> UnsafeMutablePointer<CChar>? {
  guard let data = try? JSONSerialization.data(withJSONObject: ["ok": true, "value": value]),
    let string = String(data: data, encoding: .utf8)
  else { return failure("Native response could not be encoded.") }
  return strdup(string)
}
func failure(_ message: String) -> UnsafeMutablePointer<CChar>? {
  guard let data = try? JSONSerialization.data(withJSONObject: ["ok": false, "error": message]),
    let text = String(data: data, encoding: .utf8)
  else { return nil }
  return strdup(text)
}
@_cdecl("pickle_string_free") public func pickleStringFree(_ pointer: UnsafeMutablePointer<CChar>?)
{ free(pointer) }
private func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
  var value: CFTypeRef?
  return AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success ? value : nil
}
private func exclusions(_ text: UnsafePointer<CChar>?) -> Set<String> {
  guard let text, let data = String(cString: text).data(using: .utf8),
    let list = try? JSONSerialization.jsonObject(with: data) as? [String]
  else { return [] }
  return Set(list.map { $0.lowercased() })
}
@_cdecl("pickle_permission_status") public func picklePermissionStatus() -> UnsafeMutablePointer<
  CChar
>? {
  response([
    "accessibility": AXIsProcessTrusted(), "screen_capture": CGPreflightScreenCaptureAccess(),
  ])
}
// Match winit's flip_window_screen_coordinates and window backing-scale rules.
@_cdecl("pickle_window_work_area")
public func pickleWindowWorkArea(_ number: UInt32, _ fallbackX: Int32, _ fallbackY: Int32)
  -> UnsafeMutablePointer<CChar>?
{
  guard Thread.isMainThread else { return response(NSNull()) }
  let primaryHeight = CGDisplayBounds(CGMainDisplayID()).height
  let screens = NSScreen.screens
  var selected: NSScreen?
  var scale: CGFloat?
  if number != 0, let application = NSApp,
    let window = application.window(withWindowNumber: Int(number))
  {
    selected = window.screen
    if selected == nil {
      let intersections = screens.map { screen -> (NSScreen, CGFloat) in
        let intersection = screen.frame.intersection(window.frame)
        return (screen, intersection.isNull ? 0 : intersection.width * intersection.height)
      }.sorted { $0.1 > $1.1 }
      if let first = intersections.first, first.1 > 0 { selected = first.0 }
    }
    scale = window.backingScaleFactor
  }
  if selected == nil {
    // Scaled global origins can overlap on mixed-DPI desktops. With no real
    // window identity, select only one unambiguous containing screen.
    let matches = screens.filter { screen in
      let frame = screen.frame
      let s = screen.backingScaleFactor
      let left = frame.minX * s
      let top = (primaryHeight - frame.maxY) * s
      return CGFloat(fallbackX) >= left && CGFloat(fallbackX) < left + frame.width * s
        && CGFloat(fallbackY) >= top && CGFloat(fallbackY) < top + frame.height * s
    }
    if matches.count == 1 {
      selected = matches[0]
      scale = matches[0].backingScaleFactor
    }
  }
  guard let screen = selected, let scale, scale > 0 else { return response(NSNull()) }
  let visible = screen.visibleFrame
  return response([
    "visible_x": visible.minX, "visible_y": visible.minY, "width": visible.width,
    "height": visible.height, "primary_height": primaryHeight, "scale": scale,
  ])
}

@_cdecl("pickle_screen_work_areas") public func pickleScreenWorkAreas()
  -> UnsafeMutablePointer<CChar>?
{
  guard Thread.isMainThread else { return response(NSNull()) }
  return response([
    "primary_height": CGDisplayBounds(CGMainDisplayID()).height,
    "screens": NSScreen.screens.map { screen -> [String: Any] in
      let frame = screen.frame
      let visible = screen.visibleFrame
      return [
        "frame": [frame.minX, frame.minY, frame.width, frame.height],
        "visible": [visible.minX, visible.minY, visible.width, visible.height],
        "scale": screen.backingScaleFactor,
      ]
    },
  ])
}

@_cdecl("pickle_request_accessibility") public func pickleRequestAccessibility() {
  _ = AXIsProcessTrustedWithOptions(
    [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary)
}
@_cdecl("pickle_request_screen_capture") public func pickleRequestScreenCapture() {
  _ = CGRequestScreenCaptureAccess()
}

@_cdecl("pickle_capture_selection") public func pickleCaptureSelection(
  _ excluded: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>? {
  guard Thread.isMainThread else {
    return failure("Capture the source on the application UI thread before opening the reader.")
  }
  guard let app = NSWorkspace.shared.frontmostApplication,
    app.processIdentifier != ProcessInfo.processInfo.processIdentifier
  else {
    return failure("Select text in another app, then invoke Pickle. You can also paste a passage.")
  }
  let bundle = app.bundleIdentifier ?? "unknown"
  guard !exclusions(excluded).contains(bundle.lowercased()) else {
    return failure("This source application is excluded in Settings.")
  }
  guard AXIsProcessTrusted() else {
    return failure(
      "Allow Pickle Accessibility access in System Settings to read selected text, or paste the passage."
    )
  }
  let root = AXUIElementCreateApplication(app.processIdentifier)
  AXUIElementSetMessagingTimeout(root, 0.35)
  guard let raw = attribute(root, kAXFocusedUIElementAttribute),
    CFGetTypeID(raw) == AXUIElementGetTypeID()
  else {
    return failure("This app does not expose a focused selection. Paste the passage instead.")
  }
  let element = raw as! AXUIElement
  var ancestor: AXUIElement? = element
  for _ in 0..<12 {
    guard let current = ancestor else { break }
    let role = attribute(current, kAXRoleAttribute) as? String ?? ""
    let subrole = attribute(current, kAXSubroleAttribute) as? String ?? ""
    guard !role.lowercased().contains("secure"), !subrole.lowercased().contains("secure"),
      (attribute(current, "AXProtectedContent") as? Bool) != true
    else {
      return failure("Pickle does not capture secure or protected fields.")
    }
    if let parent = attribute(current, kAXParentAttribute),
      CFGetTypeID(parent) == AXUIElementGetTypeID()
    {
      ancestor = (parent as! AXUIElement)
    } else {
      break
    }
  }
  var sourceURL: String?
  let browsers: Set<String> = [
    "com.apple.Safari", "com.google.Chrome", "com.microsoft.edgemac", "com.brave.Browser",
    "company.thebrowser.Browser", "company.thebrowser.dia", "org.mozilla.firefox",
  ]
  if browsers.contains(bundle) {
    var current: AXUIElement? = element
    for _ in 0..<24 {
      guard let node = current else { break }
      for key in ["AXURL", "AXDocument"] {
        let value = attribute(node, key)
        let text = (value as? URL)?.absoluteString ?? value as? String
        if let text, let parts = URLComponents(string: text),
          ["http", "https"].contains(parts.scheme?.lowercased() ?? ""), parts.host != nil,
          parts.user == nil, parts.password == nil
        {
          sourceURL = text
          break
        }
      }
      if sourceURL != nil { break }
      if let parent = attribute(node, kAXParentAttribute),
        CFGetTypeID(parent) == AXUIElementGetTypeID()
      {
        current = (parent as! AXUIElement)
      } else {
        break
      }
    }
  }
  let text = attribute(element, kAXSelectedTextAttribute) as? String ?? ""
  guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || sourceURL != nil else {
    return failure("No usable selection was exposed. Select text again or paste the passage.")
  }
  guard text.utf8.count <= 12_000 else {
    return failure("This selection is too long. Select a shorter passage.")
  }
  var bounds: [String: Double]?
  if let range = attribute(element, kAXSelectedTextRangeAttribute) {
    var raw: CFTypeRef?
    if AXUIElementCopyParameterizedAttributeValue(
      element, kAXBoundsForRangeParameterizedAttribute as CFString, range, &raw) == .success,
      let raw, CFGetTypeID(raw) == AXValueGetTypeID()
    {
      var rect = CGRect.zero
      if AXValueGetValue(raw as! AXValue, .cgRect, &rect), !rect.isEmpty,
        [rect.minX, rect.minY, rect.width, rect.height].allSatisfy({ $0.isFinite })
      {
        bounds = ["x": rect.minX, "y": rect.minY, "width": rect.width, "height": rect.height]
      }
    }
  }
  guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else {
    return failure("The source changed during capture. Select the passage again.")
  }
  let window =
    (CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
    as? [[String: Any]])?.first {
      ($0[kCGWindowOwnerPID as String] as? Int32) == app.processIdentifier
        && ($0[kCGWindowLayer as String] as? Int) == 0
        && ($0[kCGWindowAlpha as String] as? Double ?? 1) > 0
    }?[kCGWindowNumber as String] as? UInt32
  return response([
    "text": text, "app_name": app.localizedName ?? "Application", "app_id": bundle,
    "bounds": bounds as Any? ?? NSNull(), "source_url": sourceURL as Any? ?? NSNull(),
    "source_pid": app.processIdentifier, "source_window": window as Any? ?? NSNull(),
  ])
}

private let credentialService = "com.pickle.reader.rune.credentials"
@_cdecl("pickle_credential_get") public func pickleCredentialGet(
  _ account: UnsafePointer<CChar>, _ interactive: Int32
) -> UnsafeMutablePointer<CChar>? {
  let authentication = LAContext()
  authentication.interactionNotAllowed = interactive == 0
  let query: [String: Any] = [
    kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: credentialService,
    kSecAttrAccount as String: String(cString: account), kSecReturnData as String: true,
    kSecMatchLimit as String: kSecMatchLimitOne,
    kSecUseAuthenticationContext as String: authentication,
  ]
  var raw: CFTypeRef?
  let status = SecItemCopyMatching(query as CFDictionary, &raw)
  if status == errSecItemNotFound { return response(NSNull()) }
  if status == errSecInteractionNotAllowed || status == errSecAuthFailed {
    return failure("Authorize saved credentials in Settings, or save a new access key.")
  }
  guard status == errSecSuccess, let data = raw as? Data,
    let token = String(data: data, encoding: .utf8)
  else { return failure("Keychain could not read this credential.") }
  return response(token)
}
@_cdecl("pickle_credential_set") public func pickleCredentialSet(
  _ account: UnsafePointer<CChar>, _ token: UnsafePointer<CChar>
) -> UnsafeMutablePointer<CChar>? {
  let query: [String: Any] = [
    kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: credentialService,
    kSecAttrAccount as String: String(cString: account),
  ]
  let value = String(cString: token)
  if value.isEmpty {
    let status = SecItemDelete(query as CFDictionary)
    return status == errSecSuccess || status == errSecItemNotFound
      ? response(true) : failure("Keychain could not remove this credential.")
  }
  let update = [kSecValueData as String: Data(value.utf8)]
  var status = SecItemUpdate(query as CFDictionary, update as CFDictionary)
  if status == errSecItemNotFound {
    var item = query.merging(update) { _, new in new }
    item[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
    status = SecItemAdd(item as CFDictionary, nil)
  }
  return status == errSecSuccess
    ? response(true) : failure("Keychain could not save this credential.")
}

public typealias PageCallback = @convention(c) (
  UnsafeMutableRawPointer?, UnsafeMutablePointer<CChar>?
) -> Void
private final class PageOperation {
  var task: Task<Void, Never>?
  deinit { task?.cancel() }
}

enum PageCaptureError: Error { case invalidGeometry }

// SCContentFilter owns the capture geometry: its contentRect is in points and
// pointPixelScale is the matching source backing scale (both macOS 14 APIs).
// Never assume every source is 2x, or infer the filter's content from SCWindow.frame.
func pageCaptureConfiguration(contentRect: CGRect, pointPixelScale: Float) throws
  -> SCStreamConfiguration
{
  let scale = Double(pointPixelScale)
  let nativeWidth = Double(contentRect.width) * scale
  let nativeHeight = Double(contentRect.height) * scale
  guard !contentRect.isNull, !contentRect.isInfinite,
    [nativeWidth, nativeHeight, scale].allSatisfy({ $0.isFinite && $0 > 0 })
  else { throw PageCaptureError.invalidGeometry }
  let reduction = min(1, 2000 / max(nativeWidth, nativeHeight))
  let width = Int((nativeWidth * reduction).rounded())
  let height = Int((nativeHeight * reduction).rounded())
  guard (1...2000).contains(width), (1...2000).contains(height)
  else { throw PageCaptureError.invalidGeometry }
  let config = SCStreamConfiguration()
  config.width = width
  config.height = height
  config.captureResolution = .best
  // With dimensions derived from actual backing pixels there is no upscaling
  // to perform. False permits only proportional downscaling for the pixel cap.
  config.scalesToFit = false
  config.preservesAspectRatio = true
  config.ignoreShadowsSingleWindow = true
  config.pixelFormat = kCVPixelFormatType_32BGRA
  config.colorSpaceName = CGColorSpace.sRGB
  config.showsCursor = false
  return config
}
@_cdecl("pickle_capture_page_start") public func pickleCapturePage(
  _ input: UnsafePointer<CChar>, _ callback: @escaping PageCallback,
  _ context: UnsafeMutableRawPointer?
) -> UnsafeMutableRawPointer? {
  guard let data = String(cString: input).data(using: .utf8),
    let target = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
    let windowID = target["source_window"] as? UInt32, let pid = target["source_pid"] as? Int32,
    let bundle = target["app_id"] as? String
  else {
    callback(context, failure("This session has no source window."))
    return nil
  }
  guard CGPreflightScreenCaptureAccess() else {
    callback(context, failure("Allow screen recording access to include page context."))
    return nil
  }
  let operation = PageOperation()
  operation.task = Task {
    do {
      let available = try await SCShareableContent.excludingDesktopWindows(
        true, onScreenWindowsOnly: true)
      try Task.checkCancellation()
      guard
        let window = available.windows.first(where: {
          $0.windowID == windowID && $0.owningApplication?.processID == pid
            && $0.owningApplication?.bundleIdentifier == bundle
        }), window.frame.width > 0, window.frame.height > 0
      else { throw NSError(domain: "Pickle", code: 1) }
      let filter = SCContentFilter(desktopIndependentWindow: window)
      let config = try pageCaptureConfiguration(
        contentRect: filter.contentRect, pointPixelScale: filter.pointPixelScale)
      let image = try await SCScreenshotManager.captureImage(
        contentFilter: filter, configuration: config)
      try Task.checkCancellation()
      let worker = Task.detached { try processPage(image) }
      let result = try await withTaskCancellationHandler(
        operation: { try await worker.value }, onCancel: { worker.cancel() })
      try Task.checkCancellation()
      callback(context, response(result))
    } catch {
      callback(
        context,
        failure(
          "The source window could not be captured. Continue with the passage or select it again."))
    }
  }
  return Unmanaged.passRetained(operation).toOpaque()
}
@_cdecl("pickle_capture_page_release") public func pickleCapturePageRelease(
  _ handle: UnsafeMutableRawPointer
) {
  let operation = Unmanaged<PageOperation>.fromOpaque(handle).takeRetainedValue()
  operation.task?.cancel()
}
// Internal so the standalone content-free fixture can exercise this exact path.
func processPage(_ image: CGImage) throws -> [String: Any] {
  try Task.checkCancellation()
  let request = VNRecognizeTextRequest()
  request.recognitionLevel = .accurate
  request.usesLanguageCorrection = true
  request.automaticallyDetectsLanguage = true
  // Explicitly retain small screen text on every supported OS. Apple's public
  // documentation and newer SDK header differ about this property's default.
  request.minimumTextHeight = 0
  try VNImageRequestHandler(cgImage: image).perform([request])
  try Task.checkCancellation()
  let full = (request.results ?? []).compactMap { $0.topCandidates(1).first }.filter {
    $0.confidence >= 0.35
  }.map(\.string).joined(separator: "\n")
  var text = ""
  for character in full {
    if text.utf8.count + String(character).utf8.count > 6000 { break }
    text.append(character)
  }
  let scale = min(1, 1440 / Double(max(image.width, image.height)))
  let width = max(1, Int((Double(image.width) * scale).rounded()))
  let height = max(1, Int((Double(image.height) * scale).rounded()))
  guard
    let canvas = CGContext(
      data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
      space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
  else { throw NSError(domain: "Pickle", code: 2) }
  canvas.setFillColor(CGColor(gray: 1, alpha: 1))
  canvas.fill(CGRect(x: 0, y: 0, width: width, height: height))
  canvas.interpolationQuality = .high
  canvas.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
  guard let resized = canvas.makeImage() else { throw NSError(domain: "Pickle", code: 3) }
  let bitmap = NSBitmapImageRep(cgImage: resized)
  for quality in [0.7, 0.5, 0.3] {
    if let data = bitmap.representation(using: .jpeg, properties: [.compressionFactor: quality]),
      data.count <= 750000
    {
      return ["jpeg": Array(data), "text": text, "truncated": text.utf8.count < full.utf8.count]
    }
  }
  throw NSError(domain: "Pickle", code: 4)
}
