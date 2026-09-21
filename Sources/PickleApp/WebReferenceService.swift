import Foundation
import WebKit
import Darwin
import PickleCore

@MainActor final class WebReferenceService: NSObject, WKNavigationDelegate {
    private var web: WKWebView?
    private var continuation: CheckedContinuation<[String: String], Error>?
    private var startedNavigation = false
    private var expiry: Task<Void, Never>?

    static func fetch(_ raw: String, selection: String) async throws -> WebReference {
        guard let url = WebReference.publicURL(raw) else { throw PickleError.message("This address can’t be used as a page reference.") }
        // DNS lookup is off the UI thread. Private/local destinations are never fetched.
        let address = await Task.detached { publicAddress(url.host!) }.value
        guard let address else { throw PickleError.message("This address isn’t a public webpage.") }
        try Task.checkCancellation()
        let data = try await PublicPageDownload().fetch(url, address: address)
        try Task.checkCancellation()
        guard let html = String(data: data, encoding: .utf8) else { throw PickleError.message("Use the browser extension to read this page.") }
        let parser = WebReferenceService()
        let article = try await parser.extract(html, url: url)
        let text = WebReference.excerpt(article["text"] ?? "", selection: selection)
        let reference = WebReference(url: url.absoluteString, title: PageContextLimits.bounded(article["title"] ?? url.host!, bytes: 1000), text: text)
        try reference.validate()
        return reference
    }

    /// No remote resources or source-page scripts run in the extraction view.
    func extract(_ html: String, url: URL) async throws -> [String: String] {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        let rules = try await WKContentRuleListStore.default().compileContentRuleList(forIdentifier: "pickle-local-parser", encodedContentRuleList: "[{\"trigger\":{\"url-filter\":\".*\"},\"action\":{\"type\":\"block\"}}]")
        if let rules { config.userContentController.add(rules) }
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = self; web = view
        return try await withTaskCancellationHandler(operation: {
            try await withCheckedThrowingContinuation { continuation in
                self.continuation = continuation
                expiry = Task { [weak self] in
                    try? await Task.sleep(for: .seconds(3))
                    guard !Task.isCancelled else { return }
                    self?.finish(.failure(PickleError.message("Page extraction took too long.")))
                }
                view.loadHTMLString("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'none'; style-src 'none'\">" + html, baseURL: url)
            }
        }, onCancel: { Task { @MainActor [weak self] in self?.finish(.failure(CancellationError())) } })
    }
    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        guard !startedNavigation, navigationAction.targetFrame?.isMainFrame == true else { decisionHandler(.cancel); return }
        startedNavigation = true; decisionHandler(.allow)
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        Task {
            do {
                let packaged = Bundle.main.resourceURL.flatMap { Bundle(url: $0.appendingPathComponent("Pickle_PickleApp.bundle")) }
                guard let resource = (packaged ?? Bundle.module).url(forResource: "Readability", withExtension: "js", subdirectory: "Resources") else { throw PickleError.message("Page reader resource is missing.") }
                let source = try String(contentsOf: resource)
                let result = try await webView.evaluateJavaScript(source + "\n(() => { const clone = document.cloneNode(true); clone.querySelectorAll('script,style,input,textarea,[contenteditable],nav,footer').forEach(n => n.remove()); const a = new Readability(clone, {maxElemsToParse: 20000}).parse(); return {title: a?.title || document.title || '', text: a?.textContent || clone.querySelector('article,main')?.textContent || ''}; })()")
                guard let result = result as? [String: String] else { throw PickleError.message("No article text found.") }
                finish(.success(result))
            } catch { finish(.failure(error)) }
        }
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { finish(.failure(error)) }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { finish(.failure(error)) }
    private func finish(_ result: Result<[String: String], Error>) {
        let callback = continuation; continuation = nil
        expiry?.cancel(); expiry = nil; web?.stopLoading(); web?.navigationDelegate = nil; web = nil
        callback?.resume(with: result)
    }
    nonisolated static func publicAddress(_ host: String) -> String? {
        var hints = addrinfo(); hints.ai_family = AF_INET; hints.ai_socktype = SOCK_STREAM
        var result: UnsafeMutablePointer<addrinfo>?
        guard getaddrinfo(host, nil, &hints, &result) == 0, let first = result else { return nil }
        defer { freeaddrinfo(first) }
        var chosen: String?
        var cursor: UnsafeMutablePointer<addrinfo>? = first
        while let entry = cursor {
            guard let address = entry.pointee.ai_addr else { return nil }
            let number = address.withMemoryRebound(to: sockaddr_in.self, capacity: 1) { UInt32(bigEndian: $0.pointee.sin_addr.s_addr) }
            let a = number >> 24, b = (number >> 16) & 255
            guard a != 0, a != 10, a != 127, a < 224,
                  !(a == 169 && b == 254), !(a == 172 && (16...31).contains(b)),
                  !(a == 192 && (b == 168 || b == 0)), !(a == 100 && (64...127).contains(b)),
                  !(a == 198 && (18...19).contains(b)) else { return nil }
            if chosen == nil {
                chosen = "\(a).\(b).\((number >> 8) & 255).\(number & 255)"
            }
            cursor = entry.pointee.ai_next
        }
        return chosen
    }
}
