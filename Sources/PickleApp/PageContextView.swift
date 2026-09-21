import SwiftUI

struct PageContextView: View {
    @ObservedObject var coordinator: RequestCoordinator
    @ObservedObject var session: SessionStore
    @ObservedObject var settings: SettingsStore
    let openSettings: () -> Void
    var body: some View {
        if coordinator.pageLoading {
            HStack(spacing: 8) { ProgressView().controlSize(.mini); Text("Reading page on this Mac…"); Spacer(); Button("Skip", action: coordinator.skipPage) }
                .font(.caption).foregroundStyle(.secondary)
        } else if let page = session.page {
            DisclosureGroup {
                VStack(alignment: .leading, spacing: 12) {
                    if let image = NSImage(data: page.jpeg) {
                        Image(nsImage: image).resizable().scaledToFit().frame(maxHeight: 180)
                            .clipShape(RoundedRectangle(cornerRadius: 8)).accessibilityLabel("Captured source window")
                    }
                    Text("Captured once for this session. Only this window is included; content below the fold is not captured.").font(.caption).foregroundStyle(.secondary)
                    if !page.text.isEmpty {
                        DisclosureGroup("Preview extracted text") { Text(page.text).font(.caption).textSelection(.enabled) }
                    }
                    if page.truncated { Text("A long page: only the first portion of readable text is included.").font(.caption).foregroundStyle(.secondary) }
                    if !session.visualSummary.isEmpty {
                        DisclosureGroup("Visual context ready") { Text(session.visualSummary).font(.caption).textSelection(.enabled) }
                        Text("This AI summary is reused for follow-ups. It may misread visual details.").font(.caption).foregroundStyle(.secondary)
                    } else {
                        Text("For charts or diagrams, Analyze visuals sends this screenshot and selection to Cloudflare once. It may incur a model charge. Your account must have the vision model enabled.").font(.caption).foregroundStyle(.secondary)
                        Button(coordinator.visualBusy ? "Analyzing visuals…" : "Analyze visuals with Cloudflare", action: coordinator.analyzeVisuals)
                            .disabled(coordinator.visualBusy || coordinator.progress != nil || settings.localOnly || settings.paused)
                    }
                    HStack {
                        Text(page.capturedAt, style: .time).font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        Button("Remove page context", action: coordinator.removePage)
                    }
                }.padding(.top, 8)
            } label: {
                Label(page.text.isEmpty ? "Screenshot ready" : "Page context included", systemImage: "rectangle.and.text.magnifyingglass")
                    .font(.caption).foregroundStyle(pickleCyan)
            }
        } else if let status = coordinator.pageStatus {
            HStack { Text(status).font(.caption).foregroundStyle(.secondary); Spacer(); Button("Settings", action: openSettings).font(.caption) }
        }
    }
}
