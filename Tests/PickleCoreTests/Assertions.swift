import Foundation
// Tiny dependency-free runner so checks work with macOS Command Line Tools alone.
// Tests execute serially; actors in fixtures exercise asynchronous provider behavior.
class CheckSuite {}
enum Checks { static var failures = 0; static var tests = 0 }
func fail(_ message: String = "Check failed", file: StaticString = #fileID, line: UInt = #line) { Checks.failures += 1; print("\(file):\(line): \(message)") }
func expectTrue(_ condition: @autoclosure () -> Bool, file: StaticString = #fileID, line: UInt = #line) { if !condition() { fail("Expected true", file: file, line: line) } }
func expectEqual<T: Equatable>(_ a: @autoclosure () -> T, _ b: @autoclosure () -> T, file: StaticString = #fileID, line: UInt = #line) { if a() != b() { fail("Values differ", file: file, line: line) } }
func expectNil<T>(_ value: @autoclosure () -> T?, file: StaticString = #fileID, line: UInt = #line) { if value() != nil { fail("Expected nil", file: file, line: line) } }
func expectThrows<T>(_ expression: @autoclosure () throws -> T, file: StaticString = #fileID, line: UInt = #line) { do { _ = try expression(); fail("Expected an error", file: file, line: line) } catch {} }
