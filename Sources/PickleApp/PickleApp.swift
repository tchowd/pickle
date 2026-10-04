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
    lazy var bookmarks = BookmarkStore(defaults: settings.defaults)
    // Native checks and previews use a throwaway file, like their throwaway preferences domain.
    lazy var knowledge = KnowledgeBase(file: CommandLine.arguments.contains("--smoke-test") || CommandLine.arguments.contains("--preview-actions")
        ? FileManager.default.temporaryDirectory.appendingPathComponent("pickle-smoke-" + UUID().uuidString + "/entries.json") : KnowledgeBase.defaultFile)
    private var savedWindow: NSWindow?, knowledgeWindow: NSWindow?
    private var positioning = false
    lazy var coordinator = RequestCoordinator(session: session, settings: settings, knowledge: knowledge)
    private var statusItem: NSStatusItem!, resultPanel: ReaderPanel?, actionPanel: ActionPanel?, settingsWindow: NSWindow?
    private var browserBridge: BrowserBridge?
    private var invocation: InvocationController?
    private var escapeMonitor: Any?
    private var actionExpanded = false
    private var floatingOrigin: NSPoint?
    @Published var pinned = false
    func applicationDidFinishLaunching(_ notification: Notification) {
        if let index = CommandLine.arguments.firstIndex(of: "--import-local-server"), CommandLine.arguments.count > index + 1 {
            let address = CommandLine.arguments[index + 1]
            let token = String(data: FileHandle.standardInput.readDataToEndOfFile(), encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            do {
                try LocalAIConfiguration(address: address).validate()
                try CredentialStore.save(token, account: "local-server:" + address)
                settings.localAddress = address; settings.localModel = "llama3.2:1b"; settings.localSameDevice = false
                print("Local server saved; Online mode and credentials preserved."); exit(0)
            } catch { print("Could not save Local server settings."); exit(1) }
        }
        if CommandLine.arguments.contains("--local-connection-check") {
            Task {
                do {
                    try coordinator.localConfiguration().validate(offline: settings.localOnly)
                    let token = try CredentialStore.read(account: settings.localCredentialAccount)
                    let provider = OllamaProvider(configuration: coordinator.localConfiguration(token: token))
                    let input = RequestInput(snapshot: .init(text: "The study suggests a possible benefit, but more evidence is needed.", appName: "Fixed Local fixture", bundleID: "fixture"), action: .simplify)
                    let outcome = try await RequestPipeline(provider: provider, localOnly: settings.localOnly).run(input)
                    guard case .result(let result) = outcome else { throw PickleError.message("No Local answer returned.") }
                    print("PICKLE_LOCAL_CONNECTION_PASS: Keychain configuration, local generation, \(result.model), \(result.elapsed)s. Saved mode: \(settings.aiMode.rawValue)")
                    exit(0)
                } catch { print("PICKLE_LOCAL_CONNECTION_FAIL: \(error.localizedDescription)"); exit(1) }
            }
            return
        }
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
        if !CommandLine.arguments.contains("--smoke-test") && !CommandLine.arguments.contains("--preview-actions") {
            browserBridge = BrowserBridge { [weak self] message in
                guard let self else { return }
                try self.coordinator.receiveReference(message.reference, selection: message.selection, bundleID: message.bundleID)
                self.coordinator.capturePage(self.session.captureTarget)
                self.actionPanel?.orderOut(nil); self.actionExpanded = false; self.presentReader()
            }
            try? browserBridge?.start()
        }
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
                let features = await self.checkReadingFeatures()
                let pageFeatures = await self.checkPageContextFeatures()
                let webFeatures = await self.checkWebReferenceFeatures()
                let bridgeFeatures = await self.checkBrowserBridge()
                let localFeatures = self.checkLocalModePreferences()
                let knowledgeFeatures = await self.checkKnowledgeBase()
                let ok = generated && hidden && reopened && chooser && chosen && anchored && stillVisible && inlineReopened && clamped && remembered && freshSelection && freshEmpty && features && pageFeatures && webFeatures && bridgeFeatures && localFeatures && knowledgeFeatures
                print(ok ? "PICKLE_SMOKE_PASS: movable action bar, anchored expansion, screen-edge clamping, remembered position, reopen, fresh sessions, saved answers, appearance persistence, and reading actions" : "PICKLE_SMOKE_FAIL")
                if !ok { exit(1) }
                NSApp.terminate(nil)
            }
        } else if CommandLine.arguments.contains("--preview-actions") {
            showActions(.init(text: "The treatment may reduce symptoms in some patients, but the evidence remains limited.", appName: "Pickle sample", bundleID: "sample"), automatic: false)
        } else { presentReader() }
    }
    /// Exercises the real commit site with a fixture writer; no network, Keychain item, or user file is touched.
    private func checkKnowledgeBase() async -> Bool {
        let suite = "com.pickle.knowledge-check." + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("pickle-knowledge-check-" + UUID().uuidString)
        defer { defaults.removePersistentDomain(forName: suite); try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("entries.json")
        let preferences = SettingsStore(defaults: defaults)
        preferences.aiMode = .local; preferences.localAddress = "http://127.0.0.1:11999"; preferences.localSameDevice = true
        preferences.localConsentServer = preferences.localConsentID; preferences.webContextEnabled = true
        let store = KnowledgeBase(file: file), writer = KnowledgeCheckWriter(), session = SessionStore()
        let coordinator = RequestCoordinator(session: session, settings: preferences, knowledge: store, writer: writer,
                                             referenceFetcher: { _, _ in throw PickleError.message("Fixture makes no network requests") })
        func settle() async { try? await Task.sleep(for: .milliseconds(200)) }
        func until(_ condition: () -> Bool) async -> Bool {
            for _ in 0..<60 { if condition() { return true }; try? await Task.sleep(for: .milliseconds(50)) }
            return condition()
        }
        func select() {
            // No source URL, so no reference fetch starts; the attached reference is set directly.
            coordinator.setSelection(.init(text: "Cells release energy.", appName: "Fixture browser", bundleID: "fixture.browser"))
            session.reference = WebReference(url: "https://example.com/cells", title: "Cell energy", text: "REFERENCE BODY TEXT")
            session.page = CapturedPage(jpeg: Data("JPEG PIXELS".utf8), text: "OCR PAGE TEXT", capturedAt: Date(), truncated: false)
            session.visualSummary = "VISUAL SUMMARY TEXT"
        }
        select(); coordinator.run(.simplify); await settle()
        let offByDefault = !preferences.knowledgeBaseEnabled && session.result != nil && store.entries.isEmpty && !FileManager.default.fileExists(atPath: file.path)
        preferences.knowledgeBaseEnabled = true
        coordinator.run(.simplify); _ = await until { store.entries.count == 1 }
        let first = store.entries.first
        let recorded = store.entries.count == 1 && first?.action == .simplify && first?.question == nil && first?.passage == "Cells release energy."
            && first?.answer == session.result?.text && first?.reference == .init(url: "https://example.com/cells", title: "Cell energy", kind: "article")
        coordinator.run(.followUp, followUp: "Why does it matter?"); _ = await until { store.entries.count == 2 }
        coordinator.explainTerm("energy"); _ = await until { store.entries.count == 3 }
        let explainHeading = session.conversation.last?.label == "Explain ‘energy’"
        coordinator.adjust("Rewrite the current explanation more briefly.", label: "Shorter"); _ = await until { store.entries.count == 4 }
        let separate = store.entries.map(\.action) == [.adjustment, .explain, .followUp, .simplify]
            && store.entries.map(\.question) == ["Shorter", "energy", "Why does it matter?", nil]
        let headings = explainHeading && session.conversation.last?.label == "Shorter" && session.conversation.first?.label == nil
        // Retrying a failed explain, and an adjustment queued behind page reading, keep what the user asked.
        await writer.failNextCall()
        coordinator.explainTerm("cells"); _ = await until { coordinator.error != nil }
        coordinator.retry(); _ = await until { store.entries.count == 5 }
        coordinator.pageLoading = true
        coordinator.adjust("Give a short example.", label: "Give an example")
        let queued = coordinator.progress != nil && store.entries.count == 5
        coordinator.skipPage(); _ = await until { store.entries.count == 6 }
        let carried = queued && store.entries.prefix(2).map(\.action) == [.adjustment, .explain]
            && store.entries.prefix(2).map(\.question) == ["Give an example", "cells"]
        let raw = (try? String(contentsOf: file, encoding: .utf8)) ?? ""
        let excluded = !raw.isEmpty && !["REFERENCE BODY TEXT", "OCR PAGE TEXT", "VISUAL SUMMARY TEXT", "JPEG PIXELS", Data("JPEG PIXELS".utf8).base64EncodedString(),
                                          "Rewrite the current", "Explain the meaning", "fixture-model"].contains { raw.contains($0) }
        let count = store.entries.count
        // Cancelled, cleared and replaced sessions never append, even when the writer ignores cancellation.
        coordinator.run(.expand); coordinator.cancel(); await settle()
        coordinator.run(.expand); coordinator.clear(); await settle()
        select(); coordinator.run(.expand); select(); await settle()
        let staleIgnored = store.entries.count == count && session.result == nil
        // Turning the switch off stops new writes, including for a request already in flight.
        coordinator.run(.expand); preferences.knowledgeBaseEnabled = false; await settle()
        coordinator.run(.expand); await settle()
        let switchedOff = store.entries.count == count && session.result != nil
        preferences.knowledgeBaseEnabled = true
        coordinator.sample(); _ = await until { session.result != nil }; await settle()
        let sampleSkipped = store.entries.count == count && session.result != nil
        // Clearing keeps the log, and the log never enters a later request.
        coordinator.clear()
        let kept = KnowledgeBase(file: file).entries.count == count
        coordinator.setSelection(.init(text: "A new passage.", appName: "Fixture", bundleID: "fixture"))
        coordinator.run(.simplify); _ = await until { session.result != nil }
        let earlier = store.entries.filter { $0.passage == "Cells release energy." }.map(\.answer)
        let notInjected = await writer.prompts.last.map { prompt in !earlier.contains { prompt.system.contains($0) || prompt.user.contains($0) } } ?? false
        let checks = [offByDefault, recorded, separate, headings, carried, excluded, staleIgnored, switchedOff, sampleSkipped, kept, notInjected]
        let ok = !checks.contains(false)
        print(ok ? "PICKLE_KNOWLEDGE_BASE_PASS: off by default, one record per answer, follow-up/explain/adjustment records and headings, retry and queued actions, pointer-only references, stale/cancelled/sample skipped, switch off, kept after clear, not injected"
                 : "PICKLE_KNOWLEDGE_BASE_FAIL: \(checks)")
        return ok
    }
    private func checkLocalModePreferences() -> Bool {
        let suite = "com.pickle.local-mode-check." + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        defaults.set(true, forKey: "localOnly")
        defaults.set("@cf/meta/llama-3.2-1b-instruct", forKey: "model")
        defaults.set(true, forKey: "jevEnabled")
        let preferences = SettingsStore(defaults: defaults)
        let migrated = preferences.aiMode == .local && preferences.localOnly && preferences.localModel == "llama3.2:1b"
        preferences.aiMode = .online
        let coordinator = RequestCoordinator(session: SessionStore(), settings: preferences)
        coordinator.setSelection(.init(text: "Selected passage", appName: "Fixture", bundleID: "fixture"))
        coordinator.session.context = "Keep this context"
        coordinator.progress = "Reviewing…"; coordinator.draft = "Old draft"
        preferences.aiMode = .local; coordinator.policyChanged()
        let cancelled = coordinator.progress == nil && coordinator.draft.isEmpty && coordinator.session.snapshot?.text == "Selected passage" && coordinator.session.context == "Keep this context" && coordinator.session.result == nil
        coordinator.run(.simplify)
        let blocked = coordinator.error?.contains("Offline") == true
        preferences.aiMode = .online; coordinator.policyChanged()
        let restored = SettingsStore(defaults: defaults)
        let preserved = restored.localOnly && restored.jevEnabled && restored.model == "@cf/meta/llama-3.2-1b-instruct" && restored.aiMode == .online
        preferences.aiMode = .local
        let persisted = SettingsStore(defaults: defaults).aiMode == .local
        let ok = migrated && cancelled && blocked && preserved && persisted
        print(ok ? "PICKLE_LOCAL_MODE_PASS: migration, mode cancellation, retained context, offline tunnel gate, Online preference preservation" : "PICKLE_LOCAL_MODE_FAIL")
        return ok
    }
    private func checkBrowserBridge() async -> Bool {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("pickle-bridge-test-" + UUID().uuidString)
        var accepted = 0
        let bridge = BrowserBridge(directory: directory) { _ in accepted += 1 }
        defer { bridge.stop(); try? FileManager.default.removeItem(at: directory) }
        do {
            try bridge.start()
            let config = directory.appendingPathComponent("connection.json")
            for _ in 0..<20 {
                if FileManager.default.fileExists(atPath: config.path) { break }
                try await Task.sleep(for: .milliseconds(50))
            }
            let path = FileManager.default.currentDirectoryPath + "/Tests/BrowserExtensionTests/bridge_probe.py"
            let success = await Task.detached {
                let process = Process()
                process.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
                process.arguments = [path, config.path]
                do { try process.run(); process.waitUntilExit(); return process.terminationStatus == 0 } catch { return false }
            }.value
            let ok = success && accepted == 1
            print(ok ? "PICKLE_BROWSER_BRIDGE_PASS" : "PICKLE_BROWSER_BRIDGE_FAIL")
            return ok
        } catch { print("PICKLE_BROWSER_BRIDGE_FAIL"); return false }
    }
    private func checkWebReferenceFeatures() async -> Bool {
        do {
            if ProcessInfo.processInfo.environment["PICKLE_TEST_PUBLIC_PAGE"] == "1" {
                let reference = try await WebReferenceService.fetch("https://example.com", selection: "")
                guard !reference.text.isEmpty, reference.title.contains("Example Domain") else { return false }
                print("PICKLE_PUBLIC_FETCH_PASS: public HTTPS fetch, pinned public address, native article extraction")
            }
            let parser = WebReferenceService()
            let paragraph = "Mitochondria help cells release energy. This article explains the process and its limitations. "
            let fixture = "<html><head><title>Energy article</title></head><body><nav>Unrelated navigation</nav><article><h1>Energy article</h1><p>" + String(repeating: paragraph, count: 12) + "</p></article><script>document.body.textContent='SCRIPT EXECUTED';</script></body></html>"
            let extracted = try await parser.extract(fixture, url: URL(string: "https://example.com/article")!)
            let text = extracted["text"] ?? ""
            let parsed = text.contains("Mitochondria") && !text.contains("SCRIPT EXECUTED") && !text.contains("Unrelated navigation")
            let suite = "com.pickle.web-check." + UUID().uuidString
            let defaults = UserDefaults(suiteName: suite)!
            defer { defaults.removePersistentDomain(forName: suite) }
            let settings = SettingsStore(defaults: defaults); settings.webContextEnabled = true
            let session = SessionStore(), coordinator: RequestCoordinator
            coordinator = RequestCoordinator(session: session, settings: settings)
            let reference = WebReference(url: "https://example.com/article", title: "Energy", text: "Mitochondria release energy.")
            try coordinator.receiveReference(reference, selection: "", bundleID: "com.google.Chrome")
            coordinator.run(.simplify)
            let disclosure = coordinator.needsDisclosure && session.reference == reference
            coordinator.clear()
            let cleared = session.reference == nil && !coordinator.referenceLoading
            try coordinator.receiveReference(reference, selection: "", bundleID: "com.google.Chrome")
            settings.webContextEnabled = false; coordinator.policyChanged()
            let disabled = session.reference == nil
            let localBlocked = await Task.detached { WebReferenceService.publicAddress("localhost") == nil }.value
            settings.webContextEnabled = true
            let delayed = RequestCoordinator(session: session, settings: settings, referenceFetcher: { _, _ in
                // Ignore cancellation deliberately, reproducing a late source response.
                await withCheckedContinuation { continuation in
                    DispatchQueue.global().asyncAfter(deadline: .now() + 0.08) { continuation.resume() }
                }
                return reference
            })
            delayed.setSelection(.init(text: "A selection", appName: "Browser", bundleID: "com.google.Chrome", sourceURL: reference.url))
            delayed.run(.simplify)
            let queued = delayed.progress != nil && delayed.referenceLoading
            delayed.clear()
            try await Task.sleep(for: .milliseconds(120))
            let staleIgnored = session.reference == nil && session.snapshot == nil && delayed.progress == nil && !delayed.needsDisclosure
            let ok = parsed && disclosure && cleared && disabled && localBlocked && queued && staleIgnored
            print(ok ? "PICKLE_WEB_REFERENCE_PASS: isolated HTML extraction, page-only consent, cleanup, local-address rejection" : "PICKLE_WEB_REFERENCE_FAIL")
            return ok
        } catch { print("PICKLE_WEB_REFERENCE_FAIL: \(error.localizedDescription)"); return false }
    }
    private func checkPageContextFeatures() async -> Bool {
        // Synthetic pixels only: this check never captures the desktop or sends a request.
        let image = NSImage(size: NSSize(width: 900, height: 300), flipped: false) { rect in
            NSColor.white.setFill(); rect.fill()
            ("Pickle page context" as NSString).draw(at: NSPoint(x: 40, y: 140), withAttributes: [
                .font: NSFont.systemFont(ofSize: 36), .foregroundColor: NSColor.black
            ])
            return true
        }
        guard let pixels = image.cgImage(forProposedRect: nil, context: nil, hints: nil),
              let page = try? await Task.detached(operation: { try ScreenContextService.process(pixels) }).value else { return false }
        let ocr = page.text.contains("Pickle page context") && page.jpeg.count <= PageContextLimits.imageBytes
        let suite = "com.pickle.page-check." + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let preferences = SettingsStore(defaults: defaults)
        let testSession = SessionStore()
        let coordinator = RequestCoordinator(session: testSession, settings: preferences)
        let selection = SelectionSnapshot(text: "A passage", appName: "Fixture", bundleID: "fixture")
        coordinator.setSelection(selection)
        testSession.page = page; testSession.visualSummary = "Visual context"
        coordinator.run(.simplify)
        let disclosure = coordinator.needsDisclosure
        coordinator.cancel()
        let retained = testSession.page?.id == page.id
        coordinator.removePage()
        let removed = testSession.page == nil && testSession.visualSummary.isEmpty
        testSession.page = page; testSession.visualSummary = "Visual context"
        coordinator.setSelection(selection)
        let fresh = testSession.page == nil && testSession.visualSummary.isEmpty
        testSession.page = page; testSession.visualSummary = "Visual context"
        preferences.screenContextEnabled = false; coordinator.policyChanged()
        let disabled = testSession.page == nil && testSession.visualSummary.isEmpty
        let ok = ocr && disclosure && retained && removed && fresh && disabled
        print(ok ? "PICKLE_PAGE_CONTEXT_PASS: synthetic OCR, bounded JPEG, consent, session reuse, removal, fresh session, disabled cleanup" : "PICKLE_PAGE_CONTEXT_FAIL")
        return ok
    }
    private func checkReadingFeatures() async -> Bool {
        let suite = "com.pickle.feature-check." + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let preferences = SettingsStore(defaults: defaults)
        preferences.textSize = 22; preferences.glassOpacity = 0.7; preferences.themeIntensity = 0.2
        preferences.expandedSize = NSSize(width: 610, height: 710)
        preferences.floatingFrame = NSRect(x: 100, y: 100, width: 610, height: 710)
        let restored = SettingsStore(defaults: defaults)
        let appearance = restored.textSize == 22 && restored.glassOpacity == 0.7 && restored.themeIntensity == 0.2
            && restored.expandedSize == NSSize(width: 610, height: 710) && restored.floatingFrame == preferences.floatingFrame
        let library = BookmarkStore(defaults: defaults)
        library.save(passage: "A passage", answer: "An answer", source: "Sample")
        library.save(passage: "A passage", answer: "An answer", source: "Sample")
        let restoredLibrary = BookmarkStore(defaults: defaults)
        let saved = restoredLibrary.items.count == 1 && restoredLibrary.items.first?.answer == "An answer"
        if let item = restoredLibrary.items.first { restoredLibrary.remove(item.id) }
        let removed = BookmarkStore(defaults: defaults).items.isEmpty
        let testSession = SessionStore()
        let testCoordinator = RequestCoordinator(session: testSession, settings: preferences)
        testCoordinator.sample()
        try? await Task.sleep(for: .milliseconds(100))
        testCoordinator.adjust("Make this shorter.", label: "Shorter")
        try? await Task.sleep(for: .milliseconds(100))
        let adjusted = testSession.conversation.last?.question == "Make this shorter."
        testCoordinator.explainTerm("evidence")
        try? await Task.sleep(for: .milliseconds(100))
        let explained = testSession.conversation.last?.question.contains("‘evidence’") == true
        testCoordinator.explainTerm("not in the passage")
        let invalidTerm = testCoordinator.error != nil
        testCoordinator.clear()
        let cleared = testSession.conversation.isEmpty && testSession.snapshot == nil
        // Exercise native resize notifications, not only preference encoding.
        var resized = false
        if let panel = actionPanel {
            actionExpanded = true
            panel.setContentSize(NSSize(width: 600, height: 650))
            rememberWindow(Notification(name: NSWindow.didResizeNotification, object: panel))
            let reloaded = SettingsStore(defaults: settings.defaults)
            resized = reloaded.expandedSize == panel.frame.size && reloaded.floatingFrame == panel.frame
        }
        return appearance && saved && removed && adjusted && explained && invalidTerm && cleared && resized
    }
    func applicationWillTerminate(_ notification: Notification) { browserBridge?.stop() }
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
        add("Saved answers…", #selector(openSavedAnswers))
        add("Knowledge base…", #selector(openKnowledgeBase))
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
            if automatic && snapshot.text.isEmpty { return }
            showActions(snapshot, automatic: automatic, pageTarget: ScreenContextService.target(for: snapshot))
        } catch {
            guard !automatic else { return }
            actionPanel?.orderOut(nil)
            // A shortcut always starts fresh, even when no selection can be read.
            coordinator.captureMessage = error.localizedDescription; showResult()
        }
    }
    private func showActions(_ snapshot: SelectionSnapshot, automatic: Bool, pageTarget: ScreenContextTarget? = nil) {
        if let actionPanel { floatingOrigin = actionPanel.frame.origin }
        actionPanel?.orderOut(nil)
        actionExpanded = false
        if !automatic {
            coordinator.setSelection(snapshot)
            coordinator.manualText = snapshot.text
            coordinator.capturePage(pageTarget)
        }
        let panel = ActionPanel(contentRect: NSRect(x: 0, y: 0, width: 440, height: 124), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.title = "Pickle actions"
        panel.isMovable = true
        panel.delegate = self
        panel.level = pinned ? .floating : .normal; panel.isFloatingPanel = true; panel.hidesOnDeactivate = false; panel.isOpaque = false; panel.backgroundColor = .clear; panel.hasShadow = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        panel.contentView = NSHostingView(rootView: ActionMenu(snapshot: snapshot, choose: { [weak self] action in
            self?.chooseAction(action, snapshot: snapshot, pageTarget: pageTarget)
        }, dismiss: { [weak self] in self?.dismissFloater() }).pickleAppearance(settings))
        resultPanel?.orderOut(nil)
        let screen = NSScreen.screens.first { $0.frame.contains(NSEvent.mouseLocation) } ?? NSScreen.main
        positionFloater(panel, on: screen, expanded: false)
        // Keep the source app focused and its selection intact until an action is clicked.
        panel.orderFrontRegardless(); actionPanel = panel
    }
    private func chooseAction(_ action: ReadingAction, snapshot: SelectionSnapshot, pageTarget: ScreenContextTarget? = nil) {
        guard !settings.paused else { return }
        if session.snapshot?.id != snapshot.id { coordinator.setSelection(snapshot); coordinator.capturePage(pageTarget) }
        if CommandLine.arguments.contains("--smoke-test") || CommandLine.arguments.contains("--preview-actions") { coordinator.isSample = true }
        guard let panel = actionPanel else { return }
        positioning = true
        defer { positioning = false }
        actionExpanded = true
        panel.acceptsKeyboard = true
        panel.styleMask.insert(.resizable)
        panel.minSize = NSSize(width: 420, height: 420)
        panel.title = "Pickle"
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.contentView = NSHostingView(rootView: ReaderView(coordinator: coordinator, session: session, settings: settings, app: self, floating: true))
        positionFloater(panel, on: panel.screen, expanded: true)
        panel.makeKeyAndOrderFront(nil)
        coordinator.run(action)
    }
    private func positionFloater(_ panel: NSPanel, on screen: NSScreen?, expanded: Bool) {
        guard let frame = (screen ?? NSScreen.main)?.visibleFrame else { return }
        positioning = true
        defer { positioning = false }
        let preferred = settings.expandedSize ?? NSSize(width: 520, height: 640)
        let width = min(expanded ? max(420, preferred.width) : 440, frame.width - 32)
        let height = min(expanded ? max(420, preferred.height) : 124, frame.height - 48)
        let desired = expanded
            ? NSPoint(x: panel.frame.midX - width / 2, y: panel.frame.minY)
            : floatingOrigin ?? settings.floatingFrame?.origin ?? NSPoint(x: frame.midX - width / 2, y: frame.minY + 24)
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
            resultPanel = panel
            if let saved = settings.readerFrame { panel.setFrame(visibleFrame(saved), display: false) }
            else { panel.center() }
        }
        if let snapshot { position(resultPanel!, snapshot: snapshot) }
        resultPanel?.makeKeyAndOrderFront(nil)
    }
    private func visibleFrame(_ saved: NSRect) -> NSRect {
        let screen = NSScreen.screens.first { $0.visibleFrame.intersects(saved) } ?? NSScreen.main
        guard let bounds = screen?.visibleFrame else { return saved }
        let width = min(max(saved.width, 420), bounds.width - 32)
        let height = min(max(saved.height, 420), bounds.height - 32)
        return NSRect(x: max(bounds.minX + 16, min(saved.minX, bounds.maxX - width - 16)),
                      y: max(bounds.minY + 16, min(saved.minY, bounds.maxY - height - 16)), width: width, height: height)
    }
    func windowDidMove(_ notification: Notification) { rememberWindow(notification) }
    func windowDidResize(_ notification: Notification) { rememberWindow(notification) }
    private func rememberWindow(_ notification: Notification) {
        guard !positioning, let window = notification.object as? NSWindow else { return }
        if window === actionPanel {
            settings.floatingFrame = window.frame
            floatingOrigin = window.frame.origin
            if actionExpanded { settings.expandedSize = window.frame.size }
        } else if window === resultPanel { settings.readerFrame = window.frame }
    }
    @objc func openSavedAnswers() {
        if savedWindow == nil {
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 600, height: 700), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
            window.title = "Saved answers"; window.isReleasedWhenClosed = false
            window.isOpaque = false; window.backgroundColor = .clear; window.minSize = NSSize(width: 500, height: 440)
            window.contentView = NSHostingView(rootView: SavedAnswersView(store: bookmarks, settings: settings))
            window.center(); savedWindow = window
        }
        NSApp.activate(ignoringOtherApps: true); savedWindow?.makeKeyAndOrderFront(nil)
    }
    @objc func openKnowledgeBase() {
        if knowledgeWindow == nil {
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 720), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
            window.title = "Knowledge base"; window.isReleasedWhenClosed = false
            window.isOpaque = false; window.backgroundColor = .clear; window.minSize = NSSize(width: 520, height: 440)
            window.contentView = NSHostingView(rootView: KnowledgeBaseView(store: knowledge, settings: settings))
            window.center(); knowledgeWindow = window
        }
        NSApp.activate(ignoringOtherApps: true); knowledgeWindow?.makeKeyAndOrderFront(nil)
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
/// Deterministic writer for the knowledge-base check. It ignores cancellation to reproduce late responses.
private actor KnowledgeCheckWriter: GenerativeProvider {
    nonisolated let isRemote = false
    private(set) var prompts: [GenerationPrompt] = []
    private var failNext = false
    func failNextCall() { failNext = true }
    func generate(_ prompt: GenerationPrompt) async throws -> Generation {
        prompts.append(prompt)
        if failNext { failNext = false; throw PickleError.message("Fixture failure") }
        await withCheckedContinuation { continuation in DispatchQueue.global().asyncAfter(deadline: .now() + 0.05) { continuation.resume() } }
        return Generation(text: "Answer \(prompts.count).", model: "fixture-model")
    }
}
