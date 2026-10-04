import SwiftUI
import PickleCore

extension KnowledgeEntry.Action {
    var title: String {
        switch self {
        case .simplify: return "Simplify"; case .expand: return "Explain more"; case .chart: return "Visualize"
        case .followUp: return "Follow-up"; case .adjustment: return "Adjustment"; case .explain: return "Explain"
        }
    }
}

/// Browsing the log never loads an entry into the reader or starts a request.
struct KnowledgeBaseView: View {
    @ObservedObject var store: KnowledgeBase
    @ObservedObject var settings: SettingsStore
    @State private var search = ""
    @State private var confirmDeleteAll = false
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Your knowledge base.").font(.system(size: 28, weight: .bold, design: .rounded))
            Text(settings.knowledgeBaseEnabled ? "Completed answers are recorded on this Mac. Turn this off in Settings → Privacy." : "Recording is off. Existing entries stay until you delete them. Turn it on in Settings → Privacy.")
                .font(.callout).foregroundStyle(.secondary)
            HStack(spacing: 10) {
                TextField("Search passages, questions, answers and sources", text: $search).textFieldStyle(GlassFieldStyle())
                Button("Delete all") { confirmDeleteAll = true }.disabled(store.entries.isEmpty)
            }
            if let error = store.error { Text(error).foregroundStyle(.secondary) }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    let matches = store.search(search)
                    if matches.isEmpty { Text(store.entries.isEmpty ? "Answers appear here after you read with the knowledge base turned on." : "No matching entries.").foregroundStyle(.secondary).padding(.vertical, 30) }
                    ForEach(matches) { entry in
                        GlassSection(entry.appName + " · " + entry.action.title + (entry.chart.map { $0 == .bar ? " · Bar chart" : " · Flow diagram" } ?? "")) {
                            if let question = entry.question { Text(question).font(.headline).textSelection(.enabled) }
                            SelectablePassage(text: entry.answer, size: settings.textSize, markdown: true)
                            if !entry.passage.isEmpty {
                                DisclosureGroup("Original passage") { Text(entry.passage).textSelection(.enabled).padding(.top, 8) }
                            }
                            if let reference = entry.reference {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(reference.title.isEmpty ? reference.kind.capitalized : reference.title)
                                    Text(reference.url).foregroundStyle(.secondary)
                                }.font(.caption).textSelection(.enabled)
                            }
                            HStack {
                                Text(entry.recordedAt.formatted(date: .abbreviated, time: .shortened)).font(.caption).foregroundStyle(.secondary)
                                Spacer()
                                Button("Copy") { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(AnswerMarkup.plainText(entry.answer), forType: .string) }
                                Button("Delete") { store.remove(entry.id) }
                            }
                        }
                    }
                }
            }
        }.padding(24).frame(minWidth: 520, minHeight: 440)
            .background { PortalSurface() }.pickleAppearance(settings).buttonStyle(PickleActionStyle())
            .confirmationDialog("Delete every knowledge base entry?", isPresented: $confirmDeleteAll) {
                Button("Delete all", role: .destructive) { store.removeAll() }
            } message: { Text("This removes all entries from this Mac. It can’t be undone.") }
    }
}

/// Recording problems surface beside the answer they affect.
struct KnowledgeBaseNotice: View {
    @ObservedObject var store: KnowledgeBase
    var body: some View {
        if let error = store.error { Text(error).font(.caption).foregroundStyle(.secondary) }
    }
}
