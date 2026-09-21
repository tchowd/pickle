import PickleCore

final class InvocationTests: CheckSuite {
    func testTwoQuickTaps() {
        var taps = DoubleOptionTap()
        expectTrue(!taps.optionChanged(isDown: true, at: 0))
        expectTrue(!taps.optionChanged(isDown: false, at: 0.08))
        expectTrue(!taps.optionChanged(isDown: true, at: 0.20))
        expectTrue(taps.optionChanged(isDown: false, at: 0.28))
        expectTrue(!taps.optionChanged(isDown: true, at: 0.35))
        expectTrue(!taps.optionChanged(isDown: false, at: 0.40))
    }
    func testSlowTapsAndHoldsDoNotInvoke() {
        var taps = DoubleOptionTap()
        _ = taps.optionChanged(isDown: true, at: 0)
        _ = taps.optionChanged(isDown: false, at: 0.1)
        _ = taps.optionChanged(isDown: true, at: 0.8)
        expectTrue(!taps.optionChanged(isDown: false, at: 0.9))
        _ = taps.optionChanged(isDown: true, at: 1)
        expectTrue(!taps.optionChanged(isDown: false, at: 2))
    }
    func testTypingOrModifiersInterruptTapSequence() {
        var taps = DoubleOptionTap()
        _ = taps.optionChanged(isDown: true, at: 0)
        _ = taps.optionChanged(isDown: false, at: 0.1)
        taps.reset() // Ordinary key, other modifier, mouse click, pause, or shortcut change.
        _ = taps.optionChanged(isDown: true, at: 0.2)
        expectTrue(!taps.optionChanged(isDown: false, at: 0.3))
        taps.reset()
        expectTrue(!taps.optionChanged(isDown: false, at: 0.4))
    }
}
