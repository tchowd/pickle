import Foundation

// Immutable, worker-safe text only. The retained object never creates or reads
// AppKit/Accessibility/native window state. ARC lifetime is owned by Rust.
private final class PickleLibraryText: Sendable {
  let rows: [[String]]
  init(_ rows: [[String]]) { self.rows = rows }
}

@_cdecl("pickle_library_search_create")
public func pickleLibrarySearchCreate(_ bytes: UnsafePointer<UInt8>?, _ length: Int)
  -> UnsafeMutableRawPointer?
{
  autoreleasepool {
    guard let bytes, length > 0,
      let rows = try? JSONDecoder().decode([[String]].self, from: Data(bytes: bytes, count: length))
    else { return nil }
    return Unmanaged.passRetained(PickleLibraryText(rows)).toOpaque()
  }
}

@_cdecl("pickle_library_search_release")
public func pickleLibrarySearchRelease(_ pointer: UnsafeMutableRawPointer?) {
  guard let pointer else { return }
  autoreleasepool { Unmanaged<PickleLibraryText>.fromOpaque(pointer).release() }
}

@_cdecl("pickle_library_search_find")
public func pickleLibrarySearchFind(
  _ pointer: UnsafeMutableRawPointer?, _ queryBytes: UnsafePointer<UInt8>?,
  _ queryLength: Int, _ matches: UnsafeMutablePointer<UInt8>?, _ count: Int
) -> Int32 {
  autoreleasepool {
    guard let pointer, let queryBytes, let matches, queryLength > 0 else { return 0 }
    let index = Unmanaged<PickleLibraryText>.fromOpaque(pointer).takeUnretainedValue()
    guard count == index.rows.count,
      let query = try? JSONDecoder().decode(
        String.self, from: Data(bytes: queryBytes, count: queryLength))
    else { return 0 }
    for (row, fields) in index.rows.enumerated() {
      // EXACT legacy comparison: preserve raw text, locale and separate fields.
      matches[row] = fields.contains { $0.localizedCaseInsensitiveContains(query) } ? 1 : 0
    }
    return 1
  }
}

private final class PickleLibraryLocaleEpoch: @unchecked Sendable {
  private let lock = NSLock()
  private var epoch: UInt64 = 0
  func advance() {
    lock.lock()
    epoch &+= 1
    lock.unlock()
  }
  func value() -> UInt64 {
    lock.lock()
    defer { lock.unlock() }
    return epoch
  }
}
private let pickleLibraryLocaleEpoch = PickleLibraryLocaleEpoch()
// One process-lifetime observer, not one observer per index/search. Include the
// epoch even if the locale identifier stays the same after a settings change.
private let pickleLibraryLocaleObserver = NotificationCenter.default.addObserver(
  forName: NSLocale.currentLocaleDidChangeNotification, object: nil, queue: nil
) { _ in pickleLibraryLocaleEpoch.advance() }

@_cdecl("pickle_library_search_locale")
public func pickleLibrarySearchLocale() -> UnsafeMutablePointer<CChar>? {
  autoreleasepool {
    _ = pickleLibraryLocaleObserver
    let values = [Locale.current.identifier] + Locale.preferredLanguages
      + [String(pickleLibraryLocaleEpoch.value())]
    guard let data = try? JSONEncoder().encode(values),
      let json = String(data: data, encoding: .utf8)
    else { return nil }
    return strdup(json)
  }
}

@_cdecl("pickle_library_search_string_free")
public func pickleLibrarySearchStringFree(_ pointer: UnsafeMutablePointer<CChar>?) { free(pointer) }
