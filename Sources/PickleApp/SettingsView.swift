import SwiftUI
import PickleCore

struct SettingsView: View {
    @ObservedObject var settings: SettingsStore
    @ObservedObject var coordinator: RequestCoordinator
    @State private var token = ""
    @State private var localToken = ""
    @State private var keyStatus = ""
    @State private var screenPermission = ScreenContextService.permitted
    @State private var hasPermission = SelectionService.trusted
    @State private var tab = 0
    @Environment(\.scenePhase) private var scenePhase

    @State private var extensionID = ""
    @State private var extensionBrowser = "Chrome"
    @State private var extensionMessage = ""
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
                Toggle("Check answers against the passage", isOn: $settings.jevEnabled).disabled(settings.localOnly || settings.aiMode == .local)
                Text(settings.aiMode == .local ? "Cloud checks stay off in Local mode. Your Online preference is preserved." : "An extra check for meaning and missing context.").font(.caption).foregroundStyle(.secondary)
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
                Text(settings.aiMode == .local ? "Local sends your passage and context only to your configured model server. Cloud AI and TypeSafe checks stay off. Enabled webpage references still contact the original website." : "Highlighting alone sends nothing. When you choose an action, your passage and conversation go to Cloudflare. Answer checks also share the passage and answer with TypeSafe.")
                    .font(.callout).foregroundStyle(.secondary)
                Toggle("Keep Pickle offline", isOn: $settings.localOnly)
                Text("Strict offline blocks webpage fetches and AI on other computers, including Tailscale. A configured model on this Mac can still work.").font(.caption).foregroundStyle(.secondary)
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
            GlassSection("Browser and video context") {
                Toggle("Include webpage references", isOn: $settings.webContextEnabled)
                Text("When enabled, Pickle fetches the current public webpage once per session. This contacts the website without browser cookies. Offline mode stops these fetches. The browser extension can share the page you already have open, including available captions.").font(.caption).foregroundStyle(.secondary)
                DisclosureGroup("Connect browser extension") {
                    Text("Load the extension folder as an unpacked extension in your browser’s extension manager. Paste its ID here to connect it to Pickle.").font(.caption).foregroundStyle(.secondary)
                    Button("Show extension folder") {
                        if let folder = Bundle.main.resourceURL?.appendingPathComponent("browser-extension") { NSWorkspace.shared.activateFileViewerSelecting([folder]) }
                    }
                    Picker("Browser", selection: $extensionBrowser) {
                        ForEach(["Chrome", "Edge", "Brave", "Arc"], id: \.self) { Text($0) }
                    }
                    TextField("Extension ID", text: $extensionID).textFieldStyle(GlassFieldStyle())
                    Button("Connect") {
                        do { try BrowserBridge.installExtension(id: extensionID.trimmingCharacters(in: .whitespacesAndNewlines), browser: extensionBrowser); extensionMessage = "Connected. Use the Pickle button in your browser." }
                        catch { extensionMessage = error.localizedDescription }
                    }
                    if !extensionMessage.isEmpty { Text(extensionMessage).font(.caption) }
                }
                Text("Video context uses captions first. Listen for 30 seconds records source-app audio only when you choose it; local speech recognition must be available.").font(.caption).foregroundStyle(.secondary)
            }
            GlassSection("Your session") {
                Text("Passages and answers stay in memory until you clear them or quit. Only answers you explicitly bookmark are saved on this Mac.").font(.callout).foregroundStyle(.secondary)
                Button("Clear current passage and answers") { coordinator.clear() }
                Button("Ask before sharing again") { settings.cloudConsent = false; settings.jevConsent = false; settings.screenContextConsent = false; settings.webContextConsent = false; settings.localConsentServer = "" }
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
            GlassSection("AI location") {
                Picker("Use", selection: $settings.aiMode) { ForEach([AIMode.local, .online], id: \.self) { Text($0.rawValue).tag($0) } }.pickerStyle(.segmented)
                Text(settings.aiLocationDescription).font(.callout).foregroundStyle(.secondary)
                Text("Local is the default. Your choice is remembered. Switching never starts an answer automatically.").font(.caption).foregroundStyle(.secondary)
            }
            if settings.aiMode == .local {
                GlassSection("Your Local server") {
                    TextField("Server address", text: $settings.localAddress)
                    Toggle("This server runs on this Mac", isOn: $settings.localSameDevice)
                    Text("Leave off for Ross or an SSH tunnel to another computer.").font(.caption).foregroundStyle(.secondary)
                    TextField("Text model", text: $settings.localModel)
                    if !coordinator.localModels.isEmpty {
                        Menu("Choose an installed model") { ForEach(coordinator.localModels, id: \.self) { name in Button(name) { settings.localModel = name } } }
                    }
                    SecureField("Server access key (optional)", text: $localToken)
                    Button("Save Local access key") {
                        do { try CredentialStore.save(localToken, account: settings.localCredentialAccount); localToken = ""; coordinator.localConnectionStatus = "Access key saved in Keychain." }
                        catch { coordinator.localConnectionStatus = error.localizedDescription }
                    }.disabled(localToken.isEmpty)
                    Button("Test connection", action: coordinator.testLocalConnection)
                    if settings.usesRossTunnel {
                        Button("Reconnect to Ross", action: coordinator.reconnectRoss)
                    }
                    Text(coordinator.localConnectionStatus).font(.caption).foregroundStyle(.secondary)
                    DisclosureGroup("Optional vision model") {
                        TextField("Local vision model (leave blank to disable)", text: $settings.localVisionModel)
                        Text("Llama 3.2 1B is text-only. Configure a separately installed vision model only if your server supports it. Pickle never downloads models or falls back to cloud vision.").font(.caption).foregroundStyle(.secondary)
                    }
                    Text("8K context, up to 1K output tokens, one request at a time and a two-minute keep-alive. Long selections are rejected; supplementary context uses relevant excerpts. Charts keep their evidence checks and may exceed this small model’s ability.").font(.caption).foregroundStyle(.secondary)
                }
            }
            DisclosureGroup("Online account") {
            GlassSection("Connect your account") {
                Text("Online uses Cloudflare to create explanations. These details stay saved when you use Local.").font(.callout).foregroundStyle(.secondary)
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
}
