import SwiftUI
import Charts
import PickleCore

private let pickleGreen = Color(red: 0.30, green: 0.43, blue: 0.22)
struct ActionMenu: View {
    let snapshot: SelectionSnapshot, choose: (ReadingAction) -> Void, dismiss: () -> Void
    var body: some View {
        VStack(spacing: 16) {
            HStack(spacing: 9) {
                Image(systemName: "leaf.fill").foregroundStyle(pickleGreen)
                Text("Pickle").font(.system(size: 16, weight: .semibold, design: .rounded))
                Text("\(snapshot.text.count) characters captured").font(.callout).foregroundStyle(.secondary)
                Spacer()
                Button(action: dismiss) { Image(systemName: "xmark").padding(5) }.buttonStyle(.plain).accessibilityLabel("Dismiss selection menu")
            }
            Text(snapshot.text).font(.callout).foregroundStyle(.secondary)
                .lineLimit(2).frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
                .accessibilityLabel("Captured selection: \(snapshot.text)")
            HStack(spacing: 10) {
                action(.simplify, icon: "text.alignleft")
                action(.expand, icon: "text.badge.plus")
                action(.chart, icon: "chart.bar.xaxis")
            }
        }.padding(20).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 24))
            .overlay(RoundedRectangle(cornerRadius: 24).strokeBorder(.white.opacity(0.35)))
            .padding(6)
    }
    private func action(_ action: ReadingAction, icon: String) -> some View {
        Button { choose(action) } label: {
            Label(action.rawValue, systemImage: icon).font(.system(size: 16, weight: .medium))
                .frame(maxWidth: .infinity).padding(.vertical, 12)
                .background(pickleGreen.opacity(0.09), in: RoundedRectangle(cornerRadius: 12))
                .contentShape(RoundedRectangle(cornerRadius: 12))
        }.buttonStyle(.plain).accessibilityLabel(action.rawValue)
    }
}
struct ReaderView: View {
    @ObservedObject var coordinator: RequestCoordinator
    @ObservedObject var session: SessionStore
    @ObservedObject var settings: SettingsStore
    @ObservedObject var app: AppDelegate
    var floating = false
    @State private var originalExpanded = false
    @FocusState private var questionFocused: Bool
    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
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
                    if let progress = coordinator.progress {
                        HStack(spacing: 12) { ProgressView().controlSize(.small); Text(progress).font(.callout); Spacer(); Button("Cancel") { coordinator.cancel() } }
                            .padding(16).background(pickleGreen.opacity(0.07), in: RoundedRectangle(cornerRadius: 12))
                            .accessibilityElement(children: .combine)
                    }
                    if let error = coordinator.error { notice("Couldn’t complete this action", detail: error, icon: "exclamationmark.triangle"); Button("Retry") { coordinator.retry() } }
                    if let result = session.result { resultBody(result) }
                    ForEach(session.conversation) { turn in
                        VStack(alignment: .leading, spacing: 10) {
                            Text(turn.question).font(.headline)
                            Text(turn.answer).textSelection(.enabled).lineSpacing(5)
                            Label(turn.quality.label, systemImage: "text.badge.checkmark").font(.caption).foregroundStyle(.secondary)
                        }.padding(16).background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 12))
                    }
                }.padding(24).frame(maxWidth: .infinity, alignment: .leading)
            }
            if session.result != nil { followUp }
            HStack {
                Image(systemName: "hand.raised").font(.caption)
                Text("Session stays in memory. Cloud actions send text to providers.").font(.caption2)
                Spacer()
                Button("Clear") { coordinator.clear() }.buttonStyle(.plain).font(.caption)
            }.foregroundStyle(.secondary).padding(.horizontal, 20).padding(.vertical, 10)
        }
        .background {
            if floating { RoundedRectangle(cornerRadius: 24).fill(.regularMaterial) }
            else { Color(nsColor: .windowBackgroundColor) }
        }
        .clipShape(RoundedRectangle(cornerRadius: floating ? 24 : 0))
        .overlay { if floating { RoundedRectangle(cornerRadius: 24).strokeBorder(.white.opacity(0.35)) } }
        .padding(floating ? 6 : 0).tint(pickleGreen)
        .onExitCommand { app.closePanel() }
    }
    private var header: some View {
        HStack(spacing: 10) {
            Image(systemName: "leaf.fill").font(.title2).foregroundStyle(pickleGreen)
            VStack(alignment: .leading, spacing: 2) {
                Text("Pickle").font(.system(size: 19, weight: .semibold, design: .rounded))
                Text(floating ? "\(session.snapshot?.text.count ?? 0) characters captured" : "A little clarity, right here.").font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            Button { app.openSettings() } label: { Image(systemName: "gearshape") }.help("Settings").accessibilityLabel("Open settings")
            Button { app.togglePin() } label: { Image(systemName: app.pinned ? "pin.fill" : "pin") }.help(app.pinned ? "Unpin" : "Pin above other windows").accessibilityLabel(app.pinned ? "Unpin result" : "Pin result")
            Button { app.closePanel() } label: { Image(systemName: "xmark") }.help("Close").accessibilityLabel("Close result")
        }.buttonStyle(.borderless).padding(.horizontal, 22).padding(.vertical, 16)
    }
    private var welcome: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("Stay with what\nyou’re reading.").font(.system(size: 33, weight: .semibold, design: .rounded)).tracking(-0.7)
            Text("Highlight a passage. Invoke Pickle. Find the explanation that makes it click.").font(.title3).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack { Label("1  Highlight", systemImage: "text.cursor"); Spacer(); Label("2  Invoke", systemImage: "command"); Spacer(); Label("3  Understand", systemImage: "sparkles") }.font(.caption).foregroundStyle(pickleGreen)
            Text(settings.shortcutLabel).font(.system(.callout, design: .monospaced)).padding(12).frame(maxWidth: .infinity).background(.quaternary, in: RoundedRectangle(cornerRadius: 10))
            HStack {
                Button("Set up Pickle…") { app.openSettings(); settings.onboarded = true }.buttonStyle(.borderedProminent)
                Button("Try offline sample") { coordinator.sample() }.buttonStyle(.bordered)
            }
            Divider()
            Text("Or paste a passage").font(.headline)
            TextEditor(text: $coordinator.manualText).font(.body).frame(minHeight: 100, maxHeight: 160).padding(8).background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 10)).overlay(RoundedRectangle(cornerRadius: 10).stroke(.quaternary)).accessibilityLabel("Passage to explain")
            HStack { Text("Up to 12,000 UTF-8 bytes").font(.caption).foregroundStyle(.secondary); Spacer(); Button("Use this passage") { coordinator.pasteSelection() }.disabled(coordinator.manualText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || settings.paused) }
            Text("Pickle reads supported selections through macOS Accessibility. No browser extension is needed. Some apps require manual paste.").font(.caption).foregroundStyle(.secondary)
        }
    }
    private func selection(_ snapshot: SelectionSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack { Label(snapshot.appName, systemImage: "macwindow"); Spacer(); Text(snapshot.capturedAt, style: .time) }.font(.caption).foregroundStyle(.secondary)
            DisclosureGroup("Original selection · \(snapshot.text.count) characters", isExpanded: $originalExpanded) {
                Text(snapshot.text).font(.callout).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 10)
                Text("\(snapshot.method) · \(snapshot.bundleID)").font(.caption2).foregroundStyle(.secondary)
                ForEach(snapshot.limitations, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
            }
            if session.result == nil { Text(snapshot.text).lineLimit(floating ? 2 : 4).foregroundStyle(.secondary).italic() }
            HStack(spacing: 8) {
                actionButton(.simplify, icon: "text.alignleft", key: "1")
                actionButton(.expand, icon: "text.badge.plus", key: "2")
                actionButton(.chart, icon: "chart.bar.xaxis", key: "3")
            }.disabled(coordinator.progress != nil || settings.paused || coordinator.needsDisclosure)
            DisclosureGroup("Add surrounding context") {
                TextEditor(text: $session.context).font(.callout).frame(height: 90).accessibilityLabel("Additional source context")
                Text("Only text you add is used. Up to 6,000 UTF-8 bytes.").font(.caption).foregroundStyle(.secondary)
            }.font(.callout).disabled(coordinator.progress != nil)
        }
    }
    private func actionButton(_ action: ReadingAction, icon: String, key: KeyEquivalent) -> some View {
        Button { coordinator.run(action) } label: { Label(action == .chart ? "Chart" : action.rawValue, systemImage: icon).frame(maxWidth: .infinity) }
            .buttonStyle(.bordered).controlSize(.large).keyboardShortcut(key, modifiers: .command)
    }
    private var disclosure: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label("Before your first cloud request", systemImage: "network").font(.headline)
            Text("Your selection, added context, and relevant conversation are sent to Cloudflare for generation. With Jev enabled, Cloudflare also routes the selection and generated answer to TypeSafe for evaluation. Provider handling follows their policies; native does not mean local.").font(.callout)
            Text("Highlighting alone never sends text. You can pause Pickle, exclude apps, disable Jev, or use local-only mode in Settings.").font(.caption).foregroundStyle(.secondary)
            HStack {
                Button("Agree and continue") {
                    settings.cloudConsent = true; if settings.jevEnabled { settings.jevConsent = true }
                    coordinator.needsDisclosure = false
                    // Allow settings observers to settle before starting an authorized request.
                    Task { @MainActor in await Task.yield(); coordinator.retry() }
                }.buttonStyle(.borderedProminent)
                Button("Cancel") { coordinator.needsDisclosure = false }
            }
        }.padding(18).background(pickleGreen.opacity(0.08), in: RoundedRectangle(cornerRadius: 14))
    }
    private func contextRequest(_ flags: [String]) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            notice("A little more context would help", detail: "The passage may depend on information outside your selection. Add surrounding text above, or continue with an explicitly limited explanation.", icon: "text.bubble")
            Text(flags.joined(separator: ", ").replacingOccurrences(of: "_", with: " ")).font(.caption).foregroundStyle(.secondary)
            Button("Use added context and retry") { coordinator.retry() }
            Button("Continue with a limited explanation") { coordinator.run(coordinator.pendingAction, limited: true, followUp: coordinator.question) }
        }
    }
    private var chartChoice: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Choose a supported format").font(.headline)
            Text("Jev is unavailable or did not make a clear choice. Choose a bar chart for explicit quantities with matching units, or a flow diagram for stated steps and relationships. Source and structure checks still apply.").font(.callout).foregroundStyle(.secondary)
            HStack { Button("Bar chart") { coordinator.run(.chart, chart: .bar) }; Button("Flow diagram") { coordinator.run(.chart, chart: .flow) } }
        }
    }
    private func resultBody(_ result: ReadingResult) -> some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack { Text(result.action.rawValue).font(.system(.title, design: .rounded).weight(.semibold)); Spacer(); Button { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(result.text, forType: .string) } label: { Label("Copy", systemImage: "doc.on.doc") }; Button("Retry") { coordinator.run(result.action) }.disabled(coordinator.progress != nil) }
            if let chart = result.chart { ResultRenderer(chart: chart) }
            else { Text(result.text).font(.system(size: 16)).lineSpacing(6).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
            VStack(alignment: .leading, spacing: 7) {
                Label(result.quality.label, systemImage: "text.badge.checkmark").font(.caption.weight(.medium))
                Text(result.quality.details).font(.caption).foregroundStyle(.secondary)
                ForEach(result.notes, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
                Text("\(result.model) · \(result.elapsed.formatted(.number.precision(.fractionLength(1))))s\(result.repairs > 0 ? " · 1 revision" : "")").font(.caption2).foregroundStyle(.secondary)
                if let evaluator = result.evaluatorModel { Text("Jev: \(evaluator) via Cloudflare").font(.caption2).foregroundStyle(.secondary) }
            }.padding(14).frame(maxWidth: .infinity, alignment: .leading).background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 12))
        }
    }
    private var followUp: some View {
        VStack(spacing: 8) {
            Divider()
            HStack {
                TextField("Ask about this selection…", text: $coordinator.question).textFieldStyle(.plain).focused($questionFocused).onSubmit(sendFollowUp).accessibilityLabel("Follow-up question")
                Button(action: sendFollowUp) { Image(systemName: "arrow.up.circle.fill").font(.title2) }.buttonStyle(.plain).foregroundStyle(pickleGreen).accessibilityLabel("Send follow-up").disabled(coordinator.question.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || coordinator.progress != nil || settings.paused)
            }.padding(.horizontal, 22).padding(.vertical, 10)
            if !session.conversation.isEmpty { Button("Clear follow-ups (keep selection)") { session.conversation = [] }.font(.caption).padding(.bottom, 5).disabled(coordinator.progress != nil) }
        }
    }
    private func sendFollowUp() {
        let question = coordinator.question.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !question.isEmpty, coordinator.progress == nil else { return }
        coordinator.run(.followUp, followUp: question)
    }
    private func notice(_ title: String, detail: String, icon: String) -> some View {
        VStack(alignment: .leading, spacing: 8) { Label(title, systemImage: icon).font(.headline); Text(detail).font(.callout).foregroundStyle(.secondary) }
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
                        Text(labels[edge.from] ?? "").padding(10).frame(maxWidth: .infinity).background(pickleGreen.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
                        HStack { Image(systemName: "arrow.down"); Text(edge.condition.isEmpty ? "leads to" : edge.condition).font(.caption) }.frame(maxWidth: .infinity)
                        Text(labels[edge.to] ?? "").padding(10).frame(maxWidth: .infinity).background(pickleGreen.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
                        Text("Source: \(edge.evidence)").font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                }
            }
            DisclosureGroup("Accessible text alternative") { Text(chart.alternative).font(.callout).textSelection(.enabled) }
        }
    }
}
