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
private final class NativeShell: NSObject {
  let callback: @convention(c) (UnsafeMutableRawPointer?, Int32) -> Void
  let context: UnsafeMutableRawPointer?
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
    _ context: UnsafeMutableRawPointer?
  ) {
    self.callback = callback
    self.context = context
    super.init()
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
    restartMonitors()
  }
  @objc func pick(_ sender: NSMenuItem) { callback(context, Int32(sender.tag)) }
  func cancelAutomatic() {
    pending?.cancel()
    pending = nil
    generation &+= 1
  }
  func removeMonitors() {
    cancelAutomatic()
    for monitor in monitors { NSEvent.removeMonitor(monitor) }
    monitors = []
  }
  func restartMonitors() {
    removeMonitors()
    guard !paused else { return }
    var mask: NSEvent.EventTypeMask = [.keyDown]
    if settings.modifier_gesture {
      mask.formUnion([.flagsChanged, .leftMouseDown, .rightMouseDown])
    }
    if settings.automatic_menu { mask.formUnion([.leftMouseDown, .leftMouseUp, .rightMouseDown]) }
    if let monitor = NSEvent.addGlobalMonitorForEvents(
      matching: mask, handler: { [weak self] in self?.event($0, external: true) })
    {
      monitors.append(monitor)
    }
    if let monitor = NSEvent.addLocalMonitorForEvents(
      matching: mask,
      handler: { [weak self] event in
        self?.event(event, external: false)
        return event
      })
    {
      monitors.append(monitor)
    }
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
    if !paused && !value.modifier_gesture {
      if value.shortcut == settings.shortcut, let hotkey {
        next = hotkey
        nextID = activeHotkeyID
      } else {
        guard let (key, modifiers) = binding else { return }
        guard eventHandler != nil else {
          throw InvocationError.message(
            "The macOS shortcut service is unavailable. Try the Control + Option gesture.")
        }
        guard hotkeyGeneration < UInt32.max else {
          throw InvocationError.message("Restart Pickle before changing its shortcut again.")
        }
        hotkeyGeneration += 1
        nextID = hotkeyGeneration
        let result = RegisterEventHotKey(
          key, modifiers, EventHotKeyID(signature: 0x5049_434B, id: nextID), GetApplicationEventTarget(),
          0, &next)
        guard result == noErr else {
          throw InvocationError.message(
            "This shortcut is already in use or unavailable. Choose a different combination.")
        }
      }
    }
    if let hotkey, hotkey != next { UnregisterEventHotKey(hotkey) }
    hotkey = next
    activeHotkeyID = nextID
    settings = value
    restartMonitors()
  }
  func setPaused(_ value: Bool) throws {
    guard value != paused else { return }
    if value {
      paused = true
      activeHotkeyID = 0
      removeMonitors()
      if let hotkey {
        UnregisterEventHotKey(hotkey)
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
      UnregisterEventHotKey(hotkey)
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
