import AppKit
import Carbon
import Combine
import PickleCore

@MainActor final class InvocationController {
    private var hotKey: EventHotKeyRef?, eventHandler: EventHandlerRef?
    private var mouseMonitor: Any?, mouseDownMonitor: Any?, pending: Task<Void, Never>?
    private var cancellables = Set<AnyCancellable>()
    private var keyboardMonitor: Any?, localKeyboardMonitor: Any?
    private var taps = DoubleOptionTap()
    private let settings: SettingsStore
    var onInvoke: ((Bool) -> Void)?, onDrag: (() -> Void)?, onPolicyChanged: (() -> Void)?, onDismiss: (() -> Void)?
    var shortcutError: ((String) -> Void)?
    init(settings: SettingsStore) {
        self.settings = settings
        var event = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, _, pointer -> OSStatus in
            guard let pointer else { return OSStatus(eventNotHandledErr) }
            let controller = Unmanaged<InvocationController>.fromOpaque(pointer).takeUnretainedValue()
            MainActor.assumeIsolated { if !controller.settings.paused { controller.onInvoke?(false) } }
            return noErr
        }, 1, &event, Unmanaged.passUnretained(self).toOpaque(), &eventHandler)
        settings.$shortcutKey.combineLatest(settings.$shortcutModifiers, settings.$doubleOption, settings.$paused).sink { [weak self] key, mods, doubleOption, paused in
            self?.register(key: key, modifiers: mods, doubleOption: doubleOption, paused: paused)
        }.store(in: &cancellables)
        settings.$automaticMenu.combineLatest(settings.$paused).sink { [weak self] automatic, paused in self?.monitor(enabled: automatic && !paused) }.store(in: &cancellables)
        settings.objectWillChange.sink { [weak self] in
            Task { @MainActor [weak self] in self?.onPolicyChanged?() }
        }.store(in: &cancellables)
    }
    private func register(key: String, modifiers: String, doubleOption: Bool, paused: Bool) {
        if let hotKey { UnregisterEventHotKey(hotKey); self.hotKey = nil }
        if let keyboardMonitor { NSEvent.removeMonitor(keyboardMonitor); self.keyboardMonitor = nil }
        if let localKeyboardMonitor { NSEvent.removeMonitor(localKeyboardMonitor); self.localKeyboardMonitor = nil }
        taps.reset()
        guard !paused else { return }
        let events: NSEvent.EventTypeMask = [.flagsChanged, .keyDown, .leftMouseDown, .rightMouseDown]
        keyboardMonitor = NSEvent.addGlobalMonitorForEvents(matching: events) { [weak self] event in self?.handle(event, doubleOption: doubleOption) }
        localKeyboardMonitor = NSEvent.addLocalMonitorForEvents(matching: events) { [weak self] event in
            self?.handle(event, doubleOption: doubleOption)
            return event
        }
        guard !doubleOption else { return }
        let codes: [String: UInt32] = ["P": 35, "Space": 49, "Return": 36, "K": 40]
        let mask = modifiers == "Command + Shift" ? UInt32(cmdKey | shiftKey) : UInt32(controlKey | optionKey)
        let status = RegisterEventHotKey(codes[key] ?? 35, mask, EventHotKeyID(signature: 0x5049434B, id: 1), GetApplicationEventTarget(), 0, &hotKey)
        if status != noErr { shortcutError?("This shortcut is unavailable. Choose another combination in Settings.") }
    }
    private func handle(_ event: NSEvent, doubleOption: Bool) {
        guard !settings.paused else { taps.reset(); return }
        if event.type == .keyDown {
            taps.reset()
            if event.keyCode == 53 { onDismiss?() }
            return
        }
        guard event.type == .flagsChanged else { taps.reset(); return }
        guard doubleOption, [UInt16(58), 61].contains(event.keyCode),
              event.modifierFlags.intersection([.command, .control, .shift, .function]).isEmpty else { taps.reset(); return }
        if taps.optionChanged(isDown: event.modifierFlags.contains(.option), at: event.timestamp) { onInvoke?(false) }
    }
    private func monitor(enabled: Bool) {
        pending?.cancel()
        if let mouseMonitor { NSEvent.removeMonitor(mouseMonitor); self.mouseMonitor = nil }
        if let mouseDownMonitor { NSEvent.removeMonitor(mouseDownMonitor); self.mouseDownMonitor = nil }
        guard enabled else { return }
        mouseDownMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown]) { [weak self] _ in
            self?.pending?.cancel(); self?.onDrag?()
        }
        mouseMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseUp]) { [weak self] _ in
            guard let self else { return }
            self.pending?.cancel()
            self.pending = Task { @MainActor [weak self] in
                try? await Task.sleep(for: .milliseconds(160))
                guard !Task.isCancelled, let self, !self.settings.paused else { return }
                self.onInvoke?(true)
            }
        }
    }
}
