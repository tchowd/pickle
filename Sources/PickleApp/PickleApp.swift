import SwiftUI
import AppKit
import PickleCore

@main struct PickleApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
    var body: some Scene { Settings { EmptyView() } }
}
@MainActor final class ReaderPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}
@MainActor final class ActionPanel: NSPanel {
    var acceptsKeyboard = false
    override var canBecomeKey: Bool { acceptsKeyboard }
    override var canBecomeMain: Bool { false }
}
@MainActor final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate, NSWindowDelegate, ObservableObject {
    let settings = SettingsStore(defaults: CommandLine.arguments.contains("--smoke-test") || CommandLine.arguments.contains("--preview-actions") ? UserDefaults(suiteName: "com.pickle.smoke." + UUID().uuidString)! : .standard), session = SessionStore()
    lazy var coordinator = RequestCoordinator(session: session, settings: settings)
    private var statusItem: NSStatusItem!, resultPanel: ReaderPanel?, actionPanel: ActionPanel?, settingsWindow: NSWindow?
    private var invocation: InvocationController?
    private var escapeMonitor: Any?
    private var actionExpanded = false
    private var floatingOrigin: NSPoint?
    @Published var pinned = false
    func applicationDidFinishLaunching(_ notification: Notification) {
        if let index = CommandLine.arguments.firstIndex(of: "--import-cli-credential"), CommandLine.arguments.count > index + 1 {
            let token = String(data: FileHandle.standardInput.readDataToEndOfFile(), encoding: .utf8) ?? ""
            do { try CredentialStore.save(token); settings.accountID = CommandLine.arguments[index + 1]; print("Credential imported into Keychain. OAuth tokens expire."); exit(0) }
            catch { print("Keychain import failed."); exit(1) }
        }
        NSApp.appearance = NSAppearance(named: .darkAqua)
        NSApp.setActivationPolicy(.accessory)
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.button?.image = pickleStatusIcon()
        let menu = NSMenu(); menu.delegate = self; statusItem.menu = menu
        invocation = InvocationController(settings: settings)
        invocation?.onInvoke = { [weak self] automatic in self?.capture(automatic: automatic) }
        invocation?.onDrag = { [weak self] in if self?.actionExpanded == false { self?.actionPanel?.orderOut(nil) } }
        invocation?.onDismiss = { [weak self] in self?.closePanel() }
        // Handle Escape before a text editor consumes it, including while paused.
        escapeMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, event.keyCode == 53,
                  let window = event.window,
                  window === self.resultPanel || window === self.actionPanel else { return event }
            self.closePanel()
            return nil
        }
        invocation?.onPolicyChanged = { [weak self] in
            guard let self else { return }
            self.coordinator.policyChanged()
            if !self.actionExpanded { self.actionPanel?.orderOut(nil) }
            self.statusItem.button?.image = self.settings.paused ? NSImage(systemSymbolName: "pause.circle", accessibilityDescription: "Pickle paused") : pickleStatusIcon()
        }
        invocation?.shortcutError = { [weak self] message in self?.coordinator.error = message }
        if CommandLine.arguments.contains("--smoke-test") {
            coordinator.sample(); showResult()
            Task { @MainActor in
                try? await Task.sleep(for: .seconds(1))
                let snapshotID = self.session.snapshot?.id
                let generated = self.session.result != nil && self.resultPanel?.isVisible == true
                let escape = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                    windowNumber: self.resultPanel!.windowNumber, context: nil, characters: "\u{1b}",
                    charactersIgnoringModifiers: "\u{1b}", isARepeat: false, keyCode: 53)!
                NSApp.sendEvent(escape)
                let hidden = self.resultPanel?.isVisible == false
                _ = self.applicationShouldHandleReopen(NSApp, hasVisibleWindows: false)
                let reopened = self.resultPanel?.isVisible == true && self.session.snapshot?.id == snapshotID
                let fixture = SelectionSnapshot(text: "The treatment may reduce symptoms in some patients, but the evidence remains limited.", appName: "Pickle sample", bundleID: "sample")
                self.showActions(fixture, automatic: false)
                let chooserPanel = self.actionPanel
                // Expansion must preserve a user-moved anchor, not recenter the panel.
                self.actionPanel!.setFrameOrigin(NSPoint(x: self.actionPanel!.frame.minX - 60, y: self.actionPanel!.frame.minY))
                let chooserFrame = self.actionPanel!.frame
                let chooser = self.actionPanel?.isVisible == true && self.resultPanel?.isVisible == false && self.session.snapshot?.id == fixture.id && self.coordinator.manualText == fixture.text && self.session.result == nil && self.coordinator.progress == nil
                self.chooseAction(.simplify, snapshot: fixture)
                try? await Task.sleep(for: .milliseconds(250))
                let chosen = self.actionPanel === chooserPanel && self.actionPanel?.isVisible == true && self.resultPanel?.isVisible == false && self.session.snapshot?.id == fixture.id && self.session.result != nil
                let anchored = abs(self.actionPanel!.frame.minY - chooserFrame.minY) < 1 && abs(self.actionPanel!.frame.midX - chooserFrame.midX) < 1 && self.actionPanel!.frame.height > chooserFrame.height
                self.coordinator.policyChanged()
                let stillVisible = self.actionPanel?.isVisible == true
                self.closePanel()
                _ = self.applicationShouldHandleReopen(NSApp, hasVisibleWindows: false)
                let inlineReopened = self.actionPanel === chooserPanel && self.actionPanel?.isVisible == true && self.resultPanel?.isVisible == false
                let visible = self.actionPanel!.screen!.visibleFrame
                self.actionPanel!.setFrameOrigin(NSPoint(x: visible.maxX - 60, y: visible.maxY - 60))
                self.positionFloater(self.actionPanel!, on: self.actionPanel!.screen, expanded: true)
                let clamped = visible.contains(self.actionPanel!.frame)
                let rememberedOrigin = self.actionPanel!.frame.origin
                self.showActions(fixture, automatic: false)
                let remembered = self.actionPanel!.frame.origin == rememberedOrigin
                self.session.context = "Previous context"
                self.session.conversation = [.init(question: "Previous question", answer: "Previous answer", quality: .checked)]
                self.coordinator.question = "Unsent question"
                self.capture(automatic: false) { fixture }
                let freshSelection = self.session.snapshot?.id == fixture.id && self.session.result == nil
                    && self.session.context.isEmpty && self.session.conversation.isEmpty && self.coordinator.question.isEmpty
                self.chooseAction(.simplify, snapshot: fixture)
                try? await Task.sleep(for: .milliseconds(250))
                self.capture(automatic: false) { throw PickleError.message("No text selected") }
                let freshEmpty = self.session.snapshot == nil && self.session.result == nil
                    && self.coordinator.progress == nil && self.coordinator.manualText.isEmpty
                    && self.coordinator.captureMessage == "No text selected"
                    && self.resultPanel?.isVisible == true && self.actionPanel?.isVisible == false
                let ok = generated && hidden && reopened && chooser && chosen && anchored && stillVisible && inlineReopened && clamped && remembered && freshSelection && freshEmpty
                print(ok ? "PICKLE_SMOKE_PASS: movable action bar, anchored expansion, screen-edge clamping, remembered position, reopen, and fresh shortcut sessions" : "PICKLE_SMOKE_FAIL")
                if !ok { exit(1) }
                NSApp.terminate(nil)
            }
        } else if CommandLine.arguments.contains("--preview-actions") {
            showActions(.init(text: "The treatment may reduce symptoms in some patients, but the evidence remains limited.", appName: "Pickle sample", bundleID: "sample"), automatic: false)
        } else { presentReader() }
    }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        presentReader()
        return true
    }
    private func presentReader() {
        showResult()
        NSApp.activate(ignoringOtherApps: true)
        if actionExpanded { actionPanel?.makeKeyAndOrderFront(nil) }
        else { resultPanel?.makeKeyAndOrderFront(nil) }
    }
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        func add(_ title: String, _ selector: Selector?, key: String = "") {
            let item = NSMenuItem(title: title, action: selector, keyEquivalent: key); item.target = self; menu.addItem(item)
        }
        add(settings.paused ? "Resume Pickle" : "Pause Pickle", #selector(togglePause))
        menu.addItem(.separator())
        add("Selection shortcut: \(settings.shortcutLabel)", nil)
        add("Paste a passage…", #selector(manual))
        add("Reopen current result", #selector(reopen))
        add("Try an example", #selector(sample))
        menu.addItem(.separator())
        add("Settings…", #selector(openSettings), key: ",")
        add("Clear session", #selector(clearSession))
        add("Quit Pickle", #selector(quit), key: "q")
    }
    @objc func togglePause() { settings.paused.toggle(); coordinator.cancel(); actionPanel?.orderOut(nil) }
    @objc func manual() { dismissFloater(); actionExpanded = false; coordinator.clear(); showResult() }
    @objc func reopen() { presentReader() }
    @objc func sample() { coordinator.sample(); showResult() }
    @objc func clearSession() { coordinator.clear(); actionPanel?.orderOut(nil); resultPanel?.orderOut(nil) }
    @objc func quit() { coordinator.cancel(); NSApp.terminate(nil) }
    func capture(automatic: Bool) {
        capture(automatic: automatic) {
            try SelectionService().capture(excluded: settings.isExcluded)
        }
    }
    private func capture(automatic: Bool, readSelection: () throws -> SelectionSnapshot) {
        guard !settings.paused else { return }
        if automatic && (coordinator.progress != nil || pinned) { return }
        if !automatic {
            coordinator.clear()
            actionExpanded = false
            actionPanel?.orderOut(nil)
            resultPanel?.orderOut(nil)
        }
        do {
            let snapshot = try readSelection()
            showActions(snapshot, automatic: automatic)
        } catch {
            guard !automatic else { return }
            actionPanel?.orderOut(nil)
            // A shortcut always starts fresh, even when no selection can be read.
            coordinator.captureMessage = error.localizedDescription; showResult()
        }
    }
    private func showActions(_ snapshot: SelectionSnapshot, automatic: Bool) {
        if let actionPanel { floatingOrigin = actionPanel.frame.origin }
        actionPanel?.orderOut(nil)
        actionExpanded = false
        if !automatic {
            coordinator.setSelection(snapshot)
            coordinator.manualText = snapshot.text
        }
        let panel = ActionPanel(contentRect: NSRect(x: 0, y: 0, width: 440, height: 124), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.title = "Pickle actions"
        panel.isMovable = true
        panel.level = pinned ? .floating : .normal; panel.isFloatingPanel = true; panel.hidesOnDeactivate = false; panel.isOpaque = false; panel.backgroundColor = .clear; panel.hasShadow = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        panel.contentView = NSHostingView(rootView: ActionMenu(snapshot: snapshot, choose: { [weak self] action in
            self?.chooseAction(action, snapshot: snapshot)
        }, dismiss: { [weak self] in self?.dismissFloater() }))
        resultPanel?.orderOut(nil)
        let screen = NSScreen.screens.first { $0.frame.contains(NSEvent.mouseLocation) } ?? NSScreen.main
        positionFloater(panel, on: screen, expanded: false)
        // Keep the source app focused and its selection intact until an action is clicked.
        panel.orderFrontRegardless(); actionPanel = panel
    }
    private func chooseAction(_ action: ReadingAction, snapshot: SelectionSnapshot) {
        guard !settings.paused else { return }
        if session.snapshot?.id != snapshot.id { coordinator.setSelection(snapshot) }
        if CommandLine.arguments.contains("--smoke-test") || CommandLine.arguments.contains("--preview-actions") { coordinator.isSample = true }
        guard let panel = actionPanel else { return }
        actionExpanded = true
        panel.acceptsKeyboard = true
        panel.title = "Pickle"
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.contentView = NSHostingView(rootView: ReaderView(coordinator: coordinator, session: session, settings: settings, app: self, floating: true))
        positionFloater(panel, on: panel.screen, expanded: true)
        panel.makeKeyAndOrderFront(nil)
        coordinator.run(action)
    }
    private func positionFloater(_ panel: NSPanel, on screen: NSScreen?, expanded: Bool) {
        guard let frame = (screen ?? NSScreen.main)?.visibleFrame else { return }
        let width = min(expanded ? 520 : 440, frame.width - 32)
        let height = min(expanded ? 640 : 124, frame.height - 48)
        let desired = expanded
            ? NSPoint(x: panel.frame.midX - width / 2, y: panel.frame.minY)
            : floatingOrigin ?? NSPoint(x: frame.midX - width / 2, y: frame.minY + 24)
        let x = max(frame.minX + 16, min(desired.x, frame.maxX - width - 16))
        let y = max(frame.minY + 16, min(desired.y, frame.maxY - height - 16))
        panel.setFrame(NSRect(x: x, y: y, width: width, height: height), display: true)
    }
    private func dismissFloater() {
        if actionExpanded { coordinator.cancel() }
        actionPanel?.orderOut(nil)
    }
    func showResult(snapshot: SelectionSnapshot? = nil) {
        if actionExpanded, let actionPanel {
            actionPanel.makeKeyAndOrderFront(nil)
            return
        }
        if resultPanel == nil {
            let panel = ReaderPanel(contentRect: NSRect(x: 0, y: 0, width: 540, height: 720), styleMask: [.titled, .closable, .resizable, .utilityWindow, .nonactivatingPanel], backing: .buffered, defer: false)
            panel.delegate = self; panel.title = "Pickle"; panel.minSize = NSSize(width: 420, height: 460); panel.level = pinned ? .floating : .normal; panel.isFloatingPanel = true; panel.hidesOnDeactivate = false
            panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
            panel.isOpaque = false; panel.backgroundColor = .clear
            panel.isReleasedWhenClosed = false
            panel.contentView = NSHostingView(rootView: ReaderView(coordinator: coordinator, session: session, settings: settings, app: self))
            resultPanel = panel; panel.center()
        }
        if let snapshot { position(resultPanel!, snapshot: snapshot) }
        resultPanel?.makeKeyAndOrderFront(nil)
    }
    func windowShouldClose(_ sender: NSWindow) -> Bool { if sender === resultPanel { coordinator.cancel() }; return true }
    func closePanel() { coordinator.cancel(); resultPanel?.orderOut(nil); actionPanel?.orderOut(nil) }
    func togglePin() { pinned.toggle(); resultPanel?.level = pinned ? .floating : .normal; actionPanel?.level = pinned ? .floating : .normal }
    private func position(_ panel: NSPanel, snapshot: SelectionSnapshot) {
        let pointer = NSEvent.mouseLocation
        var anchor = pointer
        if let b = snapshot.bounds, let primary = NSScreen.screens.first {
            // AX coordinates use the primary screen's top-left; AppKit uses bottom-left.
            anchor = CGPoint(x: b.x + b.width + 12, y: primary.frame.maxY - b.y)
        }
        let screen = NSScreen.screens.first { $0.frame.contains(anchor) } ?? NSScreen.screens.first { $0.frame.contains(pointer) } ?? NSScreen.main
        guard let frame = screen?.visibleFrame else { return }
        let width = min(panel.frame.width, frame.width - 24), height = min(panel.frame.height, frame.height - 24)
        let x = max(frame.minX + 12, min(anchor.x, frame.maxX - width - 12))
        let y = max(frame.minY + 12, min(anchor.y - height, frame.maxY - height - 12))
        panel.setFrame(NSRect(x: x, y: y, width: width, height: height), display: false)
    }
    @objc func openSettings() {
        if settingsWindow == nil {
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 620, height: 760), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
            window.title = "Pickle Settings"; window.isReleasedWhenClosed = false
            window.isOpaque = false; window.backgroundColor = .clear
            window.minSize = NSSize(width: 560, height: 500)
            window.contentView = NSHostingView(rootView: SettingsView(settings: settings, coordinator: coordinator))
            window.center(); settingsWindow = window
        }
        NSApp.activate(ignoringOtherApps: true); settingsWindow?.makeKeyAndOrderFront(nil)
    }
}
