import SwiftUI
import PickleCore

struct WebReferenceView: View {
    @ObservedObject var coordinator: RequestCoordinator
    @ObservedObject var session: SessionStore
    @ObservedObject var settings: SettingsStore
    var body: some View {
        if coordinator.referenceLoading {
            HStack { ProgressView().controlSize(.mini); Text("Reading page reference…").font(.caption); Spacer(); Button("Skip", action: coordinator.removeReference) }
        } else if let reference = session.reference {
            DisclosureGroup {
                VStack(alignment: .leading, spacing: 10) {
                    Text(reference.title).font(.headline)
                    Text(reference.text).font(.caption).textSelection(.enabled)
                    if let url = URL(string: reference.url) { Link("Open source", destination: url) }
                    Text("A session snapshot. Only included excerpts are used; navigate or invoke Pickle again to refresh.").font(.caption).foregroundStyle(.secondary)
                    Button("Remove reference", action: coordinator.removeReference)
                }.padding(.top, 8)
            } label: {
                Label(reference.kind == "article" ? "Page reference included" : "Video context included", systemImage: reference.kind == "article" ? "doc.text" : "play.rectangle")
                    .font(.caption).foregroundStyle(pickleCyan)
            }
        } else if let status = coordinator.referenceStatus {
            Text(status).font(.caption).foregroundStyle(.secondary)
        } else if session.snapshot?.sourceURL != nil && !settings.webContextEnabled {
            Toggle("Include page reference", isOn: $settings.webContextEnabled)
                .font(.caption).onChange(of: settings.webContextEnabled) { if settings.webContextEnabled { coordinator.captureReference() } }
        }
    }
}
