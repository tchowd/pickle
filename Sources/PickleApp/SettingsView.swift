import SwiftUI
import PickleCore
struct SettingsView: View {
    @ObservedObject var settings: SettingsStore
    @ObservedObject var coordinator: RequestCoordinator
    @State private var token = ""
    @State private var keyStatus = ""
    @State private var testing = false
    @State private var diagnosticSummary = "Content-free counters are kept in memory. No selections, answers, titles, or tokens are logged."
    @State private var hasPermission = SelectionService.trusted
    var body: some View {
        Form {
            Section {
                Text("Highlight → invoke → understand").font(.title2.weight(.semibold))
                Text("Pickle works beside your source app. Configure Cloudflare, choose whether to use Jev, then try a selected passage with your shortcut.").foregroundStyle(.secondary)
            }
            Section("1 · Selection access") {
                Label(hasPermission ? "Accessibility granted" : "Accessibility not granted", systemImage: hasPermission ? "checkmark.circle" : "hand.raised")
                Text("This lets Pickle read selected text in supported apps. It never edits your source. No Screen Recording permission is requested.").font(.caption).foregroundStyle(.secondary)
                HStack { Button("Request Accessibility access") { SelectionService.requestPermission() }; Button("Refresh status") { hasPermission = SelectionService.trusted } }
            }
            Section("2 · Generative model") {
                LabeledContent("Provider", value: "Cloudflare Workers AI")
                TextField("Account ID", text: $settings.accountID)
                TextField("Writing model", text: $settings.model)
                Text("Default: Llama 3.3 70B Instruct FP8 Fast").font(.caption).foregroundStyle(.secondary)
                LabeledContent("Diagram model", value: "Llama 3.3 70B")
                Text("Diagrams use a separate model with structured output support. Simplify, Expand, and follow-ups use your writing model.").font(.caption).foregroundStyle(.secondary)
                SecureField("Cloudflare API token", text: $token)
                HStack {
                    Button("Save token to Keychain") { do { try CredentialStore.save(token.trimmingCharacters(in: .whitespacesAndNewlines)); token = ""; keyStatus = "Saved in Keychain" } catch { keyStatus = error.localizedDescription } }.disabled(token.isEmpty)
                    Button("Remove saved token") { do { try CredentialStore.save(""); keyStatus = "Credential removed" } catch { keyStatus = error.localizedDescription } }
                }
                Button("Authorize saved credential…") {
                    Task {
                        do {
                            let present = try await Task.detached { !(try CredentialStore.read(interactive: true)).isEmpty }.value
                            keyStatus = present ? "Keychain access authorized" : "No saved credential"
                        } catch { keyStatus = error.localizedDescription }
                    }
                }
                if !keyStatus.isEmpty { Text(keyStatus).font(.caption) }
                Text("Use a personal Workers AI API token. Wrangler CLI setup is documented in the project. The token is shared by both Cloudflare routes and never embedded in the app.").font(.caption).foregroundStyle(.secondary)
                Link("Create a Workers AI token", destination: URL(string: "https://dash.cloudflare.com/?to=/:account/ai/workers-ai")!)
            }
            Section("3 · Jev decision assistance") {
                Toggle("Enable Jev via Cloudflare", isOn: $settings.jevEnabled).disabled(settings.localOnly)
                LabeledContent("Decision model", value: "typesafe/jev")
                Text("Jev chooses chart formats, detects missing context, assesses passage difficulty, checks simplification fidelity, and checks expansion support. The writing model still produces every explanation.").font(.caption).foregroundStyle(.secondary)
                Text(coordinator.jevStatus).font(.caption)
                Button(testing ? "Testing…" : "Test Jev with a small sample (billable)") {
                    testing = true
                    Task {
                        do {
                            let result = try await JevClient(accountID: settings.accountID, token: CredentialStore.read()).checkConnection()
                            coordinator.jevStatus = "Connected: \(result) via Cloudflare"
                        } catch { coordinator.jevStatus = (error as? PickleError)?.localizedDescription ?? "Connection failed. Check credentials and model availability." }
                        testing = false
                    }
                }.disabled(testing || settings.localOnly || settings.paused)
                Text("This explicit test sends only ‘The sky is blue.’ to Cloudflare/TypeSafe. It may incur a small charge. Core explanations still work if Jev is unavailable.").font(.caption).foregroundStyle(.secondary)
            }
            Section("4 · Reading and invocation") {
                Picker("Explanation level", selection: $settings.level) { ForEach(ReadingLevel.allCases, id: \.self) { Text($0.rawValue).tag($0) } }
                Toggle("Double-tap Option to open Pickle", isOn: $settings.doubleOption)
                Text("Highlight text, tap and release Option twice, and Pickle automatically imports the selection. Check its preview, then choose Simplify, Expand, or Chart. No text is sent to a model until you choose an action. Accessibility access is required.").font(.caption).foregroundStyle(.secondary)
                if !settings.doubleOption {
                    Picker("Shortcut modifiers", selection: $settings.shortcutModifiers) { Text("Control + Option").tag("Control + Option"); Text("Command + Shift").tag("Command + Shift") }
                    Picker("Shortcut key", selection: $settings.shortcutKey) { ForEach(["P", "K", "Space", "Return"], id: \.self) { Text($0).tag($0) } }
                }
                Toggle("Show a small menu after highlighting", isOn: $settings.automaticMenu)
                Text("Optional. Only checks for a selection after mouse release. No model requests begin until you choose an action. Keyboard selection works with the shortcut.").font(.caption).foregroundStyle(.secondary)
            }
            Section("Diagnostics") {
                Text(diagnosticSummary).font(.caption).foregroundStyle(.secondary)
                Button("Refresh session metrics") { Task { diagnosticSummary = await coordinator.metrics.summary() } }
            }
            Section("5 · Privacy and exclusions") {
                Toggle("Local-only mode (offline sample in this build)", isOn: $settings.localOnly)
                Text("Blocks all cloud generation and Jev calls. A local AI provider is not included yet. The sample uses a fixed bundled answer.").font(.caption).foregroundStyle(.secondary)
                Text("Excluded application bundle IDs · one per line").font(.caption)
                TextEditor(text: $settings.exclusions).font(.system(.caption, design: .monospaced)).frame(height: 70)
                Text("Source content stays in memory until you clear the session or quit. Cloud actions disclose text to Cloudflare; Jev requests are routed to TypeSafe. Generated answers may also be sent for checks. Persistent history is not enabled.").font(.caption).foregroundStyle(.secondary)
                Button("Reset cloud disclosures") { settings.cloudConsent = false; settings.jevConsent = false }
                Button("Clear current session") { coordinator.clear() }
                Toggle("Pause Pickle", isOn: $settings.paused)
            }
        }.formStyle(.grouped).padding(8).frame(minWidth: 540)
    }
}
