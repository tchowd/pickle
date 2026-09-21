import SwiftUI
import Charts
import PickleCore

struct ActionMenu: View {
    let snapshot: SelectionSnapshot, choose: (ReadingAction) -> Void, dismiss: () -> Void
    var body: some View {
        VStack(spacing: 10) {
            DragGrip()
            Text(snapshot.text).font(.system(size: 14, design: .rounded)).foregroundStyle(.secondary)
                .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: 6) {
                action(.simplify, icon: "text.alignleft")
                action(.expand, icon: "text.badge.plus")
                action(.chart, icon: "chart.bar.xaxis")
            }
        }.padding(.horizontal, 16).padding(.bottom, 16)
            .background { PortalSurface().clipShape(RoundedRectangle(cornerRadius: 20)) }
            .overlay(RoundedRectangle(cornerRadius: 20).strokeBorder(.white.opacity(0.16)))
            .padding(6)
            .onExitCommand(perform: dismiss)
            .preferredColorScheme(.dark)
    }
    private func action(_ action: ReadingAction, icon: String) -> some View {
        Button { choose(action) } label: {
            Label(action.title, systemImage: icon).frame(maxWidth: .infinity)
        }.buttonStyle(PickleActionStyle(selected: action == .simplify)).accessibilityLabel(action.title)
    }
}
struct ReaderView: View {
    @ObservedObject var coordinator: RequestCoordinator
    @ObservedObject var session: SessionStore
    @ObservedObject var settings: SettingsStore
    @ObservedObject var app: AppDelegate
    var floating = false
    @State private var originalExpanded = false
    @State private var copied = false
    @State private var term = ""
    @State private var wordPopover = false
    @FocusState private var questionFocused: Bool
    var body: some View {
        VStack(spacing: 0) {
            header
            ScrollViewReader { scroll in
            ScrollView {
                VStack(alignment: .leading, spacing: 22) {
                    if settings.paused { notice("Pickle is paused", detail: "Resume from the menu bar to capture text or run an action.", icon: "pause.circle") }
                    if let message = coordinator.captureMessage {
                        notice("Selection unavailable", detail: message, icon: "text.cursor")
                        Button("Paste a passage instead") { coordinator.clear() }
                    }
                    if let snapshot = session.snapshot { selection(snapshot) } else { welcome }
                    if coordinator.needsDisclosure { disclosure }
                    if let flags = coordinator.needsContext { contextRequest(flags) }
                    if coordinator.needsChart { chartChoice }
                    if coordinator.progress != nil {
                        HStack(spacing: 12) { ProgressView().controlSize(.small); Text(answerStatus ?? "Finding the words…").font(.callout); Spacer(); Button("Cancel") { coordinator.cancel() } }
                            .padding(16).glassInset()
                            .accessibilityElement(children: .combine)
                    }
                    if let error = coordinator.error { notice("Couldn’t complete this action", detail: error, icon: "exclamationmark.triangle"); Button("Retry") { coordinator.retry() } }
                    if let result = session.result, coordinator.draft.isEmpty || coordinator.pendingAction == .followUp { resultBody(result) }
                    ForEach(session.conversation) { turn in
                        VStack(alignment: .leading, spacing: 10) {
                            Text(turn.question).font(.headline)
                            SelectablePassage(text: turn.answer, size: settings.textSize, explain: coordinator.explainTerm)
                            HStack { Spacer(); BookmarkButton(store: app.bookmarks, passage: session.snapshot?.text ?? "", answer: turn.answer, source: session.snapshot?.appName ?? "Passage") }
                        }.padding(16).glassInset()
                    }
                    if !coordinator.draft.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            Text(answerStatus.map { "Draft · " + $0 } ?? "Writing…").font(.caption).foregroundStyle(.secondary)
                            Text(coordinator.draft).font(.system(size: settings.textSize, design: .rounded)).lineSpacing(7)
                        }.frame(maxWidth: .infinity, alignment: .leading).id("draft")
                    }
                    Color.clear.frame(height: 1).id("end")
                }.padding(24).frame(maxWidth: .infinity, alignment: .leading)
            }
            .onChange(of: coordinator.draft) { if !coordinator.draft.isEmpty { scroll.scrollTo("draft", anchor: .bottom) } }
            .onChange(of: session.conversation.count) { scroll.scrollTo("end", anchor: .bottom) }
            }
            if session.result != nil || !session.conversation.isEmpty { followUp }
            HStack {
                Text(session.result == nil ? "Small pickle. Big ideas." : "Stay curious. Get out of a pickle.").font(.caption)
                Spacer()
                if session.snapshot != nil { Button("New passage") { coordinator.clear() }.buttonStyle(.plain).font(.caption) }
            }.foregroundStyle(.secondary).padding(.horizontal, 24).padding(.vertical, 12)
        }
        .background { PortalSurface() }
        .clipShape(RoundedRectangle(cornerRadius: floating ? 20 : 0))
        .overlay { if floating { RoundedRectangle(cornerRadius: 20).strokeBorder(.white.opacity(0.16)) } }
        .overlay(alignment: .bottomTrailing) {
            if floating {
                Image(systemName: "arrow.down.right").font(.system(size: 9)).foregroundStyle(.secondary)
                    .frame(width: 22, height: 22).overlay(WindowResizeHandle()).padding(7)
            }
        }
        .padding(floating ? 6 : 0).tint(pickleGreen)
        .onChange(of: session.result?.id) { copied = false }
        .onExitCommand { app.closePanel() }
        .pickleAppearance(settings)
        .buttonStyle(PickleActionStyle())
        .fontDesign(.rounded)
    }
    private var header: some View {
        ZStack {
            DragGrip().padding(.horizontal, 48)
            HStack {
                Spacer()
                Button { app.openSavedAnswers() } label: { Image(systemName: "bookmark").frame(width: 28, height: 28) }
                    .buttonStyle(.plain).foregroundStyle(.secondary).help("Saved answers").accessibilityLabel("Open saved answers")
                Button { app.openSettings() } label: {
                    Image(systemName: "gearshape").font(.system(size: 13)).frame(width: 28, height: 28)
                }.buttonStyle(.plain).foregroundStyle(.secondary)
                    .help("Settings").accessibilityLabel("Open settings")
            }.padding(.horizontal, 12)
        }.frame(height: 32)
    }
    private var welcome: some View {
        VStack(alignment: .leading, spacing: 22) {
            HStack(alignment: .center, spacing: 0) {
                VStack(alignment: .leading, spacing: 10) {
                    Text("OUT OF THE WORDS.\nINTO THE KNOW.")
                        .font(.system(size: 10, weight: .medium, design: .monospaced)).tracking(2).foregroundStyle(pickleCyan)
                    Text("Big brain.\nPickle energy.").font(.system(size: 34, weight: .heavy, design: .rounded)).tracking(-1)
                }
                Spacer(minLength: 0)
                PicklePortal()
            }
            Text("Untangle a passage. Connect the dots. Take your brain somewhere new.")
                .font(.system(size: 15)).foregroundStyle(.secondary).lineSpacing(4)
            HStack(spacing: 10) {
                Image(systemName: "text.cursor").foregroundStyle(pickleGreen)
                Text("Highlight text, then press").font(.callout)
                Text(settings.shortcutLabel).font(.system(size: 12, weight: .medium)).padding(8)
                    .background(.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
            }
            Divider()
            Text("Or bring a passage here").font(.system(size: 14, weight: .medium))
            TextEditor(text: $coordinator.manualText).font(.system(size: 16, design: .rounded))
                .scrollContentBackground(.hidden).frame(minHeight: 100, maxHeight: 150).padding(12)
                .glassInset()
                .accessibilityLabel("Passage to explain")
            HStack {
                Button("Try an example") { coordinator.sample() }.buttonStyle(.plain).foregroundStyle(pickleGreen)
                Spacer()
                Button("Continue", action: coordinator.pasteSelection).buttonStyle(PickleActionStyle(selected: true))
                    .disabled(coordinator.manualText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || settings.paused)
            }
            if !SelectionService.trusted || settings.accountID.isEmpty {
                Button("Finish setting up Pickle", action: app.openSettings).buttonStyle(.plain).font(.caption).foregroundStyle(.secondary)
            }
        }
    }
    private func selection(_ snapshot: SelectionSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack { Label(snapshot.appName, systemImage: "macwindow"); Spacer(); Text(snapshot.capturedAt, style: .time) }.font(.caption).foregroundStyle(.secondary)
            DisclosureGroup(snapshot.text.isEmpty ? "Current page" : "Original passage", isExpanded: $originalExpanded) {
                SelectablePassage(text: snapshot.text, size: settings.textSize, explain: coordinator.explainTerm).padding(.top, 10)
            }
            if session.result == nil { Text(snapshot.text.isEmpty ? (snapshot.sourceURL ?? "Current page") : snapshot.text).lineLimit(floating ? 2 : 4).foregroundStyle(.secondary).italic() }
            HStack(spacing: 8) {
                actionButton(.simplify, icon: "text.alignleft", key: "1")
                actionButton(.expand, icon: "text.badge.plus", key: "2")
                actionButton(.chart, icon: "chart.bar.xaxis", key: "3")
            }.disabled(coordinator.progress != nil || settings.paused || coordinator.needsDisclosure)
            Button("Explain a word") { wordPopover = true }.buttonStyle(.plain).font(.caption).foregroundStyle(pickleCyan)
                .popover(isPresented: $wordPopover) {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("A word, in context").font(.headline)
                        Text("Enter a word or phrase from this passage. You can also select text and right-click to explain it.").font(.caption).foregroundStyle(.secondary)
                        TextField("Word or phrase", text: $term).textFieldStyle(GlassFieldStyle()).onSubmit(explainWord)
                        Button("Explain", action: explainWord).disabled(term.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || coordinator.progress != nil)
                    }.padding(20).frame(width: 300).background { PortalSurface() }.pickleAppearance(settings)
                }.disabled(coordinator.progress != nil || settings.paused)
            WebReferenceView(coordinator: coordinator, session: session, settings: settings)
            PageContextView(coordinator: coordinator, session: session, settings: settings, openSettings: app.openSettings)
            DisclosureGroup("Add surrounding context") {
                TextEditor(text: $session.context).font(.callout).scrollContentBackground(.hidden).frame(height: 90).padding(10).glassInset().accessibilityLabel("Additional source context")
                Text("Paste a few surrounding sentences for a fuller explanation.").font(.caption).foregroundStyle(.secondary)
            }.font(.callout).disabled(coordinator.progress != nil)
        }
    }
    private func actionButton(_ action: ReadingAction, icon: String, key: KeyEquivalent) -> some View {
        Button { coordinator.run(action) } label: { Label(action.title, systemImage: icon).frame(maxWidth: .infinity) }
            .buttonStyle(PickleActionStyle(selected: (session.result?.action ?? .simplify) == action)).keyboardShortcut(key, modifiers: .command)
    }
    private var disclosure: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label("Before we begin", systemImage: "network").font(.headline)
            Text("Pickle sends your passage and conversation to Cloudflare to write an explanation. When answer checks are enabled, TypeSafe also receives the passage and answer.").font(.callout)
            if session.page != nil {
                Text("Page context adds text read from the captured source window. This text and any visual summary are also sent with your request and answer checks. The screenshot itself is uploaded only when you choose Analyze visuals.").font(.callout)
            }
            if session.reference != nil {
                Text("Your page reference or video transcript is also shared with Cloudflare and enabled answer checks.").font(.callout)
            }
            Text("Nothing is sent to AI providers until you choose an action. You can change this in Settings.").font(.caption).foregroundStyle(.secondary)
            HStack {
                Button("Agree and continue") {
                    settings.cloudConsent = true; if session.reference != nil { settings.webContextConsent = true }; if session.page != nil { settings.screenContextConsent = true }; if settings.jevEnabled { settings.jevConsent = true }
                    coordinator.needsDisclosure = false
                    // Allow settings observers to settle before starting an authorized request.
                    Task { @MainActor in await Task.yield(); coordinator.retry() }
                }.buttonStyle(PickleActionStyle(selected: true))
                Button("Cancel") { coordinator.needsDisclosure = false }
            }
        }.padding(18).glassInset(radius: 14)
    }
    private func contextRequest(_ flags: [String]) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            notice("A little more context would help", detail: "Add a few surrounding sentences above, or continue with just this passage.", icon: "text.bubble")
            Button("Use added context and retry") { coordinator.retry() }
            Button("Continue with a limited explanation") { coordinator.run(coordinator.pendingAction, limited: true, followUp: coordinator.question) }
        }
    }
    private var chartChoice: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("How would you like to see it?").font(.headline)
            Text("Compare numbers with a bar chart, or connect ideas with a flow diagram.").font(.callout).foregroundStyle(.secondary)
            HStack { Button("Bar chart") { coordinator.run(.chart, chart: .bar) }; Button("Flow diagram") { coordinator.run(.chart, chart: .flow) } }
        }
    }
    private var answerStatus: String? {
        guard let progress = coordinator.progress, ["Reviewing…", "Refining…"].contains(progress) else { return nil }
        return progress
    }
    private func resultBody(_ result: ReadingResult) -> some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .firstTextBaseline) {
                Spacer()
                BookmarkButton(store: app.bookmarks, passage: session.snapshot?.text ?? "", answer: result.text, source: session.snapshot?.appName ?? "Passage")
                Button {
                    NSPasteboard.general.clearContents(); NSPasteboard.general.setString(result.text, forType: .string)
                    copied = true
                } label: { Image(systemName: copied ? "checkmark" : "doc.on.doc") }
                    .buttonStyle(.plain).help(copied ? "Copied" : "Copy answer").accessibilityLabel(copied ? "Copied" : "Copy answer")
                Button { coordinator.run(result.action) } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.plain).help("Try again").accessibilityLabel("Try again").disabled(coordinator.progress != nil)
            }
            if let chart = result.chart { ResultRenderer(chart: chart) }
            else { SelectablePassage(text: result.text, size: settings.textSize, explain: coordinator.explainTerm) }
            HStack(spacing: 8) {
                Button("Shorter") { coordinator.adjust("Rewrite the current explanation more briefly, preserving its meaning and qualifications.") }
                Button("More detail") { coordinator.adjust("Explain the current answer in more detail, staying grounded in the passage.") }
                Button("Give an example") { coordinator.adjust("Give a short example to clarify the passage. Clearly label invented examples as hypothetical.") }
            }.disabled(coordinator.progress != nil || settings.paused)
            if let error = app.bookmarks.error { Text(error).font(.caption).foregroundStyle(.secondary) }

        }
    }
    private var followUp: some View {
        VStack(spacing: 8) {
            HStack {
                TextField("Ask about this selection…", text: $coordinator.question).textFieldStyle(.plain).focused($questionFocused).onSubmit(sendFollowUp).accessibilityLabel("Follow-up question")
                Button(action: sendFollowUp) { Image(systemName: "arrow.up.circle.fill").font(.title2) }.buttonStyle(.plain).foregroundStyle(pickleGreen).accessibilityLabel("Send follow-up").disabled(coordinator.question.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || coordinator.progress != nil || settings.paused)
            }.padding(12).glassInset(focused: questionFocused)
                .padding(.horizontal, 20).padding(.vertical, 8)
            if !session.conversation.isEmpty { Button("Clear follow-ups (keep selection)") { session.conversation = [] }.font(.caption).padding(.bottom, 5).disabled(coordinator.progress != nil) }
        }
    }
    private func explainWord() {
        guard coordinator.progress == nil else { return }
        coordinator.explainTerm(term); wordPopover = false; term = ""
    }
    private func sendFollowUp() {
        let question = coordinator.question.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !question.isEmpty, coordinator.progress == nil else { return }
        coordinator.run(.followUp, followUp: question)
    }
    private func notice(_ title: String, detail: String, icon: String) -> some View {
        VStack(alignment: .leading, spacing: 8) { Label(title, systemImage: icon).font(.headline).foregroundStyle(pickleCyan); Text(detail).font(.callout).foregroundStyle(.secondary) }
            .frame(maxWidth: .infinity, alignment: .leading).padding(16).glassInset()
    }
}
struct ResultRenderer: View {
    let chart: ValidatedChart
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            switch chart {
            case .bar(let bars):
                Text("Quantities in your selection").font(.headline)
                Chart(Array(bars.enumerated()), id: \.element.id) { index, bar in
                    BarMark(x: .value(bar.unit, bar.value), y: .value("Source quantity", "\(index + 1). \(bar.quote)"))
                        .foregroundStyle(pickleGreen.gradient)
                        .accessibilityLabel(bar.excerpt).accessibilityValue("\(bar.value.formatted()) \(bar.unit)")
                }.chartXScale(domain: min(0, bars.map(\.value).min() ?? 0)...max(1, bars.map(\.value).max() ?? 1)).frame(height: CGFloat(bars.count * 48 + 36))
                ForEach(Array(bars.enumerated()), id: \.element.id) { index, bar in
                    VStack(alignment: .leading, spacing: 4) { Text("\(index + 1). \(bar.value.formatted()) \(bar.unit)").font(.callout.weight(.semibold)); Text(bar.excerpt).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                }
            case .flow(let nodes, let edges):
                let labels = Dictionary(uniqueKeysWithValues: nodes.map { ($0.id, $0.label) })
                Text("Relationships in your selection").font(.headline)
                ForEach(Array(edges.enumerated()), id: \.offset) { _, edge in
                    VStack(alignment: .leading, spacing: 10) {
                        Text(labels[edge.from] ?? "").padding(10).frame(maxWidth: .infinity).glassInset(radius: 8)
                        HStack { Image(systemName: "arrow.down"); Text(edge.condition.isEmpty ? "leads to" : edge.condition).font(.caption) }.frame(maxWidth: .infinity)
                        Text(labels[edge.to] ?? "").padding(10).frame(maxWidth: .infinity).glassInset(radius: 8)
                        Text("Source: \(edge.evidence)").font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                }
            }
            DisclosureGroup("Read as text") { Text(chart.alternative).font(.callout).textSelection(.enabled) }
        }
    }
}
