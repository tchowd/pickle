import AppKit

// A separate, controlled source application for real cross-process AX testing.
// It owns no user data and neither requests permissions nor captures other apps.
@MainActor final class FixtureDelegate: NSObject, NSApplicationDelegate {
  var window: NSWindow?
  func applicationDidFinishLaunching(_ notification: Notification) {
    let passage = "Café researchers say the treatment might help some people, but the evidence remains limited. Larger studies are needed — this is not a proven cure."
    let source = "BEFORE: this surrounding text must not become the selected passage.\n\n"
      + passage + "\n\nAFTER: this surrounding text must not become the selected passage."
    let window = NSWindow(
      contentRect: NSRect(x: 180, y: 180, width: 700, height: 360),
      styleMask: [.titled, .closable, .miniaturizable, .resizable],
      backing: .buffered, defer: false)
    window.title = "Pickle capture fixture — controlled text"
    let scroll = NSScrollView(frame: window.contentView!.bounds)
    scroll.autoresizingMask = [.width, .height]
    scroll.hasVerticalScroller = true
    let editor = NSTextView(frame: scroll.bounds)
    editor.autoresizingMask = [.width, .height]
    editor.isRichText = false
    editor.font = .systemFont(ofSize: 19)
    editor.textContainerInset = NSSize(width: 20, height: 20)
    editor.string = source
    editor.setAccessibilityIdentifier("capture-fixture-passage")
    editor.setAccessibilityLabel("Controlled passage for Pickle capture tests")
    scroll.documentView = editor
    window.contentView = scroll
    window.makeKeyAndOrderFront(nil)
    window.makeFirstResponder(editor)
    editor.setSelectedRange((source as NSString).range(of: passage))
    self.window = window
    NSApp.activate(ignoringOtherApps: true)
  }
  func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

MainActor.assumeIsolated {
  let app = NSApplication.shared
  app.setActivationPolicy(.regular)
  let menu = NSMenu()
  let appItem = NSMenuItem()
  let appMenu = NSMenu()
  appMenu.addItem(withTitle: "Quit fixture", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
  appItem.submenu = appMenu
  menu.addItem(appItem)
  app.mainMenu = menu
  let delegate = FixtureDelegate()
  app.delegate = delegate
  app.run()
}
