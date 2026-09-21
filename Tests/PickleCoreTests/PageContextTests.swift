import Foundation
import PickleCore

final class PageContextTests: CheckSuite {
    func testPageContextBudgetsAndUnicode() {
        let input = String(repeating: "🌍 café\n", count: 2000)
        let bounded = PageContextLimits.bounded(input)
        expectTrue(bounded.utf8.count <= PageContextLimits.textBytes)
        expectTrue(input.hasPrefix(bounded))
        let snapshot = SelectionSnapshot(text: "Selected passage", appName: "Test", bundleID: "test")
        expectThrows(try RequestInput(snapshot: snapshot, action: .simplify, pageContext: String(repeating: "a", count: 6001)).validate())
        expectThrows(try RequestInput(snapshot: snapshot, action: .simplify, visualContext: String(repeating: "a", count: 3001)).validate())
    }
    func testOCRIsLabeledAndVisualSummaryIsNotEvidence() async throws {
        let writer = MockWriter(["An explanation."])
        let input = RequestInput(snapshot: .init(text: "This is the passage.", appName: "Test", bundleID: "test"), action: .simplify,
                                 pageContext: "A visible heading", visualContext: "A possible visual relationship")
        _ = try await RequestPipeline(provider: writer).run(input)
        let prompts = await writer.prompts
        let body = try JSONSerialization.jsonObject(with: Data(prompts[0].user.utf8)) as! [String: String]
        expectTrue(body["source"]!.contains("automatic OCR"))
        expectTrue(body["source"]!.contains("A visible heading"))
        expectTrue(!body["source"]!.contains("possible visual"))
        expectEqual(body["visual_context_summary"], "A possible visual relationship")
        expectTrue(prompts[0].system.contains("never source evidence"))
    }
    func testScreenNumbersCannotCreateAChart() async throws {
        let writer = MockWriter(["unused"])
        let input = RequestInput(snapshot: .init(text: "A passage with no numbers.", appName: "Test", bundleID: "test"), action: .chart,
                                 chartChoice: .bar, pageContext: "$10 and $20", visualContext: "$30 and $40")
        do { _ = try await RequestPipeline(provider: writer).run(input); fail("Screen context must not create chart evidence") } catch {}
        let calls = await writer.count
        expectEqual(calls, 0)
    }
    func testVisionRequestContainsOneBoundedDataImage() async throws {
        let transport = MockTransport(#"{"success":true,"result":{"response":"A chart compares two bars."}}"#)
        let client = VisualContextClient(accountID: String(repeating: "a", count: 32), token: "fixture", transport: transport)
        let jpeg = Data([255, 216, 255, 217])
        let summary = try await client.summarize(jpeg: jpeg, selection: "Selected text")
        expectEqual(summary, "A chart compares two bars.")
        let request = await transport.requests.first!
        expectTrue(request.url!.path.hasSuffix(VisualContextClient.model))
        let body = try JSONSerialization.jsonObject(with: request.httpBody!) as! [String: Any]
        let messages = body["messages"] as! [[String: Any]]
        let parts = messages[1]["content"] as! [[String: Any]]
        expectEqual(parts.count, 2)
        expectEqual((parts[1]["image_url"] as! [String: String])["url"], "data:image/jpeg;base64," + jpeg.base64EncodedString())
        expectEqual(body["stream"] as? Bool, false)
        do { _ = try await client.summarize(jpeg: Data(repeating: 0, count: PageContextLimits.imageBytes + 1), selection: "Text"); fail("Oversize image must fail") } catch {}
        let calls = await transport.requests.count
        expectEqual(calls, 1)
    }
}
