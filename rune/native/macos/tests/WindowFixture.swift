// Unshown test windows and in-memory mouse events only. The overridden drag
// methods record routing; they never start an actual Window Server drag.
import AppKit

@main struct WindowFixture {
  static func main() throws {
    let application = NSApplication.shared
    application.setActivationPolicy(.prohibited)
    try runWindowDragFixture()
    print("macOS panel drag forwarding fixture passed")
  }
}
