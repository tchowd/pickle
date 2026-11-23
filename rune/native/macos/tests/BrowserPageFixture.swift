// Pure observations and capture counters only. No real AX/CG/permission query.
import AppKit
@main struct BrowserPageFixture {
  static func main() async throws {
    precondition(selectionAXTimeout(deadline:100,now:99) == 0.08)
    precondition(abs(Double(selectionAXTimeout(deadline:100,now:99.98)!) - 0.02) < 0.00001)
    precondition(selectionAXTimeout(deadline:100,now:100) == nil)
    precondition(selectionAXTimeout(deadline:100,now:101) == nil)
    let flight = BrowserPageFlight()
    precondition(flight.acquire())
    precondition(!flight.acquire())
    flight.release()
    precondition(flight.acquire())
    flight.release()
    let url = "https://example.com/Path%2Fpart?q=Caf%C3%A9&b=2#route-one"
    precondition(browserDocumentURLsMatch(url,url))
    for bad in [url.replacingOccurrences(of:"#route-one",with:"#route-two"), url.replacingOccurrences(of:"%2F",with:"/"), url.replacingOccurrences(of:"?q=Caf%C3%A9&b=2",with:"?b=2&q=Caf%C3%A9"),"https://example.com/\n","https://user:pass@example.com/"] {
      precondition(!browserDocumentURLsMatch(url,bad))
    }
    let frame = CGRect(x:20,y:30,width:700,height:400)
    let one = BrowserPageWindow(pid:42,id:43,bounds:frame,eligible:true)
    precondition(uniqueBrowserCaptureWindow(pid:42,id:43,axBounds:frame,windows:[one]))
    precondition(!uniqueBrowserCaptureWindow(pid:42,id:43,axBounds:frame,windows:[one,one]))
    precondition(!uniqueBrowserCaptureWindow(pid:42,id:43,axBounds:frame,windows:[BrowserPageWindow(pid:42,id:44,bounds:frame,eligible:true)]))
    precondition(!uniqueBrowserCaptureWindow(pid:42,id:43,axBounds:frame,windows:[BrowserPageWindow(pid:42,id:43,bounds:frame,eligible:false)]))
    for mode in 0..<6 {
      var reads = 0; var captures = 0; var cancelled = false
      do {
        let result = try await verifiedBrowserCapture(pid:42,window:43,expected:url,cancelled:{cancelled},observe:{
          reads += 1
          if mode == 4 { cancelled = true }
          if mode == 5 { throw BrowserPageError.unavailable }
          return BrowserDocumentObservation(pid:42,window:43,identity:mode == 2 && reads > 1 ? 2 : 1,url:mode == 1 || (mode == 3 && reads > 2) ? "https://example.com/other" : url)
        },capture:{_ in captures += 1; return 99})
        precondition(mode == 0 && result == 99 && captures == 1)
      } catch {
        precondition(mode != 0)
        precondition(captures == (mode == 3 ? 1 : 0))
      }
    }
    print("macOS synthetic browser document URL/window fixture passed (no AX/CG/permissions/pixels queried)")
  }
}
