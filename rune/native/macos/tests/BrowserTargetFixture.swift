// Pure injected metadata. Never enumerates real apps/windows or creates an app.
import AppKit

@main struct BrowserTargetFixture {
  static func main() {
    let chrome = BrowserSourceApp(pid: 200, bundle: "com.google.Chrome", name: "Owned fixture")
    let edge = BrowserSourceApp(pid: 201, bundle: "com.microsoft.edgemac", name: "Other fixture")
    let visible = BrowserSourceWindow(id: 42, pid: 200, layer: 0, alpha: 1, bounds: CGRect(x: 0, y: 0, width: 700, height: 400))
    func bind(_ app: BrowserSourceApp?, _ windows: [BrowserSourceWindow], own: Int32 = 100) -> [String:Any]? {
      bindBrowserSource(expected: chrome.bundle, ownPID: own, foreground: { app }, windows: { windows })
    }
    let valid = bind(chrome, [visible])!
    precondition(valid["source_pid"] as? Int32 == 200 && valid["source_window"] as? UInt32 == 42)
    precondition(valid["text"] as? String == "" && valid["source_url"] is NSNull && valid["bounds"] is NSNull)
    precondition(bind(edge, [visible]) == nil)
    precondition(bind(chrome, [visible], own: 200) == nil)
    precondition(bind(nil, [visible]) == nil && bind(chrome, []) == nil)
    for bad in [
      BrowserSourceWindow(id: 0, pid: 200, layer: 0, alpha: 1, bounds: visible.bounds),
      BrowserSourceWindow(id: 42, pid: 999, layer: 0, alpha: 1, bounds: visible.bounds),
      BrowserSourceWindow(id: 42, pid: 200, layer: 2, alpha: 1, bounds: visible.bounds),
      BrowserSourceWindow(id: 42, pid: 200, layer: 0, alpha: 0, bounds: visible.bounds),
      BrowserSourceWindow(id: 42, pid: 200, layer: 0, alpha: 1, bounds: .zero),
    ] { precondition(bind(chrome, [bad]) == nil) }
    var reads = 0
    precondition(bindBrowserSource(expected: chrome.bundle, ownPID: 100, foreground: {
      reads += 1; return reads == 1 ? chrome : edge
    }, windows: { [visible] }) == nil)
    reads = 0
    precondition(bindBrowserSource(expected: chrome.bundle, ownPID: 100, foreground: { chrome }, windows: {
      reads += 1; return reads == 1 ? [visible] : []
    }) == nil)
    precondition(bindBrowserSource(expected: "page-supplied identity", ownPID: 100, foreground: {
      preconditionFailure("Unregistered identity must be rejected before metadata access")
    }, windows: { [] }) == nil)
    print("macOS synthetic browser target fixture passed")
  }
}
