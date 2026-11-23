import AppKit
import CoreGraphics

// Independent of Rune, Metal, source text, credentials and application delegates.
enum PickleLaunchFailure {
  static let title = "Pickle could not open the reader"
  static let message = "Restart Pickle. If the problem continues, restart your computer and check for operating system and graphics-driver updates."

  static func hasWindowServer() -> Bool {
    CGSessionCopyCurrentDictionary() != nil
  }

  static func present(
    available: () -> Bool = hasWindowServer,
    modal: (NSAlert) -> Void = { alert in
      NSApplication.shared.activate(ignoringOtherApps: true)
      alert.runModal()
    }
  ) -> Bool {
    guard Thread.isMainThread, available() else { return false }
    let alert = NSAlert()
    alert.messageText = title
    alert.informativeText = message
    alert.alertStyle = .critical
    alert.addButton(withTitle: "Quit Pickle")
    modal(alert)
    return true
  }
}

@_cdecl("pickle_present_reader_failure")
public func picklePresentReaderFailure() -> Int32 {
  PickleLaunchFailure.present() ? 1 : 0
}
