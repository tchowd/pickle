import SwiftUI
import PickleCore

struct WebReferenceView: View {
    @ObservedObject var coordinator: RequestCoordinator
    @ObservedObject var session: SessionStore
    @ObservedObject var settings: SettingsStore
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
        referenceBody
        if settings.webContextEnabled, session.snapshot?.sourceURL != nil, session.captureTarget != nil {
            if coordinator.listening {
                HStack { ProgressView().controlSize(.mini); Text("Listening · up to 30 seconds").font(.caption); Button("Cancel", action: coordinator.stopListening) }
            } else {
                DisclosureGroup("No captions?") {
                    Text("Play the video, then listen. Records audio from the source app for 30 seconds, including other playing tabs in that app. Speech is transcribed on this Mac. Nothing before you start is recorded.").font(.caption).foregroundStyle(.secondary)
                    Button("Listen for 30 seconds", action: coordinator.listen)
                        .disabled(coordinator.progress != nil || coordinator.pageLoading || coordinator.referenceLoading || coordinator.visualBusy || settings.paused)
                }.font(.caption)
            }
        }
        }
    }
    @ViewBuilder private var referenceBody: some View {
        if coordinator.referenceLoading {
            HStack { ProgressView().controlSize(.mini); Text("Reading page reference…").font(.caption); Spacer(); Button("Skip", action: coordinator.skipReference) }
        } else if let reference = session.reference {
            DisclosureGroup {
                VStack(alignment: .leading, spacing: 10) {
                    Text(reference.title).font(.headline)
                    if let time = reference.timestamp {
                        Text("At \(Int(time) / 60):\(String(format: "%02d", Int(time) % 60))").font(.caption).foregroundStyle(.secondary)
                    }
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
