// AXEnhancedUserInterface can animate individual frame writes. A subsequent
// position/size write can interrupt the previous animation and leave half a snap.
// Keep the setting scoped to the operation; never permanently disable it.
func withImmediateWindowAdjustment(
    assistiveTechnologyEnabled: Bool,
    readEnhancedUI: () -> Bool?,
    writeEnhancedUI: (Bool) -> Bool,
    report: (String) -> Void,
    adjustment: () throws -> Void
) rethrows {
    var restore = false
    if !assistiveTechnologyEnabled, readEnhancedUI() == true {
        restore = writeEnhancedUI(false)
        if !restore { report("Could not suspend AXEnhancedUserInterface; window may animate") }
    }
    defer {
        if restore && !writeEnhancedUI(true) {
            report("Could not restore AXEnhancedUserInterface after window adjustment")
        }
    }
    try adjustment()
}
