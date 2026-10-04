import Foundation
import PickleCore

final class AnswerMarkupTests: CheckSuite {
    func testListsAndStrongTextRenderWithoutSyntax() {
        let answer = """
        The design involves several key components:

        1.  **Model Inference**: The API will use a large language model.
        2.  **GPU Batching**: The model will be run on a pool of GPUs.
        10) Wide markers stay numbered
        """
        let blocks = AnswerMarkup.blocks(answer)
        expectEqual(blocks.count, 4)
        expectEqual(blocks[0].kind, .paragraph)
        expectEqual(blocks[1].kind, .numbered("1."))
        expectEqual(blocks[1].runs, [AnswerMarkup.Run("Model Inference", strong: true), AnswerMarkup.Run(": The API will use a large language model.")])
        expectEqual(blocks[3].kind, .numbered("10."))
        expectEqual(AnswerMarkup.plainText(answer), """
        The design involves several key components:

        1. Model Inference: The API will use a large language model.
        2. GPU Batching: The model will be run on a pool of GPUs.
        10. Wide markers stay numbered
        """)
    }
    func testBlockStructure() {
        let blocks = AnswerMarkup.blocks("## Overview ##\nFirst line\nsecond line\n\n- Parent\n  continued\n    * Child\n- Sibling\n\n---\n> Quoted\n> text\n```\nlet x = 1\n```\n```\nstill streaming")
        expectEqual(blocks.map(\.kind), [.heading(2), .paragraph, .bullet, .bullet, .bullet, .quote, .code, .code])
        expectEqual(blocks[0].text, "Overview")
        expectEqual(blocks[1].text, "First line\nsecond line")
        expectEqual(blocks[2].text, "Parent\ncontinued")
        expectEqual(blocks.map(\.depth), [0, 0, 0, 1, 0, 0, 0, 0])
        expectEqual(blocks[4].marker, "•"); expectEqual(blocks[3].marker, "◦")
        expectEqual(blocks[5].text, "Quoted\ntext")
        expectEqual(blocks[6].runs, [AnswerMarkup.Run("let x = 1", code: true)])
        expectEqual(blocks[7].text, "still streaming")
    }
    func testInlineSyntaxStaysInert() {
        let runs = AnswerMarkup.blocks("Use `snake_case` *carefully*, see [docs](https://example.com), keep <b>tags</b> and **unclosed")[0].runs
        expectEqual(runs.map(\.text).joined(), "Use snake_case carefully, see docs, keep <b>tags</b> and **unclosed")
        expectTrue(runs.contains(AnswerMarkup.Run("snake_case", code: true)))
        expectTrue(runs.contains(AnswerMarkup.Run("carefully", emphasis: true)))
        // Ordinary prose punctuation never becomes structure.
        expectEqual(AnswerMarkup.blocks("-5 degrees, #hashtag, 2024. A year\n2 * 3 = 6").map(\.kind), [.paragraph])
        expectEqual(AnswerMarkup.plainText(""), "")
    }
    func testAnnouncementPreambleIsRemoved() async throws {
        let answer = "Here is a simplified explanation of the problem statement:\n\nThe problem is to design an API."
        expectEqual(ResultValidator.withoutPreamble(answer), "The problem is to design an API.")
        for lead in ["Sure! Here's a simpler version:", "**Here is a brief summary of the passage:**", "Certainly, here are some examples:", "Below is an overview:"] {
            expectEqual(ResultValidator.withoutPreamble(lead + "\r\n- First point"), "- First point")
        }
        // Lead-ins that carry content, inline announcements and lone lines stay intact.
        for kept in ["Here are the three causes:\n1. Heat", "Here is a simplified explanation: it batches requests.\nMore.", "Here is a simplified explanation:", "The explanation has two parts:\nFirst."] {
            expectEqual(ResultValidator.withoutPreamble(kept), kept)
        }
        let writer = MockWriter([answer])
        let outcome = try await RequestPipeline(provider: writer).run(.init(snapshot: .init(text: "Design an API.", appName: "Test", bundleID: "test"), action: .simplify))
        guard case .result(let result) = outcome else { return fail("Expected a result") }
        expectEqual(result.text, "The problem is to design an API.")
        let prompts = await writer.prompts; expectTrue(prompts[0].system.contains("Begin directly with the answer"))
    }
}
