import AppKit
import Carbon

private struct InvocationSettings: Decodable {
  var shortcut: String
  var modifier_gesture: Bool
  var automatic_menu: Bool
  static let defaults = InvocationSettings(
    shortcut: "ctrl+alt+p", modifier_gesture: true, automatic_menu: false)
}
private enum InvocationError: Error { case message(String) }
// Private adapters let the rollback fixture exercise real configuration control
// flow without installing observers, registering keys, or creating application UI.
private struct InvocationOperations {
  var globalMonitor: (NSEvent.EventTypeMask, @escaping (NSEvent) -> Void) -> Any?
  var localMonitor: (NSEvent.EventTypeMask, @escaping (NSEvent) -> Void) -> Any?
  var removeMonitor: (Any) -> Void
  var registerHotkey: (UInt32, UInt32, UInt32, Bool) throws -> EventHotKeyRef
  var unregisterHotkey: (EventHotKeyRef) -> Void

  static let native = InvocationOperations(
    globalMonitor: { mask, handler in
      NSEvent.addGlobalMonitorForEvents(matching: mask, handler: handler)
    },
    localMonitor: { mask, handler in
      NSEvent.addLocalMonitorForEvents(matching: mask) { event in
        handler(event)
        return event
      }
    },
    removeMonitor: { NSEvent.removeMonitor($0) },
    registerHotkey: { key, modifiers, identifier, hasHandler in
      guard hasHandler else {
        throw InvocationError.message(
          "The macOS shortcut service is unavailable. Try the Control + Option gesture.")
      }
      var next: EventHotKeyRef?
      let result = RegisterEventHotKey(
        key, modifiers, EventHotKeyID(signature: 0x5049_434B, id: identifier),
        GetApplicationEventTarget(), 0, &next)
      guard result == noErr, let next else {
        if let next { UnregisterEventHotKey(next) }
        throw InvocationError.message(
          "This shortcut is already in use or unavailable. Choose a different combination.")
      }
      return next
    },
    unregisterHotkey: { UnregisterEventHotKey($0) })
}
private final class NativeShell: NSObject {
  let callback: @convention(c) (UnsafeMutableRawPointer?, Int32) -> Void
  let context: UnsafeMutableRawPointer?
  let operations: InvocationOperations
  var status: NSStatusItem?
  var monitors: [Any] = []
  var hotkey: EventHotKeyRef?
  var hotkeyGeneration: UInt32 = 0
  var activeHotkeyID: UInt32 = 0
  var eventHandler: EventHandlerRef?
  var settings = InvocationSettings.defaults
  var paused = false
  var pending: DispatchWorkItem?
  var generation: UInt64 = 0

  init(
    _ callback: @escaping @convention(c) (UnsafeMutableRawPointer?, Int32) -> Void,
    _ context: UnsafeMutableRawPointer?,
    operations: InvocationOperations = .native,
    installSystemUI: Bool = true
  ) {
    self.callback = callback
    self.context = context
    self.operations = operations
    super.init()
    guard installSystemUI else { return }
    status = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    status?.button?.image = NSImage(systemSymbolName: "leaf", accessibilityDescription: "Pickle")
    let menu = NSMenu()
    for (title, code) in [
      ("Open Pickle", 1), ("Paste a passage", 2), ("Settings…", 3), ("Saved answers…", 4),
      ("Pause or resume", 5), ("Quit Pickle", 6),
    ] {
      let item = NSMenuItem(
        title: title, action: #selector(pick(_:)), keyEquivalent: code == 6 ? "q" : "")
      item.target = self
      item.tag = code
      menu.addItem(item)
    }
    status?.menu = menu
    var event = EventTypeSpec(
      eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
    InstallEventHandler(
      GetApplicationEventTarget(),
      { _, event, context in
        guard let context, let event else { return OSStatus(eventNotHandledErr) }
        var identifier = EventHotKeyID()
        guard
          GetEventParameter(
            event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil,
            MemoryLayout<EventHotKeyID>.size, nil, &identifier) == noErr,
          identifier.signature == 0x5049_434B
        else { return OSStatus(eventNotHandledErr) }
        let shell = Unmanaged<NativeShell>.fromOpaque(context).takeUnretainedValue()
        // Carbon can retain already queued events after unregistering a key.
        // Each registration receives an ID that is never reused this session.
        if !shell.paused, !shell.settings.modifier_gesture, shell.hotkey != nil,
          identifier.id == shell.activeHotkeyID
        {
          shell.cancelAutomatic()
          shell.callback(shell.context, 0)
        }
        return noErr
      }, 1, &event, Unmanaged.passUnretained(self).toOpaque(), &eventHandler)
    // Rust configures immediately after installation. Keep this menu-only shell
    // usable if the initial observer registration fails, so settings can recover.
  }
  @objc func pick(_ sender: NSMenuItem) { callback(context, Int32(sender.tag)) }
  func cancelAutomatic() {
    pending?.cancel()
    pending = nil
    generation &+= 1
  }
  func removeMonitors() {
    cancelAutomatic()
    for monitor in monitors { operations.removeMonitor(monitor) }
    monitors = []
  }
  func stageMonitors(_ value: InvocationSettings) throws -> [Any] {
    var mask: NSEvent.EventTypeMask = [.keyDown]
    if value.modifier_gesture {
      mask.formUnion([.flagsChanged, .leftMouseDown, .rightMouseDown])
    }
    if value.automatic_menu { mask.formUnion([.leftMouseDown, .leftMouseUp, .rightMouseDown]) }
    guard let global = operations.globalMonitor(
      mask, { [weak self] in self?.event($0, external: true) })
    else {
      throw InvocationError.message(
        "Could not register the macOS global invocation observer. Review selection access in Settings, then try again.")
    }
    guard let local = operations.localMonitor(
      mask, { [weak self] in self?.event($0, external: false) })
    else {
      operations.removeMonitor(global)
      throw InvocationError.message(
        "Could not register the macOS local invocation observer. Try again or restart Pickle.")
    }
    return [global, local]
  }
  func event(_ event: NSEvent, external: Bool) {
    guard !paused else { return }
    if event.type == .keyDown { cancelAutomatic() }
    if event.type == .keyDown && event.keyCode == 53 { callback(context, 7) }
    let flags = event.modifierFlags
    let bits: Int32 =
      (flags.contains(.control) ? 1 : 0) | (flags.contains(.option) ? 2 : 0)
      | (!flags.intersection([.command, .shift, .function]).isEmpty ? 4 : 0)
    if settings.modifier_gesture {
      if event.type == .flagsChanged {
        cancelAutomatic()
        callback(context, 100 + bits)
      } else if event.type != .leftMouseUp {
        callback(context, 200 + bits)
      }
    }
    guard external && settings.automatic_menu else { return }
    if event.type == .leftMouseDown || event.type == .rightMouseDown {
      cancelAutomatic()
      callback(context, 9)
    } else if event.type == .leftMouseUp {
      cancelAutomatic()
      let revision = generation
      let work = DispatchWorkItem { [weak self] in
        guard let self, !self.paused, self.settings.automatic_menu, self.generation == revision
        else { return }
        self.pending = nil
        self.callback(self.context, 8)
      }
      pending = work
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.18, execute: work)
    }
  }
  func configure(_ value: InvocationSettings) throws {
    var next: EventHotKeyRef?
    var nextID: UInt32 = 0
    // Validate a dormant conventional binding too, so pausing cannot save a key
    // that is only discovered to be unsupported when the user resumes.
    let binding = value.modifier_gesture ? nil : try shortcut(value.shortcut)
    // Stage both required observers before touching the active binding. These
    // main-thread APIs do not pump the event loop, so staged observers cannot
    // deliver between staging and the commit below.
    let nextMonitors = paused ? [] : try stageMonitors(value)
    var committed = false
    defer {
      if !committed {
        for monitor in nextMonitors { operations.removeMonitor(monitor) }
        if let next, next != hotkey { operations.unregisterHotkey(next) }
      }
    }
    if !paused && !value.modifier_gesture {
      if value.shortcut == settings.shortcut, let hotkey {
        next = hotkey
        nextID = activeHotkeyID
      } else {
        guard let (key, modifiers) = binding else {
          throw InvocationError.message("Choose a valid shortcut.")
        }
        guard hotkeyGeneration < UInt32.max else {
          throw InvocationError.message("Restart Pickle before changing its shortcut again.")
        }
        hotkeyGeneration += 1
        nextID = hotkeyGeneration
        next = try operations.registerHotkey(key, modifiers, nextID, eventHandler != nil)
      }
    }
    removeMonitors()
    if let hotkey, hotkey != next { operations.unregisterHotkey(hotkey) }
    hotkey = next
    activeHotkeyID = nextID
    settings = value
    monitors = nextMonitors
    committed = true
  }
  func setPaused(_ value: Bool) throws {
    guard value != paused else { return }
    if value {
      paused = true
      activeHotkeyID = 0
      removeMonitors()
      if let hotkey {
        operations.unregisterHotkey(hotkey)
        self.hotkey = nil
      }
    } else {
      paused = false
      do { try configure(settings) } catch {
        paused = true
        removeMonitors()
        throw error
      }
    }
    status?.button?.image = NSImage(
      systemSymbolName: paused ? "pause.circle" : "leaf",
      accessibilityDescription: paused ? "Pickle paused" : "Pickle")
  }
  func stop() {
    activeHotkeyID = 0
    removeMonitors()
    if let hotkey {
      operations.unregisterHotkey(hotkey)
      self.hotkey = nil
    }
    if let eventHandler {
      RemoveEventHandler(eventHandler)
      self.eventHandler = nil
    }
    if let status { NSStatusBar.system.removeStatusItem(status) }
    status = nil
  }
}

private func shortcut(_ source: String) throws -> (UInt32, UInt32) {
  let parts = source.split(separator: "+").map(String.init)
  guard let key = parts.last else { throw InvocationError.message("Choose a valid shortcut.") }
  let codes: [String: UInt32] = [
    "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11,
    "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21,
    "6": 22, "5": 23, "equal": 24, "9": 25, "7": 26, "minus": 27, "8": 28, "0": 29,
    "rightbracket": 30, "o": 31, "u": 32, "leftbracket": 33, "i": 34, "p": 35, "return": 36,
    "l": 37, "j": 38, "quote": 39, "k": 40, "semicolon": 41, "backslash": 42, "comma": 43,
    "slash": 44, "n": 45, "m": 46, "period": 47, "tab": 48, "space": 49, "backquote": 50,
    "escape": 53, "f1": 122, "f2": 120, "f3": 99, "f4": 118, "f5": 96, "f6": 97, "f7": 98,
    "f8": 100, "f9": 101, "f10": 109, "f11": 103, "f12": 111, "f13": 105, "f14": 107, "f15": 113,
    "f16": 106, "f17": 64, "f18": 79, "f19": 80, "f20": 90,
  ]
  guard let code = codes[key] else {
    throw InvocationError.message(
      "This key is not supported for a macOS shortcut. Choose a letter, digit, Space, or F1–F20.")
  }
  var flags: UInt32 = 0
  for modifier in parts.dropLast() {
    switch modifier {
    case "ctrl": flags |= UInt32(controlKey)
    case "alt": flags |= UInt32(optionKey)
    case "shift": flags |= UInt32(shiftKey)
    case "super": flags |= UInt32(cmdKey)
    default: throw InvocationError.message("Choose a valid shortcut modifier.")
    }
  }
  return (code, flags)
}
@_cdecl("pickle_shell_install") public func pickleShellInstall(
  _ callback: @escaping @convention(c) (UnsafeMutableRawPointer?, Int32) -> Void,
  _ context: UnsafeMutableRawPointer?
) -> UnsafeMutableRawPointer? {
  guard Thread.isMainThread else { return nil }
  return Unmanaged.passRetained(NativeShell(callback, context)).toOpaque()
}
@_cdecl("pickle_shell_configure") public func pickleShellConfigure(
  _ handle: UnsafeMutableRawPointer, _ input: UnsafePointer<CChar>
) -> UnsafeMutablePointer<CChar>? {
  guard Thread.isMainThread else {
    return failure("Configure invocation on the application UI thread.")
  }
  guard let data = String(cString: input).data(using: .utf8),
    let settings = try? JSONDecoder().decode(InvocationSettings.self, from: data)
  else { return failure("The invocation settings are invalid.") }
  do {
    try Unmanaged<NativeShell>.fromOpaque(handle).takeUnretainedValue().configure(settings)
    return response(true)
  } catch InvocationError.message(let message) { return failure(message) } catch {
    return failure("Could not configure invocation.")
  }
}
@_cdecl("pickle_shell_pause") public func pickleShellPause(
  _ handle: UnsafeMutableRawPointer, _ paused: Int32
) -> UnsafeMutablePointer<CChar>? {
  guard Thread.isMainThread else {
    return failure("Pause invocation on the application UI thread.")
  }
  do {
    try Unmanaged<NativeShell>.fromOpaque(handle).takeUnretainedValue().setPaused(paused != 0)
    return response(true)
  } catch InvocationError.message(let message) { return failure(message) } catch {
    return failure("Could not resume invocation.")
  }
}
@_cdecl("pickle_shell_remove") public func pickleShellRemove(_ handle: UnsafeMutableRawPointer) {
  precondition(Thread.isMainThread)
  let shell = Unmanaged<NativeShell>.fromOpaque(handle).takeRetainedValue()
  shell.stop()
}

#if PICKLE_INVOCATION_FIXTURE
private final class InvocationFixtureOperations {
  var failGlobal = false
  var failLocal = false
  var failHotkey = false
  var serial = 0
  var removed: [Int] = []
  var unregistered: [EventHotKeyRef] = []
  var masks: [NSEvent.EventTypeMask] = []

  func monitor(_ mask: NSEvent.EventTypeMask, fail: Bool) -> Any? {
    masks.append(mask)
    guard !fail else { return nil }
    serial += 1
    return serial
  }
  var operations: InvocationOperations {
    InvocationOperations(
      globalMonitor: { mask, _ in self.monitor(mask, fail: self.failGlobal) },
      localMonitor: { mask, _ in self.monitor(mask, fail: self.failLocal) },
      removeMonitor: { self.removed.append($0 as! Int) },
      registerHotkey: { _, _, _, _ in
        if self.failHotkey { throw InvocationError.message("Fixed registration failure.") }
        self.serial += 1
        return EventHotKeyRef(bitPattern: self.serial)!
      },
      unregisterHotkey: { self.unregistered.append($0) })
  }
}

func runInvocationTransactionFixture() throws {
  func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw InvocationError.message(message) }
  }
  func expectFailure(_ action: () throws -> Void) throws {
    do { try action() } catch InvocationError.message { return }
    throw InvocationError.message("Expected a configuration failure.")
  }
  let operations = InvocationFixtureOperations()
  let shell = NativeShell({ _, _ in }, nil, operations: operations.operations, installSystemUI: false)
  let gesture = InvocationSettings.defaults
  let conventional = InvocationSettings(
    shortcut: "ctrl+alt+p", modifier_gesture: false, automatic_menu: true)

  // Initial failure retains a usable menu-only shell; retry can configure it.
  operations.failGlobal = true
  try expectFailure { try shell.configure(gesture) }
  try require(shell.monitors.isEmpty && shell.hotkey == nil, "Initial failure installed resources.")
  operations.failGlobal = false
  try shell.configure(gesture)
  let original = shell.monitors.map { $0 as! Int }
  try require(original.count == 2, "Required observers were not both installed.")
  try require(operations.masks.last!.contains(.flagsChanged), "Gesture observer mask is incomplete.")

  // A failed global or local observer must preserve every old token and setting.
  operations.failGlobal = true
  try expectFailure { try shell.configure(conventional) }
  try require(shell.monitors.map { $0 as! Int } == original, "Global failure replaced observers.")
  try require(operations.removed.isEmpty, "Global failure removed active observers.")
  operations.failGlobal = false
  operations.failLocal = true
  try expectFailure { try shell.configure(conventional) }
  try require(shell.monitors.map { $0 as! Int } == original, "Local failure replaced observers.")
  try require(operations.removed.count == 1, "Local failure leaked its staged global observer.")
  try require(!operations.removed.contains(where: original.contains), "Local failure removed active observers.")
  try require(shell.settings.modifier_gesture && shell.hotkey == nil, "Failure committed configuration.")

  // Failed Carbon registration removes only newly staged observers.
  operations.failLocal = false
  operations.failHotkey = true
  let beforeKeyFailure = operations.removed.count
  try expectFailure { try shell.configure(conventional) }
  try require(operations.removed.count == beforeKeyFailure + 2, "Hotkey failure leaked observers.")
  try require(shell.monitors.map { $0 as! Int } == original, "Hotkey failure replaced observers.")
  try require(shell.settings.modifier_gesture, "Hotkey failure changed the active mode.")

  operations.failHotkey = false
  try shell.configure(conventional)
  let active = shell.monitors.map { $0 as! Int }
  let oldHotkey = shell.hotkey!
  try require(original.allSatisfy(operations.removed.contains), "Commit leaked replaced observers.")
  try require(!shell.settings.modifier_gesture && shell.settings.automatic_menu, "Commit lost settings.")
  try require(operations.masks.last!.contains(.leftMouseUp), "Automatic observer mask is incomplete.")

  // Failure while changing an active conventional key retains that key and ID.
  let oldID = shell.activeHotkeyID
  operations.failHotkey = true
  try expectFailure {
    try shell.configure(InvocationSettings(
      shortcut: "ctrl+alt+q", modifier_gesture: false, automatic_menu: false))
  }
  try require(shell.hotkey == oldHotkey && shell.activeHotkeyID == oldID, "Failure replaced the active hotkey.")
  try require(shell.monitors.map { $0 as! Int } == active, "Failure replaced active conventional observers.")
  try require(operations.unregistered.isEmpty, "Failure unregistered the active hotkey.")

  // Pause unregisters resources; failed resume remains paused without leaks.
  try shell.setPaused(true)
  try require(shell.monitors.isEmpty && shell.hotkey == nil, "Pause retained invocation resources.")
  try require(operations.unregistered == [oldHotkey], "Pause did not unregister exactly its hotkey.")
  operations.failHotkey = false
  operations.failLocal = true
  try expectFailure { try shell.setPaused(false) }
  try require(shell.paused && shell.monitors.isEmpty && shell.hotkey == nil, "Failed resume became active.")
  operations.failLocal = false
  try shell.setPaused(false)
  try require(!shell.paused && shell.monitors.count == 2 && shell.hotkey != nil, "Resume did not recover.")
  shell.stop()
  try require(shell.monitors.isEmpty && shell.hotkey == nil, "Stop leaked invocation resources.")
}
#endif
