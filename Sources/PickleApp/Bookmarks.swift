import SwiftUI
import PickleCore

struct SavedAnswer: Identifiable, Codable {
    var id = UUID()
    let savedAt: Date
    let passage: String
    let answer: String
    let source: String
}

@MainActor final class BookmarkStore: ObservableObject {
    @Published private(set) var items: [SavedAnswer] = []
    @Published var error: String?
    private let defaults: UserDefaults
    private var unreadable = false
    init(defaults: UserDefaults) {
        self.defaults = defaults
        if let data = defaults.data(forKey: "savedAnswers") {
            do { items = try JSONDecoder().decode([SavedAnswer].self, from: data) }
            catch { unreadable = true; self.error = "Saved answers could not be opened. Existing data has not been changed." }
        }
    }
    func contains(passage: String, answer: String) -> Bool { items.contains { $0.passage == passage && $0.answer == answer } }
    func save(passage: String, answer: String, source: String) {
        guard !contains(passage: passage, answer: answer) else { return }
        guard !unreadable else { error = "Saved answers could not be opened. Existing data has not been changed."; return }
        guard items.count < 200 else { error = "Your library is full. Remove a saved answer to make room."; return }
        persist([SavedAnswer(savedAt: Date(), passage: passage, answer: answer, source: source)] + items)
    }
    func remove(_ id: UUID) { persist(items.filter { $0.id != id }) }
    private func persist(_ updated: [SavedAnswer]) {
        do {
            let data = try JSONEncoder().encode(updated)
            defaults.set(data, forKey: "savedAnswers")
            items = updated; error = nil
        } catch { self.error = "Couldn’t save your changes. Please try again." }
    }
}

struct BookmarkButton: View {
    @ObservedObject var store: BookmarkStore
    let passage: String, answer: String, source: String
    var body: some View {
        Button {
            store.save(passage: passage, answer: answer, source: source)
        } label: {
            Image(systemName: store.contains(passage: passage, answer: answer) ? "bookmark.fill" : "bookmark")
        }.buttonStyle(.plain).foregroundStyle(pickleGreen)
            .alert("Couldn’t save answer", isPresented: Binding(get: { store.error != nil }, set: { if !$0 { store.error = nil } })) { Button("OK") { store.error = nil } } message: { Text(store.error ?? "") }
            .help("Save this answer on this Mac").accessibilityLabel(store.contains(passage: passage, answer: answer) ? "Answer saved" : "Save answer on this Mac")
    }
}

struct SavedAnswersView: View {
    @ObservedObject var store: BookmarkStore
    @ObservedObject var settings: SettingsStore
    @State private var search = ""
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Your saved ideas.").font(.system(size: 28, weight: .bold, design: .rounded))
            Text("Only answers you save appear here. Stored on this Mac.").font(.callout).foregroundStyle(.secondary)
            TextField("Search saved answers", text: $search).textFieldStyle(GlassFieldStyle())
            if let error = store.error { Text(error).foregroundStyle(.secondary) }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    let matches = store.items.filter { search.isEmpty || ($0.passage + $0.answer).localizedCaseInsensitiveContains(search) }
                    if matches.isEmpty { Text(store.items.isEmpty ? "Save an answer with the bookmark button while you read." : "No matching answers.").foregroundStyle(.secondary).padding(.vertical, 30) }
                    ForEach(matches) { item in
                        GlassSection(item.source) {
                            Text(item.answer).font(.system(size: settings.textSize, design: .rounded)).textSelection(.enabled)
                            DisclosureGroup("Original passage") { Text(item.passage).textSelection(.enabled).padding(.top, 8) }
                            HStack {
                                Text(item.savedAt, style: .date).font(.caption).foregroundStyle(.secondary)
                                Spacer()
                                Button("Copy") { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(item.answer, forType: .string) }
                                Button("Remove") { store.remove(item.id) }
                            }
                        }
                    }
                }
            }
        }.padding(24).frame(minWidth: 480, minHeight: 400)
            .background { PortalSurface() }.pickleAppearance(settings).buttonStyle(PickleActionStyle())
    }
}
