import SwiftUI
import PickleCore

struct SettingsView: View {
    @ObservedObject var settings: SettingsStore
    @ObservedObject var coordinator: RequestCoordinator
    @State private var token = ""
    @State private var keyStatus = ""
    @State private var hasPermission = SelectionService.trusted
    @State private var tab = 0
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack(spacing: 12) {
                PicklePortal().scaleEffect(0.55).frame(width: 72, height: 64)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Your corner of the multiverse.").font(.system(size: 25, weight: .bold, design: .rounded))
                    Text("Tune your Pickle. Keep it simple.").font(.callout).foregroundStyle(.secondary)
                }
            }.padding(.horizontal, 24).padding(.top, 24)
            Picker("Settings", selection: $tab) {
                Text("Reading").tag(0)
                Text("Privacy").tag(1)
                Text("Connection").tag(2)
            }.pickerStyle(.segmented).labelsHidden().padding(.horizontal, 24)
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if tab == 0 { reading }
                    if tab == 1 { privacy }
                    if tab == 2 { connection }
                }.padding(.horizontal, 24).padding(.bottom, 24)
            }
            .buttonStyle(PickleActionStyle())
            .textFieldStyle(GlassFieldStyle())
            .toggleStyle(GlassToggleStyle())
            .font(.system(size: 13, design: .rounded))
        }.frame(minWidth: 540).background { PortalSurface() }.tint(pickleGreen)
            .preferredColorScheme(.dark)
            .onChange(of: scenePhase) { hasPermission = SelectionService.trusted }
    }
    private var reading: some View {
        Group {
            GlassSection("Your explanations") {
                Picker("Reading style", selection: $settings.level) {
                    ForEach(ReadingLevel.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                Toggle("Check answers against the passage", isOn: $settings.jevEnabled).disabled(settings.localOnly)
                Text("An extra check for meaning and missing context.").font(.caption).foregroundStyle(.secondary)
            }
            GlassSection("Open Pickle") {
                Toggle("Use Control + Option", isOn: $settings.controlOption)
                Text("Highlight a passage, press both keys, then release.").font(.caption).foregroundStyle(.secondary)
                if !settings.controlOption {
                    Picker("Modifier keys", selection: $settings.shortcutModifiers) {
                        Text("Control + Option").tag("Control + Option"); Text("Command + Shift").tag("Command + Shift")
                    }
                    Picker("Key", selection: $settings.shortcutKey) {
                        ForEach(["P", "K", "Space", "Return"], id: \.self) { Text($0).tag($0) }
                    }
                }
                Toggle("Show actions when I highlight text", isOn: $settings.automaticMenu)
                Text("Drag the handle at the top of the floating panel to move it.").font(.caption).foregroundStyle(.secondary)
            }
            GlassSection("Selection access") {
                Label(hasPermission ? "Ready to read selected text" : "Allow Pickle to read selected text", systemImage: hasPermission ? "checkmark.circle.fill" : "text.cursor")
                    .foregroundStyle(hasPermission ? pickleGreen : .primary)
                Text("Enable Pickle in macOS Accessibility settings. You can always paste a passage instead.").font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button("Open permissions") { SelectionService.requestPermission() }
                    Button("Check again") { hasPermission = SelectionService.trusted }
                }
            }
            GlassSection { Toggle("Pause Pickle", isOn: $settings.paused) }
        }
    }
    private var privacy: some View {
        Group {
            GlassSection("You choose what to share") {
                Text("Highlighting alone sends nothing. When you choose an action, your passage and conversation go to Cloudflare. Answer checks also share the passage and answer with TypeSafe.")
                    .font(.callout).foregroundStyle(.secondary)
                Toggle("Keep Pickle offline", isOn: $settings.localOnly)
                Text("Offline mode includes a sample explanation. New explanations need an internet connection.").font(.caption).foregroundStyle(.secondary)
            }
            GlassSection("Your session") {
                Text("Passages and answers stay in memory until you clear them or quit. Pickle does not save a reading history.").font(.callout).foregroundStyle(.secondary)
                Button("Clear current passage and answers") { coordinator.clear() }
                Button("Ask before sharing again") { settings.cloudConsent = false; settings.jevConsent = false }
            }
            GlassSection {
                DisclosureGroup("Excluded apps") {
                    Text("Pickle ignores these apps. Enter one application identifier per line, such as com.apple.mail.").font(.caption).foregroundStyle(.secondary)
                    TextEditor(text: $settings.exclusions).font(.callout).scrollContentBackground(.hidden).frame(height: 100).padding(10).glassInset(radius: 8).accessibilityLabel("Excluded application identifiers")
                }
            }
        }
    }
    private var connection: some View {
        Group {
            GlassSection("Connect your account") {
                Text("Pickle uses Cloudflare to create explanations. Connect once to start reading.").font(.callout).foregroundStyle(.secondary)
                TextField("Cloudflare account ID", text: $settings.accountID)
                SecureField("API token", text: $token)
                HStack {
                    Button("Save connection") {
                        do { try CredentialStore.save(token.trimmingCharacters(in: .whitespacesAndNewlines)); token = ""; keyStatus = "Saved securely in Keychain." }
                        catch { keyStatus = error.localizedDescription }
                    }.buttonStyle(PickleActionStyle(selected: true)).disabled(token.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    Link("Get an API token", destination: URL(string: "https://dash.cloudflare.com/?to=/:account/ai/workers-ai")!)
                }
                if !keyStatus.isEmpty { Text(keyStatus).font(.caption).foregroundStyle(.secondary) }
            }
            GlassSection {
                DisclosureGroup("Advanced connection settings") {
                    TextField("Writing model", text: $settings.model)
                    Button("Allow access to saved token") {
                        Task {
                            do {
                                let present = try await Task.detached { !(try CredentialStore.read(interactive: true)).isEmpty }.value
                                keyStatus = present ? "Connection is ready." : "No saved token. Add one above."
                            } catch { keyStatus = error.localizedDescription }
                        }
                    }
                    Button("Remove saved token") {
                        do { try CredentialStore.save(""); keyStatus = "Saved token removed." }
                        catch { keyStatus = error.localizedDescription }
                    }
                }
            }
        }
    }
}
