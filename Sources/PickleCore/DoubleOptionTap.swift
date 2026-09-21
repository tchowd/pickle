import Foundation

/// Recognizes two short, unmodified Option taps. Does not retain typed keys.
public struct DoubleOptionTap {
    private var pressedAt: TimeInterval?
    private var releasedAt: TimeInterval?
    public init() {}
    public mutating func reset() { pressedAt = nil; releasedAt = nil }
    public mutating func optionChanged(isDown: Bool, at time: TimeInterval) -> Bool {
        if isDown {
            guard pressedAt == nil else { reset(); return false }
            if let releasedAt, time - releasedAt > 0.4 { self.releasedAt = nil }
            pressedAt = time
            return false
        }
        guard let pressedAt, time >= pressedAt, time - pressedAt <= 0.3 else { reset(); return false }
        self.pressedAt = nil
        if let releasedAt, pressedAt >= releasedAt, time - releasedAt <= 0.5 {
            reset()
            return true
        }
        releasedAt = time
        return false
    }
}
