/// Recognizes Control + Option alone, invoking once after both are released.
/// Any ordinary key, extra modifier, or mouse click cancels until release.
public struct ControlOptionChord {
    private var armed = false
    private var interrupted = false
    public init() {}
    public mutating func reset() { armed = false; interrupted = false }
    public mutating func interrupt() { armed = false; interrupted = true }
    public mutating func modifiersChanged(control: Bool, option: Bool, other: Bool = false) -> Bool {
        if other { interrupt() }
        if !control && !option && !other {
            let invoke = armed && !interrupted
            reset()
            return invoke
        }
        if control && option && !interrupted { armed = true }
        return false
    }
}
