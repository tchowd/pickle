import Foundation
import WebKit
import Darwin
import PickleCore

private final class ReferenceRedirects: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) { completionHandler(nil) }
}

@MainActor final class WebReferenceService: NSObject, WKNavigationDelegate {
    private var web: WKWebView?
    private var continuation: CheckedContinuation<[String: String], Error>?
    private var expiry: Task<Void, Never>?

    static func fetch(_ raw: String, selection: String) async throws -> WebReference {
        guard let url = WebReference.publicURL(raw) else { throw PickleError.message("This address can’t be used as a page reference.") }
        // DNS lookup is off the UI thread. Private/local destinations are never fetched.
        let allowed = await Task.detached { publicHost(url.host!) }.value
        guard allowed else { throw PickleError.message("This address isn’t a public webpage.") }
        try Task.checkCancellation()
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 4; config.timeoutIntervalForResource = 5
        config.httpCookieStorage = nil; config.urlCache = nil; config.httpShouldSetCookies = false
        let session = URLSession(configuration: config, delegate: ReferenceRedirects(), delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        var request = URLRequest(url: url)
        request.setValue("text/html,application/xhtml+xml", forHTTPHeaderField: "Accept")
        let (stream, response) = try await session.bytes(for: request)
        guard let response = response as? HTTPURLResponse, response.statusCode == 200,
              ["text/html", "application/xhtml+xml"].contains(response.mimeType ?? ""),
              response.expectedContentLength <= 2_000_000 else {
            throw PickleError.message("This page needs browser access. Use the Pickle browser extension.")
        }
        var data = Data()
        for try await byte in stream {
            guard data.count < 2_000_000 else { throw PickleError.message("This page is too large. Use the browser extension.") }
            data.append(byte)
        }
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
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        Task {
            do {
                let source = try String(contentsOf: Bundle.module.url(forResource: "Readability", withExtension: "js", subdirectory: "Resources")!)
                let result = try await webView.evaluateJavaScript(source + "\n(() => { const a = new Readability(document.cloneNode(true), {maxElemsToParse: 20000}).parse(); return {title: a?.title || document.title || '', text: a?.textContent || document.body?.innerText || ''}; })()")
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
    nonisolated static func publicHost(_ host: String) -> Bool {
        var hints = addrinfo(); hints.ai_family = AF_INET; hints.ai_socktype = SOCK_STREAM
        var result: UnsafeMutablePointer<addrinfo>?
        guard getaddrinfo(host, nil, &hints, &result) == 0, let first = result else { return false }
        defer { freeaddrinfo(first) }
        var cursor: UnsafeMutablePointer<addrinfo>? = first
        while let entry = cursor {
            guard let address = entry.pointee.ai_addr else { return false }
            let number = address.withMemoryRebound(to: sockaddr_in.self, capacity: 1) { UInt32(bigEndian: $0.pointee.sin_addr.s_addr) }
            let a = number >> 24, b = (number >> 16) & 255
            guard a != 0, a != 10, a != 127, a < 224,
                  !(a == 169 && b == 254), !(a == 172 && (16...31).contains(b)),
                  !(a == 192 && (b == 168 || b == 0)), !(a == 100 && (64...127).contains(b)),
                  !(a == 198 && (18...19).contains(b)) else { return false }
            cursor = entry.pointee.ai_next
        }
        return true
    }
}
