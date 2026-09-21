import SwiftUI
import PickleCore

@MainActor final class SessionStore: ObservableObject {
    @Published var reference: WebReference?
    var captureTarget: ScreenContextTarget?
    @Published var snapshot: SelectionSnapshot?
    @Published var result: ReadingResult?
    @Published var conversation: [ConversationTurn] = []
    @Published var context = ""
    @Published var page: CapturedPage?
    @Published var visualSummary = ""
    func clear() { reference = nil; captureTarget = nil; snapshot = nil; result = nil; conversation = []; context = ""; page = nil; visualSummary = "" }
}
@MainActor final class RequestCoordinator: ObservableObject {
    @Published var progress: String?
    @Published var draft = ""
    @Published var listening = false
    private var audioTask: Task<Void, Never>?
    private var audioService: AudioContextService?
    private var audioGeneration = UUID()
    @Published var referenceLoading = false
    @Published var referenceStatus: String?
    private var referenceTask: Task<Void, Never>?
    private var referenceTimeout: Task<Void, Never>?
    private var referenceGeneration = UUID()
    @Published var pageLoading = false
    @Published var pageStatus: String?
    @Published var visualBusy = false
    private var pageTask: Task<Void, Never>?
    private var queuedPageAction: (() -> Void)?
    private var pageTimeout: Task<Void, Never>?
    private var visualTask: Task<Void, Never>?
    private var pageGeneration = UUID()
    private var visualGeneration = UUID()
    @Published var error: String?
    @Published var needsContext: [String]?
    @Published var needsChart = false
    @Published var needsDisclosure = false
    @Published var pendingAction: ReadingAction = .simplify
    @Published var question = ""
    @Published var manualText = ""
    @Published var captureMessage: String?
    @Published var isSample = false
    @Published var jevStatus = "Not tested"
    let session: SessionStore, settings: SettingsStore
    private let runner = RequestRunner()
    private let referenceFetcher: @MainActor (String, String) async throws -> WebReference
    let metrics = ContentFreeMetrics()
    private var lastQuestion = "", lastLimited = false
    private var lastChart: ChartKind?
    private var credential: String?
    init(session: SessionStore, settings: SettingsStore, referenceFetcher: @escaping @MainActor (String, String) async throws -> WebReference = { try await WebReferenceService.fetch($0, selection: $1) }) {
        self.session = session; self.settings = settings; self.referenceFetcher = referenceFetcher
    }
    func cancel() { stopListening(); queuedPageAction = nil; visualGeneration = UUID(); visualTask?.cancel(); visualTask = nil; visualBusy = false; if progress != nil { Task { await metrics.record(.cancelled) } }; runner.cancel(); progress = nil; draft = ""; credential = nil }
    func clear() { cancel(); cancelReference(); session.reference = nil; referenceStatus = nil; pageGeneration = UUID(); pageTask?.cancel(); pageTask = nil; pageTimeout?.cancel(); pageTimeout = nil; pageLoading = false; pageStatus = nil; session.clear(); manualText = ""; question = ""; captureMessage = nil; error = nil; needsContext = nil; needsChart = false; needsDisclosure = false; isSample = false; credential = nil; lastQuestion = ""; lastLimited = false; lastChart = nil; pendingAction = .simplify }
    func setSelection(_ snapshot: SelectionSnapshot) { clear(); session.snapshot = snapshot; captureReference() }
    func capturePage(_ target: ScreenContextTarget?) {
        session.captureTarget = target
        guard settings.screenContextEnabled, let snapshot = session.snapshot, snapshot.bundleID != "sample", snapshot.bundleID != "manual", !settings.isExcluded(snapshot.bundleID) else { return }
        pageGeneration = UUID(); let generation = pageGeneration
        pageTask?.cancel(); pageTask = nil; pageTimeout?.cancel(); pageTimeout = nil; pageLoading = false
        session.page = nil; session.visualSummary = ""
        guard let target, target.bundleID == snapshot.bundleID else { pageStatus = "Page context unavailable for this window."; return }
        guard ScreenContextService.permitted else { pageStatus = "Allow Screen Recording in Settings to include page context."; return }
        pageLoading = true; pageStatus = "Reading page on this Mac…"
        pageTask = Task { [weak self] in
            do {
                let page = try await ScreenContextService.capture(target)
                guard let self, !Task.isCancelled, self.pageGeneration == generation, self.session.snapshot?.id == snapshot.id,
                      self.settings.screenContextEnabled, !self.settings.isExcluded(snapshot.bundleID) else { return }
                self.session.page = page; self.pageLoading = false
                self.pageStatus = page.text.isEmpty ? "Screenshot ready · no readable text" : "Page text included"
                self.finishPageCapture()
            } catch {
                guard let self, !Task.isCancelled, self.pageGeneration == generation else { return }
                self.pageLoading = false
                self.pageStatus = (error as? PickleError)?.localizedDescription ?? "Couldn’t read this window. Your selection is still available."
                self.finishPageCapture()
            }
        }
        pageTimeout = Task { [weak self] in
            try? await Task.sleep(for: .seconds(5))
            guard let self, !Task.isCancelled, self.pageGeneration == generation, self.pageLoading else { return }
            self.pageGeneration = UUID(); self.pageTask?.cancel(); self.pageTask = nil
            self.pageStatus = "Page context took too long. Continuing with your selection."
            self.finishPageCapture()
        }
    }
    private func finishPageCapture() {
        pageLoading = false; pageTimeout?.cancel(); pageTimeout = nil
        guard !referenceLoading else { return }
        let next = queuedPageAction; queuedPageAction = nil
        next?()
    }
    func removePage() {
        cancel(); pageGeneration = UUID(); pageTask?.cancel(); pageTask = nil
        pageTimeout?.cancel(); pageTimeout = nil
        pageLoading = false; pageStatus = nil; session.page = nil; session.visualSummary = ""
    }
    func analyzeVisuals() {
        guard !settings.paused, !settings.localOnly, settings.screenContextEnabled,
              progress == nil, !visualBusy, let snapshot = session.snapshot, !settings.isExcluded(snapshot.bundleID),
              let page = session.page, session.visualSummary.isEmpty else { return }
        // The upload is explicit in the preview button; no image is sent by ordinary actions.
        let token: String
        do { token = try CredentialStore.read() } catch { self.error = error.localizedDescription; return }
        guard !token.isEmpty else { error = "Connect your account in Settings first."; return }
        visualGeneration = UUID(); let generation = visualGeneration
        visualBusy = true; error = nil
        visualTask = Task { [weak self] in
            do {
                let summary = try await VisualContextClient(accountID: self?.settings.accountID ?? "", token: token).summarize(jpeg: page.jpeg, selection: snapshot.text)
                guard let self, !Task.isCancelled, self.visualGeneration == generation, self.session.page?.id == page.id, self.session.snapshot?.id == snapshot.id else { return }
                self.session.visualSummary = summary; self.visualBusy = false
            } catch {
                guard let self, !Task.isCancelled, self.visualGeneration == generation else { return }
                self.visualBusy = false
                self.error = "Couldn’t analyze the image. Check that the vision model is enabled in your Cloudflare account. Page text remains available."
            }
        }
    }
    private func cancelReference() {
        referenceGeneration = UUID(); referenceTask?.cancel(); referenceTask = nil
        referenceTimeout?.cancel(); referenceTimeout = nil; referenceLoading = false
    }
    func skipReference() {
        let next = queuedPageAction
        removeReference()
        if pageLoading { queuedPageAction = next; if next != nil { progress = "Reading page…" } }
        else { next?() }
    }
    func skipPage() {
        let next = queuedPageAction
        removePage()
        if referenceLoading { queuedPageAction = next; if next != nil { progress = "Reading page…" } }
        else { next?() }
    }
    func removeReference() { cancel(); cancelReference(); session.reference = nil; referenceStatus = nil }
    func captureReference() {
        guard settings.webContextEnabled, !settings.localOnly, !settings.paused,
              let snapshot = session.snapshot, !settings.isExcluded(snapshot.bundleID),
              let url = snapshot.sourceURL else { return }
        cancelReference(); let generation = referenceGeneration
        referenceLoading = true; referenceStatus = "Reading page reference…"
        referenceTask = Task { [weak self] in
            do {
                let reference = try await self?.referenceFetcher(url, snapshot.text)
                guard let self, !Task.isCancelled, self.referenceGeneration == generation, self.session.snapshot?.id == snapshot.id else { return }
                self.session.reference = reference; self.referenceStatus = nil
                self.finishReference()
            } catch {
                guard let self, !Task.isCancelled, self.referenceGeneration == generation else { return }
                self.referenceStatus = "Page unavailable. Use the browser extension for signed-in pages and videos."
                self.finishReference()
            }
        }
        referenceTimeout = Task { [weak self] in
            try? await Task.sleep(for: .seconds(5))
            guard let self, !Task.isCancelled, self.referenceGeneration == generation, self.referenceLoading else { return }
            self.cancelReference(); self.referenceStatus = "Page reference timed out. Your selection is still available."
            self.finishReference()
        }
    }
    private func finishReference() {
        referenceLoading = false; referenceTimeout?.cancel(); referenceTimeout = nil
        if !pageLoading { finishPageCapture() }
    }
    func receiveReference(_ reference: WebReference, selection: String, bundleID: String) throws {
        guard !settings.paused, settings.webContextEnabled, !settings.isExcluded(bundleID) else { throw PickleError.message("Enable browser context in Pickle Settings, or resume Pickle.") }
        try reference.validate()
        guard selection.utf8.count <= Limits.selection else { throw PickleError.message("Select a shorter passage.") }
        clear()
        session.snapshot = SelectionSnapshot(text: selection, appName: "Browser", bundleID: bundleID, method: "Browser extension", sourceTitle: reference.title, sourceURL: reference.url)
        session.reference = reference
        session.captureTarget = ScreenContextService.target(for: session.snapshot!)
    }
    func stopListening() {
        audioGeneration = UUID(); audioTask?.cancel(); audioTask = nil
        audioService?.stop(); audioService = nil; listening = false
    }
    func listen() {
        guard !settings.paused, settings.webContextEnabled, progress == nil, !referenceLoading, !pageLoading,
              !visualBusy, !listening, let snapshot = session.snapshot, let url = snapshot.sourceURL,
              let target = session.captureTarget, !settings.isExcluded(snapshot.bundleID) else { return }
        let service = AudioContextService(); audioService = service
        audioGeneration = UUID(); let generation = audioGeneration
        listening = true; error = nil
        audioTask = Task { [weak self] in
            do {
                let text = try await service.record(target)
                guard let self, !Task.isCancelled, self.audioGeneration == generation, self.session.snapshot?.id == snapshot.id else { return }
                self.session.reference = WebReference(url: url, title: "Recorded 30-second excerpt", text: text, kind: "recorded audio")
                self.listening = false; self.audioService = nil; self.audioTask = nil
            } catch {
                guard let self, !Task.isCancelled, self.audioGeneration == generation else { return }
                self.listening = false; self.audioService = nil; self.audioTask = nil
                self.error = (error as? PickleError)?.localizedDescription ?? "Couldn’t transcribe this audio. Try captions or paste a transcript."
            }
        }
    }
    func pasteSelection() {
        let text = manualText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, text.utf8.count <= Limits.selection else { error = "This passage is a little long. Try a shorter excerpt."; return }
        setSelection(.init(text: text, appName: "Manual paste", bundleID: "manual", method: "Manual paste", limitations: ["App and source coordinates are unavailable for manual input."]))
    }
    func sample() {
        setSelection(.init(text: "The treatment may reduce symptoms in some patients, but the evidence remains limited.", appName: "Pickle sample", bundleID: "sample", method: "Bundled sample")); isSample = true; run(.simplify)
    }
    func adjust(_ instruction: String) {
        run(.followUp, followUp: instruction)
    }
    func explainTerm(_ term: String) {
        let term = term.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !term.isEmpty, term.count <= 150 else { error = "Choose a word or short phrase to explain."; return }
        guard let snapshot = session.snapshot else { return }
        let available = snapshot.text + "\n" + (session.result?.text ?? "") + "\n" + session.conversation.map(\.answer).joined(separator: "\n")
        guard available.localizedCaseInsensitiveContains(term) else { error = "Choose a word or phrase from the passage or answer."; return }
        run(.followUp, followUp: "Explain the meaning of ‘\(term)’ in this passage, using the surrounding context. Keep it brief; distinguish the contextual meaning from other meanings.")
    }
    func retry() { run(pendingAction, limited: lastLimited, chart: lastChart, followUp: lastQuestion) }
    func run(_ action: ReadingAction, limited: Bool = false, chart: ChartKind? = nil, followUp: String = "") {
        guard !listening else { error = "Finish listening or cancel the recording first."; return }
        guard !visualBusy else { error = "Wait for visual context, or remove it to continue."; return }
        cancel(); error = nil; needsContext = nil; needsChart = false
        guard !settings.paused else { error = "Pickle is paused. Resume from the menu bar."; return }
        guard let snapshot = session.snapshot else { error = "Select or paste a passage first."; return }
        guard !settings.isExcluded(snapshot.bundleID) else { error = "This source application is excluded."; return }
        if action == .followUp && session.conversation.count >= Limits.turns { error = "This conversation has reached six follow-ups. Clear follow-ups to keep reading with the same selection."; return }
        pendingAction = action; lastLimited = limited; lastChart = chart; lastQuestion = followUp
        if pageLoading || referenceLoading {
            progress = "Reading page…"
            let id = snapshot.id
            queuedPageAction = { [weak self] in
                guard let self, self.session.snapshot?.id == id else { return }
                self.run(action, limited: limited, chart: chart, followUp: followUp)
            }
            return
        }
        if !isSample {
            guard !settings.localOnly else { error = "Online explanations are turned off. Try an example, or enable them in Settings."; return }
            guard settings.cloudConsent, !settings.jevEnabled || settings.jevConsent, session.page == nil || settings.screenContextConsent, session.reference == nil || settings.webContextConsent else { needsDisclosure = true; return }
        }
        let token: String
        do { token = isSample ? "" : try CredentialStore.read(); credential = token }
        catch { self.error = error.localizedDescription; return }
        guard isSample || !token.isEmpty else { error = "Add your Cloudflare API token in Settings."; return }
        let input = RequestInput(snapshot: snapshot, action: action, context: session.context, level: settings.level, limited: limited, chartChoice: chart, question: followUp, previousResult: action == .followUp ? (session.conversation.last?.answer ?? session.result?.text ?? "") : "", conversation: action == .followUp ? session.conversation : [], pageContext: settings.screenContextEnabled ? session.page?.text ?? "" : "", visualContext: settings.screenContextEnabled ? session.visualSummary : "", reference: session.reference)
        do { try input.validate() } catch { self.error = error.localizedDescription; return }
        let provider: any GenerativeProvider = isSample ? SampleProvider() : CloudflareProvider(accountID: settings.accountID, token: token, model: settings.model)
        let evaluator: (any DecisionClient)? = settings.jevEnabled && !isSample ? JevClient(accountID: settings.accountID, token: token) : nil
        let metrics = self.metrics
        let pipeline = RequestPipeline(provider: provider, evaluator: evaluator, localOnly: settings.localOnly || isSample, metrics: { event in Task { await metrics.record(event) } })
        progress = "Preparing…"
        let draftHandler: (@MainActor (String) -> Void)?
        if settings.streaming { draftHandler = { [weak self] text in self?.draft = text } }
        else { draftHandler = nil }
        runner.start(pipeline: pipeline, input: input, draft: draftHandler, progress: { [weak self] message in self?.progress = message }) { [weak self] outcome in
            guard let self else { return }
            self.progress = nil; self.draft = ""; self.credential = nil
            switch outcome {
            case .success(let outcome):
                switch outcome {
                case .needsContext(let flags): self.needsContext = flags
                case .chooseChart: self.needsChart = true
                case .result(let result):
                    if action == .followUp {
                        self.session.conversation.append(.init(question: followUp, answer: result.text, quality: result.quality)); self.question = ""
                        while self.session.conversation.reduce(0, { $0 + $1.question.utf8.count + $1.answer.utf8.count }) > Limits.conversationBytes {
                            self.session.conversation.removeFirst()
                        }
                    } else { self.session.result = result; self.session.conversation = [] }
                    if let model = result.evaluatorModel { self.jevStatus = "Last response: \(model) via Cloudflare" }
                }
            case .failure(let error):
                self.error = (error as? PickleError)?.localizedDescription ?? "The request could not complete. Check your connection and retry."
                Task { await metrics.record(.failure) }
            }
        }
    }
    func policyChanged() {
        // Any settings edit invalidates an in-flight request; no stale consent or provider configuration.
        if progress != nil { cancel(); error = "Settings changed. Run the action again with your updated preferences." }
        if visualBusy || listening { cancel() }
        if !settings.screenContextEnabled || settings.paused || session.snapshot.map({ settings.isExcluded($0.bundleID) }) == true { removePage() }
        if !settings.webContextEnabled || settings.localOnly || settings.paused || session.snapshot.map({ settings.isExcluded($0.bundleID) }) == true { removeReference() }
        if settings.paused { needsDisclosure = false }
    }
}
