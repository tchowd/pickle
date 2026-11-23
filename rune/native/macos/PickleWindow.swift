import AppKit
import ObjectiveC

private enum WindowDragError: Error { case message(String) }
private typealias WindowDragMethod = @convention(c) (NSWindow, Selector, NSEvent) -> Void

// Rune 67d73a5 moves winit's view into a nonactivating panel but omits
// performWindowDragWithEvent: from its source-window forwards. Keep Rune's
// existing drag-region hit testing and forward only that missing native call.
private final class WindowDragForwarder {
  let original: WindowDragMethod
  let targets = NSMapTable<AnyObject, AnyObject>.weakToWeakObjects()
  init(original: IMP) { self.original = unsafeBitCast(original, to: WindowDragMethod.self) }
}
private var windowDragForwarders: [ObjectIdentifier: WindowDragForwarder] = [:]
private let windowDragSelector = #selector(NSWindow.performDrag(with:))

private func dragForwarder(for source: NSWindow) -> WindowDragForwarder? {
  var current: AnyClass? = object_getClass(source)
  while let type = current {
    if let forwarder = windowDragForwarders[ObjectIdentifier(type)] { return forwarder }
    current = class_getSuperclass(type)
  }
  return nil
}

private func forwardWindowDrag(_ source: NSWindow, _ selector: Selector, _ event: NSEvent) {
  // NSWindow and its weak mapping are exclusively main-thread owned.
  precondition(Thread.isMainThread)
  guard let forwarder = dragForwarder(for: source) else { return }
  if let target = forwarder.targets.object(forKey: source) as? NSWindow, target !== source,
    event.window === target
  {
    target.performDrag(with: event)
  } else {
    // Unregistered windows, expired pairs, and events for a different window
    // retain the exact implementation inherited before our adapter installed.
    forwarder.original(source, selector, event)
  }
}

private func installWindowDragForwarder(on sourceClass: AnyClass) throws -> WindowDragForwarder {
  precondition(Thread.isMainThread)
  let implementation = unsafeBitCast(forwardWindowDrag as WindowDragMethod, to: IMP.self)
  if let existing = windowDragForwarders[ObjectIdentifier(sourceClass)] {
    guard let method = class_getInstanceMethod(sourceClass, windowDragSelector),
      unsafeBitCast(method_getImplementation(method), to: UInt.self)
        == unsafeBitCast(implementation, to: UInt.self)
    else { throw WindowDragError.message("The window drag implementation changed unexpectedly.") }
    return existing
  }
  guard sourceClass !== NSWindow.self,
    let method = class_getInstanceMethod(sourceClass, windowDragSelector),
    let encoding = method_getTypeEncoding(method)
  else { throw WindowDragError.message("The window drag implementation is unavailable.") }
  let forwarder = WindowDragForwarder(original: method_getImplementation(method))
  // Add only an inherited selector on winit's declared source class. Never
  // replace a foreign override, change NSWindow, or subclass a KVO instance.
  guard class_addMethod(sourceClass, windowDragSelector, implementation, encoding) else {
    throw WindowDragError.message("The window already has an unsupported drag override.")
  }
  windowDragForwarders[ObjectIdentifier(sourceClass)] = forwarder
  return forwarder
}

@_cdecl("pickle_configure_panel_drag")
public func pickleConfigurePanelDrag(_ number: UInt32) -> UnsafeMutablePointer<CChar>? {
  guard Thread.isMainThread else {
    return failure("Configure window dragging on the application UI thread.")
  }
  guard number != 0, let app = NSApp,
    let panel = app.window(withWindowNumber: Int(number)) as? NSPanel,
    panel.styleMask.contains(.nonactivatingPanel),
    let sourceClass = NSClassFromString("WinitWindow"),
    let delegate = panel.delegate
  else { return failure("Could not enable window dragging for this Pickle panel. Close and reopen Pickle.") }
  let sources = app.windows.filter {
    $0 !== panel && $0.isKind(of: sourceClass)
      && ($0.delegate as AnyObject?) === (delegate as AnyObject)
  }
  guard sources.count == 1 else {
    return failure("Could not identify Pickle's source window for dragging. Close and reopen Pickle.")
  }
  do {
    let forwarder = try installWindowDragForwarder(on: sourceClass)
    forwarder.targets.setObject(panel, forKey: sources[0])
    return response(true)
  } catch WindowDragError.message(let message) {
    return failure("Window dragging is unavailable: \(message) Close and reopen Pickle.")
  } catch { return failure("Could not enable window dragging. Close and reopen Pickle.") }
}

#if PICKLE_WINDOW_FIXTURE
private var fixtureDragCalls: [String] = []
private class FixtureDragBase: NSWindow {
  override func performDrag(with event: NSEvent) { fixtureDragCalls.append("source") }
}
private class FixtureDragSource: FixtureDragBase {}
private class FixtureForeignDragSource: FixtureDragBase {
  override func performDrag(with event: NSEvent) { fixtureDragCalls.append("foreign") }
}
private final class FixtureDragPanel: NSPanel {
  override func performDrag(with event: NSEvent) { fixtureDragCalls.append("panel") }
}

func runWindowDragFixture() throws {
  func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw WindowDragError.message(message) }
  }
  func event(for window: NSWindow?) throws -> NSEvent {
    guard let event = NSEvent.mouseEvent(
      with: .leftMouseDown, location: .zero, modifierFlags: [], timestamp: 0,
      windowNumber: window?.windowNumber ?? 0, context: nil,
      eventNumber: 0, clickCount: 1, pressure: 1)
    else { throw WindowDragError.message("Could not create fixed mouse event.") }
    return event
  }
  func send(_ source: NSWindow, to window: NSWindow?) throws {
    try autoreleasepool { source.performDrag(with: try event(for: window)) }
  }
  let rect = NSRect(x: -10000, y: -10000, width: 200, height: 100)
  func source() -> FixtureDragSource {
    let value = FixtureDragSource(contentRect: rect, styleMask: .borderless, backing: .buffered, defer: false)
    value.isReleasedWhenClosed = false
    return value
  }
  func panel() -> FixtureDragPanel {
    let value = FixtureDragPanel(contentRect: rect, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
    value.isReleasedWhenClosed = false
    return value
  }
  var first: FixtureDragSource? = source()
  var adopted: FixtureDragPanel? = panel()
  let unrelated = source()
  let otherPanel = panel()
  let foreign = FixtureForeignDragSource(contentRect: rect, styleMask: .borderless, backing: .buffered, defer: false)
  foreign.isReleasedWhenClosed = false
  defer {
    first?.close(); adopted?.close(); unrelated.close(); otherPanel.close(); foreign.close()
    fixtureDragCalls = []
  }
  let forwarder = try installWindowDragForwarder(on: FixtureDragSource.self)
  try require(try installWindowDragForwarder(on: FixtureDragSource.self) === forwarder, "Repeated installation changed the adapter.")
  forwarder.targets.setObject(adopted!, forKey: first!)
  try send(first!, to: adopted)
  try require(fixtureDragCalls == ["panel"], "The adopted panel did not receive the drag.")
  try send(unrelated, to: adopted)
  try send(first!, to: otherPanel)
  try require(fixtureDragCalls == ["panel", "source", "source"], "Unrelated or mismatched window lost its original implementation.")
  do {
    _ = try installWindowDragForwarder(on: FixtureForeignDragSource.self)
    throw WindowDragError.message("A foreign override was replaced.")
  } catch WindowDragError.message(let message) {
    try require(message == "The window already has an unsupported drag override.", "Wrong foreign-override failure.")
  }
  try send(foreign, to: nil)
  try require(fixtureDragCalls.last == "foreign", "Foreign override did not survive.")

  // AppKit itself may retain recently closed NSWindows. Plain objects let the
  // fixture force weak expiry deterministically, using the same live source key
  // and real forwarding callback for the expired-target fallback.
  var weakValue: NSObject? = NSObject()
  weak var expiredValue = weakValue
  forwarder.targets.setObject(weakValue!, forKey: first!)
  autoreleasepool { weakValue = nil }
  try require(expiredValue == nil, "The adapter's weak mapping retained its target.")
  try autoreleasepool {
    try require(forwarder.targets.object(forKey: first!) == nil, "Expired target mapping did not clear.")
  }
  try send(first!, to: nil)
  try require(fixtureDragCalls.last == "source", "Expired target did not fall back.")
  var weakKey: NSObject? = NSObject()
  weak var expiredKey = weakKey
  forwarder.targets.setObject(otherPanel, forKey: weakKey!)
  autoreleasepool { weakKey = nil }
  try require(expiredKey == nil, "The adapter's weak mapping retained its source.")
  autoreleasepool {
    adopted?.close()
    adopted = nil
  }
  autoreleasepool {
    first?.close()
    first = nil
  }
  try require(forwarder.targets.keyEnumerator().allObjects.isEmpty, "Expired source mapping did not clear.")
}
#endif
