import SwiftUI
import PickleCore

@MainActor final class SessionStore: ObservableObject {
    @Published var snapshot: SelectionSnapshot?
    @Published var result: ReadingResult?
    @Published var conversation: [ConversationTurn] = []
    @Published var context = ""
    func clear() { snapshot = nil; result = nil; conversation = []; context = "" }
}
@MainActor final class RequestCoordinator: ObservableObject {
    @Published var progress: String?
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
    let metrics = ContentFreeMetrics()
    private var lastQuestion = "", lastLimited = false
    private var lastChart: ChartKind?
    private var credential: String?
    init(session: SessionStore, settings: SettingsStore) { self.session = session; self.settings = settings }
    func cancel() { if progress != nil { Task { await metrics.record(.cancelled) } }; runner.cancel(); progress = nil; credential = nil }
    func clear() { cancel(); session.clear(); manualText = ""; question = ""; captureMessage = nil; error = nil; needsContext = nil; needsChart = false; needsDisclosure = false; isSample = false; credential = nil; lastQuestion = "" }
    func setSelection(_ snapshot: SelectionSnapshot) { clear(); session.snapshot = snapshot }
    func pasteSelection() {
        let text = manualText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, text.utf8.count <= Limits.selection else { error = "Paste a passage of at most \(Limits.selection.formatted()) UTF-8 bytes."; return }
        setSelection(.init(text: text, appName: "Manual paste", bundleID: "manual", method: "Manual paste", limitations: ["App and source coordinates are unavailable for manual input."]))
    }
    func sample() {
        setSelection(.init(text: "The treatment may reduce symptoms in some patients, but the evidence remains limited.", appName: "Pickle sample", bundleID: "sample", method: "Bundled sample")); isSample = true; run(.simplify)
    }
    func retry() { run(pendingAction, limited: lastLimited, chart: lastChart, followUp: lastQuestion) }
    func run(_ action: ReadingAction, limited: Bool = false, chart: ChartKind? = nil, followUp: String = "") {
        cancel(); error = nil; needsContext = nil; needsChart = false
        guard !settings.paused else { error = "Pickle is paused. Resume from the menu bar."; return }
        guard let snapshot = session.snapshot else { error = "Select or paste a passage first."; return }
        guard !settings.isExcluded(snapshot.bundleID) else { error = "This source application is excluded."; return }
        if action == .followUp && session.conversation.count >= Limits.turns { error = "This conversation has reached six follow-ups. Clear follow-ups to keep reading with the same selection."; return }
        pendingAction = action; lastLimited = limited; lastChart = chart; lastQuestion = followUp
        if !isSample {
            guard !settings.localOnly else { error = "Cloudflare is remote and is blocked in local-only mode. The offline sample is available."; return }
            guard settings.cloudConsent, !settings.jevEnabled || settings.jevConsent else { needsDisclosure = true; return }
        }
        let token: String
        do { token = isSample ? "" : try CredentialStore.read(); credential = token }
        catch { self.error = error.localizedDescription; return }
        guard isSample || !token.isEmpty else { error = "Add your Cloudflare API token in Settings."; return }
        let input = RequestInput(snapshot: snapshot, action: action, context: session.context, level: settings.level, limited: limited, chartChoice: chart, question: followUp, previousResult: action == .followUp ? session.result?.text ?? "" : "", conversation: action == .followUp ? session.conversation : [])
        do { try input.validate() } catch { self.error = error.localizedDescription; return }
        let provider: any GenerativeProvider = isSample ? SampleProvider() : CloudflareProvider(accountID: settings.accountID, token: token, model: settings.model)
        let evaluator: (any DecisionClient)? = settings.jevEnabled && !isSample ? JevClient(accountID: settings.accountID, token: token) : nil
        let metrics = self.metrics
        let pipeline = RequestPipeline(provider: provider, evaluator: evaluator, localOnly: settings.localOnly || isSample, metrics: { event in Task { await metrics.record(event) } })
        progress = "Preparing…"
        runner.start(pipeline: pipeline, input: input, progress: { [weak self] message in self?.progress = message }) { [weak self] outcome in
            guard let self else { return }
            self.progress = nil; self.credential = nil
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
        if settings.paused { needsDisclosure = false }
    }
}
