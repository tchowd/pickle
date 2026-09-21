import PickleCore

final class InvocationTests: CheckSuite {
    func testChordInvokesOnceOnRelease() {
        var chord = ControlOptionChord()
        expectTrue(!chord.modifiersChanged(control: true, option: false))
        expectTrue(!chord.modifiersChanged(control: true, option: true))
        expectTrue(!chord.modifiersChanged(control: false, option: true))
        expectTrue(chord.modifiersChanged(control: false, option: false))
        expectTrue(!chord.modifiersChanged(control: false, option: false))
        expectTrue(!chord.modifiersChanged(control: false, option: true))
        expectTrue(!chord.modifiersChanged(control: true, option: true))
        expectTrue(chord.modifiersChanged(control: false, option: false))
    }
    func testSingleModifiersDoNotInvoke() {
        var chord = ControlOptionChord()
        expectTrue(!chord.modifiersChanged(control: true, option: false))
        expectTrue(!chord.modifiersChanged(control: false, option: false))
        expectTrue(!chord.modifiersChanged(control: false, option: true))
        expectTrue(!chord.modifiersChanged(control: false, option: false))
    }
    func testOtherInputCancelsUntilRelease() {
        var chord = ControlOptionChord()
        _ = chord.modifiersChanged(control: true, option: true)
        chord.interrupt()
        expectTrue(!chord.modifiersChanged(control: true, option: true))
        expectTrue(!chord.modifiersChanged(control: false, option: false))
        _ = chord.modifiersChanged(control: true, option: true, other: true)
        expectTrue(!chord.modifiersChanged(control: true, option: true))
        expectTrue(!chord.modifiersChanged(control: false, option: false))
        _ = chord.modifiersChanged(control: true, option: true)
        chord.reset()
        expectTrue(!chord.modifiersChanged(control: false, option: false))
        _ = chord.modifiersChanged(control: true, option: true)
        expectTrue(chord.modifiersChanged(control: false, option: false))
    }
}
