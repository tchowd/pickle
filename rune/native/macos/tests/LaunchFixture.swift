import AppKit

@main struct LaunchFixture {
  static func main() {
    // Construct the native alert without showing it or entering a modal loop.
    NSApplication.shared.setActivationPolicy(.prohibited)
    var presentations = 0
    precondition(PickleLaunchFailure.present(available: { true }, modal: { alert in
      presentations += 1
      precondition(alert.messageText == "Pickle could not open the reader")
      precondition(alert.informativeText == PickleLaunchFailure.message)
      precondition(alert.alertStyle == .critical)
      precondition(alert.buttons.count == 1 && alert.buttons[0].title == "Quit Pickle")
    }))
    precondition(presentations == 1)
    precondition(!PickleLaunchFailure.present(available: { false }, modal: { _ in
      preconditionFailure("No window server must not create a dialog")
    }))
    let offMain = DispatchSemaphore(value: 0)
    DispatchQueue.global().async {
      precondition(!PickleLaunchFailure.present(available: { true }, modal: { _ in
        preconditionFailure("An off-main caller must not touch AppKit")
      }))
      offMain.signal()
    }
    precondition(offMain.wait(timeout: .now() + 2) == .success)
    print("Native launch alert fixture passed (no dialog displayed).")
  }
}
