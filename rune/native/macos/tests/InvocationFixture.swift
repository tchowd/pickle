// Exercises fake observer/hotkey resources only. No NSApplication, global
// monitor, permission request, keyboard input, or window is created.
import Foundation

@main struct InvocationFixture {
  static func main() throws {
    try runInvocationTransactionFixture()
    print("macOS invocation transaction fixture passed")
  }
}
