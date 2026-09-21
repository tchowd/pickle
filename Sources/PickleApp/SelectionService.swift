import AppKit
import ApplicationServices
import PickleCore

@MainActor struct SelectionService {
    static var trusted: Bool { AXIsProcessTrusted() }
    static func requestPermission() { AXIsProcessTrustedWithOptions([kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary) }
    func capture(excluded: (String) -> Bool) throws -> SelectionSnapshot {
        guard let app = NSWorkspace.shared.frontmostApplication, app.processIdentifier != ProcessInfo.processInfo.processIdentifier else { throw PickleError.message("Select text in another app, then use the shortcut. You can also paste text here.") }
        let bundle = app.bundleIdentifier ?? "unknown"
        guard !excluded(bundle) else { throw PickleError.message("Pickle is excluded from this application. Update exclusions in Settings to use it here.") }
        guard Self.trusted else { throw PickleError.message("Accessibility access is needed to read a selection. Grant access in Settings, or paste text manually.") }
        let root = AXUIElementCreateApplication(app.processIdentifier)
        AXUIElementSetMessagingTimeout(root, 0.35)
        guard let focused = attribute(root, kAXFocusedUIElementAttribute) else { throw PickleError.message("This app does not expose a focused text selection. Paste the passage manually.") }
        guard CFGetTypeID(focused) == AXUIElementGetTypeID() else { throw PickleError.message("Unsupported focused element. Paste the passage manually.") }
        let element = focused as! AXUIElement
        // Fail closed on known secure roles, including secure ancestors. Never use AXValue/document text as fallback.
        var ancestor: AXUIElement? = element
        for _ in 0..<12 {
            guard let current = ancestor else { break }
            let role = attribute(current, kAXRoleAttribute) as? String ?? ""
            let subrole = attribute(current, kAXSubroleAttribute) as? String ?? ""
            guard !role.lowercased().contains("secure"), !subrole.lowercased().contains("secure"), (attribute(current, "AXProtectedContent") as? Bool) != true else { throw PickleError.message("Pickle does not capture secure or protected fields.") }
            if let parent = attribute(current, kAXParentAttribute), CFGetTypeID(parent) == AXUIElementGetTypeID() { ancestor = (parent as! AXUIElement) } else { break }
        }
        guard let text = attribute(element, kAXSelectedTextAttribute) as? String, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw PickleError.message("No usable selection was exposed by \(app.localizedName ?? "this app"). Try selecting text again, or paste it manually.") }
        guard text.utf8.count <= Limits.selection else { throw PickleError.message("This selection is too long. Select at most \(Limits.selection.formatted()) UTF-8 bytes.") }
        var bounds: SelectionBounds?
        if let range = attribute(element, kAXSelectedTextRangeAttribute) {
            var raw: CFTypeRef?
            if AXUIElementCopyParameterizedAttributeValue(element, kAXBoundsForRangeParameterizedAttribute as CFString, range, &raw) == .success,
               let raw, CFGetTypeID(raw) == AXValueGetTypeID() {
                var rect = CGRect.zero
                if AXValueGetValue(raw as! AXValue, .cgRect, &rect), !rect.isEmpty, rect.minX.isFinite, rect.minY.isFinite {
                    bounds = .init(x: rect.minX, y: rect.minY, width: rect.width, height: rect.height)
                }
            }
        }
        return SelectionSnapshot(text: text, appName: app.localizedName ?? "Application", bundleID: bundle, bounds: bounds, limitations: bounds == nil ? ["Selection coordinates unavailable; positioned near the pointer."] : [])
    }
    private func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
        var value: CFTypeRef?
        return AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success ? value : nil
    }
}
