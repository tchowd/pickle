import Foundation
import PickleCore

final class WebReferenceTests: CheckSuite {
    func testReferenceURLAndBudgets() throws {
        for raw in ["file:///etc/passwd", "https://user:pass@example.com", "http://127.0.0.1", "http://localhost", "http://service.local", "https://example.com:9999", "javascript:alert(1)", "http://[::1]"] {
            expectTrue(WebReference.publicURL(raw) == nil)
        }
        expectEqual(WebReference.publicURL("https://example.com/article#part")?.absoluteString, "https://example.com/article")
        let valid = WebReference(url: "https://example.com", title: "Title", text: "Page text")
        try valid.validate()
        for invalid in [WebReference(url: "https://example.com", title: "Title", text: ""),
                        WebReference(url: "https://example.com", title: "Title", text: String(repeating: "x", count: WebReference.budget+1)),
                        WebReference(url: "https://example.com", title: "Title", text: "Video", kind: "video captions", timestamp: .nan)] {
            do { try invalid.validate(); fail("Invalid reference accepted") } catch {}
        }
    }
    func testReferenceExcerptsAndPageOnlyInput() throws {
        expectEqual(WebReference.excerpt("", selection: "test"), "")
        let text = String(repeating: "Unrelated paragraph.\n", count: 100) + "The selected mitochondrial process uses energy.\nMore information."
        let excerpt = WebReference.excerpt(text, selection: "mitochondrial", budget: 100)
        expectTrue(excerpt.contains("mitochondrial")); expectTrue(excerpt.utf8.count <= 100)
        let unicode = WebReference.excerpt(String(repeating: "🌍", count: 9000), selection: "")
        expectTrue(unicode.utf8.count <= WebReference.budget)
        let reference = WebReference(url: "https://example.com", title: "Title", text: excerpt)
        try RequestInput(snapshot: .init(text: "", appName: "Browser", bundleID: "browser"), action: .simplify, reference: reference).validate()
    }
    func testReferencesReachProseButNotChartEvidence() async throws {
        let writer = MockWriter()
        let reference = WebReference(url: "https://example.com", title: "Title", text: "Revenue was $20 and $40.")
        let snapshot = SelectionSnapshot(text: "Revenue increased.", appName: "Browser", bundleID: "browser")
        _ = try await RequestPipeline(provider: writer).run(.init(snapshot: snapshot, action: .simplify, reference: reference))
        let prompts = await writer.prompts
        expectTrue(prompts[0].user.contains("Revenue was $20"))
        do {
            _ = try await RequestPipeline(provider: writer).run(.init(snapshot: snapshot, action: .chart, chartChoice: .bar, reference: reference))
            fail("Reference numbers must not become selected evidence")
        } catch {}
        let count = await writer.count; expectEqual(count, 1)
    }
}
