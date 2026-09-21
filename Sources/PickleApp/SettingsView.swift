import SwiftUI
import PickleCore

struct SettingsView: View {
    @ObservedObject var settings: SettingsStore
    @ObservedObject var coordinator: RequestCoordinator
    @State private var token = ""
    @State private var keyStatus = ""
    @State private var screenPermission = ScreenContextService.permitted
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
                Text("Appearance").tag(3)
            }.pickerStyle(.segmented).labelsHidden().padding(.horizontal, 24)
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if tab == 0 { reading }
                    if tab == 1 { privacy }
                    if tab == 2 { connection }
                    if tab == 3 { appearance }
                }.padding(.horizontal, 24).padding(.bottom, 24)
            }
            .buttonStyle(PickleActionStyle())
            .textFieldStyle(GlassFieldStyle())
            .toggleStyle(GlassToggleStyle())
            .font(.system(size: 13, design: .rounded))
        }.frame(minWidth: 540).background { PortalSurface() }.tint(pickleGreen)
            .pickleAppearance(settings)
            .onChange(of: scenePhase) { hasPermission = SelectionService.trusted; screenPermission = ScreenContextService.permitted }
    }
    private var appearance: some View {
        VStack(spacing: 16) {
            GlassSection("Reading comfort") {
                HStack { Text("Text size"); Spacer(); Text("\(Int(settings.textSize)) pt").foregroundStyle(.secondary) }
                Slider(value: $settings.textSize, in: 14...24, step: 1).accessibilityLabel("Reading text size")
                Text("A little clarity, your way.").font(.system(size: settings.textSize, design: .rounded))
            }
            GlassSection("Glass and color") {
                Text("Background opacity")
                Slider(value: $settings.glassOpacity, in: 0...1).accessibilityLabel("Background opacity")
                HStack { Text("More glass"); Spacer(); Text("More solid") }.font(.caption).foregroundStyle(.secondary)
                Text("Pickle intensity")
                Slider(value: $settings.themeIntensity, in: 0...1).accessibilityLabel("Theme intensity")
                HStack { Text("Subtle"); Spacer(); Text("Neon") }.font(.caption).foregroundStyle(.secondary)
                Button("Restore appearance defaults") { settings.textSize = 18; settings.glassOpacity = 0.12; settings.themeIntensity = 1 }
            }
        }
    }
    private var reading: some View {
        Group {
            GlassSection("Your explanations") {
                Toggle("Show answers as they arrive", isOn: $settings.streaming)
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
                    Button("Check again") { hasPermission = SelectionService.trusted; screenPermission = ScreenContextService.permitted }
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
            GlassSection("Page context") {
                Toggle("Include visible page context", isOn: $settings.screenContextEnabled)
                Text("Capture the source window once per session and read its text on this Mac. Extracted text is sent with reading actions; images are sent only through Analyze visuals. Captures are kept in memory and cleared with the session.").font(.caption).foregroundStyle(.secondary)
                Label(screenPermission ? "Screen Recording is allowed" : "Screen Recording permission is needed", systemImage: screenPermission ? "checkmark.circle" : "rectangle.dashed")
                HStack {
                    Button("Allow screen access") { ScreenContextService.requestPermission(); screenPermission = ScreenContextService.permitted }
                    Button("Check again") { screenPermission = ScreenContextService.permitted }
                }
                Text("After granting permission, select your passage and invoke Pickle again. Excluded apps and protected selections are never captured.").font(.caption).foregroundStyle(.secondary)
                Link("Set up the optional vision model", destination: URL(string: "https://developers.cloudflare.com/workers-ai/models/llama-3.2-11b-vision-instruct/")!)
            }
            GlassSection("Your session") {
                Text("Passages and answers stay in memory until you clear them or quit. Only answers you explicitly bookmark are saved on this Mac.").font(.callout).foregroundStyle(.secondary)
                Button("Clear current passage and answers") { coordinator.clear() }
                Button("Ask before sharing again") { settings.cloudConsent = false; settings.jevConsent = false; settings.screenContextConsent = false }
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
