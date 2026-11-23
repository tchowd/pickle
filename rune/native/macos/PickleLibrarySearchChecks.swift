import Foundation

// Build with PickleLibrarySearch.swift only. No app/window, user archive,
// credentials, preferences writes, model, clipboard or permission operations.
@main
struct PickleLibrarySearchChecks {
  static func flags(_ index: UnsafeMutableRawPointer, _ query: String, _ count: Int) -> [UInt8] {
    let data = try! JSONEncoder().encode(query)
    var flags = [UInt8](repeating: 0, count: count)
    let success = data.withUnsafeBytes { bytes in
      flags.withUnsafeMutableBufferPointer { output in
        pickleLibrarySearchFind(
          index, bytes.bindMemory(to: UInt8.self).baseAddress, data.count,
          output.baseAddress, output.count)
      }
    }
    precondition(success == 1)
    return flags
  }
  static func token() -> String {
    let raw = pickleLibrarySearchLocale()!
    defer { pickleLibrarySearchStringFree(raw) }
    return String(cString: raw)
  }
  static func main() {
    let cases = [
      ("cafe\u{301}", "café"), ("Straße", "STRASSE"), ("ß", "s"), ("ß", "ss"),
      ("sß", "ss"), ("ﬀ", "ff"), ("café", "cafe"), ("café", "caf"),
      ("👩‍💻", "💻"), ("👨‍👩‍👧", "👩"), ("🇺🇸", "🇺"),
      ("क्ष", "ष"), ("क्ष", "क"), ("का", "क"), ("a\u{200d}", "a"),
      ("cafe\u{301}", "\u{301}"), ("é", "\u{301}"),
      ("alpha\nbeta", "alpha beta"), ("alpha\tbeta", "alpha beta"),
      ("alpha  beta", "alpha beta"), ("alpha\nbeta", "alpha\nbeta"),
      ("Ａ", "A"), ("left\0right", "right"), ("left\0right", "left\0right"),
    ]
    for (source, query) in cases {
      let rows = [[source], ["unrelated"], ["alpha", "beta"]]
      let data = try! JSONEncoder().encode(rows)
      let index = data.withUnsafeBytes {
        pickleLibrarySearchCreate($0.bindMemory(to: UInt8.self).baseAddress, data.count)
      }!
      let expected = rows.map { fields in
        fields.contains { $0.localizedCaseInsensitiveContains(query) } ? UInt8(1) : UInt8(0)
      }
      for _ in 0..<20 { precondition(flags(index, query, rows.count) == expected) }
      pickleLibrarySearchRelease(index)
    }
    // Same locale identifier may survive a notification. The token still
    // invalidates result caches. This posts only in this disposable process.
    let before = token()
    NotificationCenter.default.post(name: NSLocale.currentLocaleDidChangeNotification, object: nil)
    precondition(before != token())
    precondition(token() == token())
    print("PASS: \(cases.count) pure-string bridge/oracle cases and locale invalidation")
  }
}
