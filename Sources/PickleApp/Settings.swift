import SwiftUI
import Security
import LocalAuthentication
import PickleCore

@MainActor final class SettingsStore: ObservableObject {
    @Published var paused: Bool { didSet { save("paused", paused) } }
    @Published var accountID: String { didSet { save("accountID", accountID) } }
    @Published var model: String { didSet { save("model", model) } }
    @Published var jevEnabled: Bool { didSet { save("jevEnabled", jevEnabled) } }
    @Published var automaticMenu: Bool { didSet { save("automaticMenu", automaticMenu) } }
    @Published var localOnly: Bool { didSet { save("localOnly", localOnly) } }
    @Published var level: ReadingLevel { didSet { save("level", level.rawValue) } }
    @Published var exclusions: String { didSet { save("exclusions", exclusions) } }
    @Published var shortcutKey: String { didSet { save("shortcutKey", shortcutKey) } }
    @Published var shortcutModifiers: String { didSet { save("shortcutModifiers", shortcutModifiers) } }
    @Published var controlOption: Bool { didSet { save("controlOption", controlOption) } }
    var shortcutLabel: String { controlOption ? "Control + Option" : "\(shortcutModifiers) + \(shortcutKey)" }
    @Published var cloudConsent: Bool { didSet { save("cloudConsentV2", cloudConsent) } }
    @Published var jevConsent: Bool { didSet { save("jevConsentV2", jevConsent) } }
    @Published var onboarded: Bool { didSet { save("onboarded", onboarded) } }
    @Published var textSize: Double { didSet { save("textSize", textSize) } }
    @Published var glassOpacity: Double { didSet { save("glassOpacity", glassOpacity) } }
    @Published var themeIntensity: Double { didSet { save("themeIntensity", themeIntensity) } }
    @Published var screenContextEnabled: Bool { didSet { save("screenContextEnabled", screenContextEnabled) } }
    @Published var screenContextConsent: Bool { didSet { save("screenContextConsent", screenContextConsent) } }
    @Published var webContextEnabled: Bool { didSet { save("webContextEnabled", webContextEnabled) } }
    @Published var webContextConsent: Bool { didSet { save("webContextConsent", webContextConsent) } }
    @Published var streaming: Bool { didSet { save("streaming", streaming) } }
    let defaults: UserDefaults
    var requestPolicy: [String] {
        [String(paused), accountID, model, String(jevEnabled), String(localOnly), level.rawValue,
         exclusions, String(cloudConsent), String(jevConsent), String(screenContextEnabled), String(screenContextConsent), String(webContextEnabled), String(webContextConsent)]
    }
    var floatingFrame: NSRect? {
        get { defaults.string(forKey: "floatingFrame").map(NSRectFromString) }
        set { defaults.set(newValue.map(NSStringFromRect), forKey: "floatingFrame") }
    }
    var readerFrame: NSRect? {
        get { defaults.string(forKey: "readerFrame").map(NSRectFromString) }
        set { defaults.set(newValue.map(NSStringFromRect), forKey: "readerFrame") }
    }
    var expandedSize: NSSize? {
        get { defaults.string(forKey: "expandedSize").map(NSSizeFromString) }
        set { defaults.set(newValue.map(NSStringFromSize), forKey: "expandedSize") }
    }
    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        webContextEnabled = defaults.bool(forKey: "webContextEnabled")
        webContextConsent = defaults.bool(forKey: "webContextConsent")
        screenContextEnabled = defaults.object(forKey: "screenContextEnabled") as? Bool ?? true
        screenContextConsent = defaults.bool(forKey: "screenContextConsent")
        textSize = min(24, max(14, defaults.object(forKey: "textSize") as? Double ?? 18))
        glassOpacity = min(1, max(0, defaults.object(forKey: "glassOpacity") as? Double ?? 0.12))
        themeIntensity = min(1, max(0, defaults.object(forKey: "themeIntensity") as? Double ?? 1))
        streaming = defaults.object(forKey: "streaming") as? Bool ?? true
        paused = defaults.bool(forKey: "paused")
        accountID = defaults.string(forKey: "accountID") ?? ""
        model = defaults.string(forKey: "model") ?? CloudflareProvider.defaultModel
        jevEnabled = defaults.bool(forKey: "jevEnabled"); automaticMenu = defaults.bool(forKey: "automaticMenu")
        localOnly = defaults.bool(forKey: "localOnly")
        level = ReadingLevel(rawValue: defaults.string(forKey: "level") ?? "") ?? .automatic
        exclusions = defaults.string(forKey: "exclusions") ?? "com.1password.1password\ncom.apple.keychainaccess"
        shortcutKey = defaults.string(forKey: "shortcutKey") ?? "P"
        shortcutModifiers = defaults.string(forKey: "shortcutModifiers") ?? "Control + Option"
        controlOption = defaults.object(forKey: "controlOption") as? Bool ?? true
        cloudConsent = defaults.bool(forKey: "cloudConsentV2"); jevConsent = defaults.bool(forKey: "jevConsentV2")
        onboarded = defaults.bool(forKey: "onboarded")
    }
    private func save(_ key: String, _ value: Any) { defaults.set(value, forKey: key) }
    func isExcluded(_ bundle: String) -> Bool { Set(exclusions.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces).lowercased() }).contains(bundle.lowercased()) }
}
struct CredentialStore {
    static let service = "com.pickle.reader.credentials"
    static func read(interactive: Bool = false) throws -> String {
        let authentication = LAContext(); authentication.interactionNotAllowed = !interactive
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "cloudflare", kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne, kSecUseAuthenticationContext as String: authentication]
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return "" }
        if status == errSecInteractionNotAllowed || status == errSecAuthFailed { throw PickleError.message("macOS needs permission to unlock Pickle’s credential. Use Authorize saved credential in Settings, or save a new token.") }
        guard status == errSecSuccess, let data = result as? Data, let token = String(data: data, encoding: .utf8) else { throw PickleError.message("Keychain could not read the Cloudflare credential (\(status)).") }
        return token
    }
    static func save(_ token: String) throws {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "cloudflare"]
        if token.isEmpty { let status = SecItemDelete(query as CFDictionary); guard status == errSecSuccess || status == errSecItemNotFound else { throw PickleError.message("Could not remove the credential (\(status)).") }; return }
        let value = [kSecValueData as String: Data(token.utf8)]
        var status = SecItemUpdate(query as CFDictionary, value as CFDictionary)
        if status == errSecItemNotFound {
            var item = query.merging(value) { _, new in new }; item[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            status = SecItemAdd(item as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw PickleError.message("Keychain could not save the credential (\(status)).") }
    }
}
