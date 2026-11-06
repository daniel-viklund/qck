import Foundation

@main struct WindowAdjustmentTest {
    enum ExpectedFailure: Error { case resize }
    static func main() throws {
        // Animation must be off throughout the sequence and restored afterward.
        var enabled = true
        var writes: [Bool] = []
        withImmediateWindowAdjustment(assistiveTechnologyEnabled: false,
            readEnhancedUI: { enabled },
            writeEnhancedUI: { enabled = $0; writes.append($0); return true },
            report: { fatalError($0) }) {
                precondition(!enabled)
            }
        precondition(enabled && writes == [false, true])

        // A resize failure must not leave Firefox's accessibility mode disabled.
        writes.removeAll()
        do {
            try withImmediateWindowAdjustment(assistiveTechnologyEnabled: false,
                readEnhancedUI: { enabled },
                writeEnhancedUI: { enabled = $0; writes.append($0); return true },
                report: { fatalError($0) }) {
                    precondition(!enabled)
                    throw ExpectedFailure.resize
                }
            fatalError("Expected the resize failure to propagate")
        } catch ExpectedFailure.resize {}
        precondition(enabled && writes == [false, true])

        // Leave unsupported/disabled attributes and active assistive tools alone.
        for (assistive, original) in [(false, Optional(false)), (false, nil), (true, Optional(true))] {
            var ran = false
            withImmediateWindowAdjustment(assistiveTechnologyEnabled: assistive,
                readEnhancedUI: { original },
                writeEnhancedUI: { _ in fatalError("Unexpected mode change") },
                report: { fatalError($0) }) { ran = true }
            precondition(ran)
        }

        // An optional-attribute failure must not prevent normal window movement.
        var warnings = 0
        writes.removeAll()
        var ran = false
        withImmediateWindowAdjustment(assistiveTechnologyEnabled: false,
            readEnhancedUI: { true },
            writeEnhancedUI: { writes.append($0); return false },
            report: { _ in warnings += 1 }) { ran = true }
        precondition(ran && writes == [false] && warnings == 1)

        // Failed restoration is visible in diagnostics, never silently ignored.
        warnings = 0
        withImmediateWindowAdjustment(assistiveTechnologyEnabled: false,
            readEnhancedUI: { true }, writeEnhancedUI: { !$0 },
            report: { _ in warnings += 1 }) {}
        precondition(warnings == 1)
        print("PASS: window adjustment scopes animation mode, restores on failure, and preserves assistive tools")
    }
}
