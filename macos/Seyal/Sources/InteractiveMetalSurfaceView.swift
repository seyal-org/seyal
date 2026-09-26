import AppKit

final class InteractiveMetalSurfaceView: MetalSurfaceView, @preconcurrency NSTextInputClient {
    private let appHandle: UInt64
    private let optionAsAlt: Bool
    private var composition = CompositionDocument()
    private var interpretingEscape = false
    private var escapeNeedsTerminalEncoding = false
    private var nextKeyboardActionID: UInt32 = 1
    private var nextMouseActionID: UInt32 = 1
    private var heldKeyboardKinds: [UInt16: TerminalNativeKeyV2] = [:]
    static let maxHeldKeyboardKinds = 256
    var onBridgeBecameUsable: (() -> Void)?
    var onRequestComposerFocus: (() -> Void)?
    var observedAlternateScreen = false
    private var announcedBridgeUsable = false
    private var mouseTrackingArea: NSTrackingArea?

    init(frame frameRect: NSRect, appHandle: UInt64) {
        self.appHandle = appHandle
        self.optionAsAlt = seyal_app_option_as_alt(appHandle) != 0
        super.init(frame: frameRect, paneID: "m001-pane")
        wantsLayer = true
        setAccessibilityIdentifier("terminal-input")
        setAccessibilityRole(.textArea)
        setAccessibilityElement(true)
    }

    override var recoveryAppHandle: UInt64 { appHandle }

    override func restoreNativeInteractionAfterRendererReady() -> Bool {
        if seyal_app_snapshot(appHandle).eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue) {
            onRequestComposerFocus?()
            return true
        }
        return window?.makeFirstResponder(self) ?? false
    }

    override func terminalBridgeStatusDidChange() {
        super.terminalBridgeStatusDidChange()
        let connected = terminalBridgeIsConnected
        if connected {
            guard !announcedBridgeUsable else { return }
            announcedBridgeUsable = true
            onBridgeBecameUsable?()
        } else {
            announcedBridgeUsable = false
            heldKeyboardKinds.removeAll(keepingCapacity: true)
            nextKeyboardActionID = 1
            nextMouseActionID = 1
            composition.clear()
        }
    }

    override var acceptsFirstResponder: Bool { true }

    override func becomeFirstResponder() -> Bool {
        let became = super.becomeFirstResponder()
        if became {
            inputContext?.activate()
        }
        return became
    }

    override func mouseDown(with event: NSEvent) {
        submitNativeMouse(event, kind: 1)
    }

    override func mouseUp(with event: NSEvent) {
        submitNativeMouse(event, kind: 2)
    }

    override func mouseDragged(with event: NSEvent) {
        submitNativeMouse(event, kind: 3)
    }

    override func mouseMoved(with event: NSEvent) {
        submitNativeMouse(event, kind: 3)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let mouseTrackingArea {
            removeTrackingArea(mouseTrackingArea)
        }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        mouseTrackingArea = area
    }

    override func rightMouseDown(with event: NSEvent) {
        submitNativeMouse(event, kind: 1)
    }

    override func rightMouseUp(with event: NSEvent) {
        submitNativeMouse(event, kind: 2)
    }

    override func rightMouseDragged(with event: NSEvent) {
        submitNativeMouse(event, kind: 3)
    }

    override func otherMouseDown(with event: NSEvent) {
        submitNativeMouse(event, kind: 1)
    }

    override func otherMouseUp(with event: NSEvent) {
        submitNativeMouse(event, kind: 2)
    }

    override func otherMouseDragged(with event: NSEvent) {
        submitNativeMouse(event, kind: 3)
    }

    override func scrollWheel(with event: NSEvent) {
        guard allowsDirectTerminalInput else {
            super.scrollWheel(with: event)
            return
        }
        let button: UInt8
        if abs(event.scrollingDeltaY) >= abs(event.scrollingDeltaX) {
            if event.scrollingDeltaY == 0 { return }
            button = event.scrollingDeltaY > 0 ? 64 : 65
        } else {
            if event.scrollingDeltaX == 0 { return }
            button = event.scrollingDeltaX > 0 ? 66 : 67
        }
        submitNativeMouse(event, kind: 4, buttonOverride: button)
    }

    override func keyDown(with event: NSEvent) {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if flags.contains(.command) {
            super.keyDown(with: event)
            return
        }
        guard allowsDirectTerminalInput else { return }

        if composition.hasMarkedText, inputContext?.handleEvent(event) == true {
            return
        }

        // The input context can own a dead key even without a marked document.
        if event.charactersIgnoringModifiers == "\u{1b}",
            flags.subtracting(.capsLock).isEmpty
        {
            interpretingEscape = true
            escapeNeedsTerminalEncoding = false
            let handled = inputContext?.handleEvent(event) == true
            interpretingEscape = false
            if handled && !escapeNeedsTerminalEncoding {
                return
            }
        }
        if hasMarkedText() {
            interpretKeyEvents([event])
            return
        }

        if terminalSupportsKeyV2(),
            let key = TerminalNativeKeyClassifier.v2(
                keyCode: event.keyCode,
                specialKey: event.specialKey,
                charactersIgnoringModifiers: event.charactersIgnoringModifiers,
                characters: event.characters,
                modifierFlags: event.modifierFlags,
                optionAsAlt: optionAsAlt
            )
        {
            switch Self.planHeldKeyPress(
                held: heldKeyboardKinds,
                keyCode: event.keyCode,
                isRepeat: event.isARepeat,
                maxHeld: Self.maxHeldKeyboardKinds
            ) {
            case .overflow:
                rejectHeldKeyOverflow()
                return
            case .rejectUntrackedRepeat:
                return
            case .submit:
                break
            }
            guard let actionID = takeNextKeyboardActionID() else { return }
            let admitted =
                terminalSubmitKeyV2(
                    kind: key.kind,
                    modifiers: key.modifiers,
                    value: key.value,
                    event: event.isARepeat ? 2 : 1,
                    shiftedASCII: key.shiftedASCII,
                    actionID: actionID
                ) == 0
            Self.recordHeldKeyIfAdmitted(
                into: &heldKeyboardKinds,
                keyCode: event.keyCode,
                key: key,
                admitted: admitted
            )
            return
        }

        if let controlScalar = TerminalNativeKeyClassifier.controlASCII(
            modifierFlags: event.modifierFlags,
            charactersIgnoringModifiers: event.charactersIgnoringModifiers
        ) {
            _ = terminalSubmitKey(kind: TerminalKeyIntent.controlASCII.rawValue, scalar: controlScalar)
            return
        }
        if flags.contains(.control) {
            super.keyDown(with: event)
            return
        }

        if let key = TerminalNativeKeyClassifier.semanticKey(
            specialKey: event.specialKey,
            charactersIgnoringModifiers: event.charactersIgnoringModifiers,
            modifierFlags: event.modifierFlags
        ) {
            _ = terminalSubmitKey(kind: key.rawValue, scalar: 0)
            return
        }

        if event.specialKey != nil {
            super.keyDown(with: event)
            return
        }

        interpretKeyEvents([event])
    }

    override func keyUp(with event: NSEvent) {
        guard allowsDirectTerminalInput else {
            heldKeyboardKinds.removeValue(forKey: event.keyCode)
            return
        }
        guard let key = Self.takeHeldKeyForV2Release(
            from: &heldKeyboardKinds,
            keyCode: event.keyCode,
            v2Supported: terminalSupportsKeyV2()
        ) else { return }
        guard let actionID = takeNextKeyboardActionID() else { return }
        _ = terminalSubmitKeyV2(
            kind: key.kind,
            modifiers: key.modifiers,
            value: key.value,
            event: 3,
            shiftedASCII: key.shiftedASCII,
            actionID: actionID
        )
    }

    override func viewWillMove(toWindow newWindow: NSWindow?) {
        if newWindow == nil {
            heldKeyboardKinds.removeAll(keepingCapacity: true)
            composition.clear()
        }
        super.viewWillMove(toWindow: newWindow)
    }

    func hasMarkedText() -> Bool { composition.hasMarkedText }

    func markedRange() -> NSRange { composition.markedRange }

    func selectedRange() -> NSRange { composition.selectedRange }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        guard let text = Self.extractPlainString(from: string) else {
            composition.clear()
            return
        }
        do {
            try composition.setMarkedText(
                text, selectedRange: selectedRange, replacementRange: replacementRange)
            inputContext?.invalidateCharacterCoordinates()
        } catch {
            composition.clear()
            scheduleDiscardMarkedText()
        }
    }

    func unmarkText() {
        guard composition.hasMarkedText else { return }
        let text = composition.text
        composition.clear()
        submitIfAllowed(text)
        inputContext?.invalidateCharacterCoordinates()
    }

    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }

    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?)
        -> NSAttributedString?
    {
        guard let (value, returned) = composition.attributedSubstring(for: range) else {
            actualRange?.pointee = NSRange(location: NSNotFound, length: 0)
            return nil
        }
        actualRange?.pointee = returned
        return value
    }

    func insertText(_ string: Any, replacementRange: NSRange) {
        guard let text = Self.extractPlainString(from: string),
            composition.validatesReplacementRange(replacementRange)
        else {
            composition.clear()
            return
        }
        composition.clear()
        submitIfAllowed(text)
        inputContext?.invalidateCharacterCoordinates()
    }

    func characterIndex(for point: NSPoint) -> Int { NSNotFound }

    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        let valid: NSRange?
        if composition.hasMarkedText {
            valid = composition.validatedCoordinateRange(range)
        } else if range.location == 0 && range.length == 0 {
            valid = range
        } else {
            valid = nil
        }
        guard let valid, let window, let frame = terminalCurrentFrame(),
            frame.cursor_row < frame.rows, frame.cursor_column < frame.columns
        else {
            actualRange?.pointee = NSRange(location: NSNotFound, length: 0)
            return .zero
        }
        let cell = terminalPresentationCellSize()
        let rect = NSRect(
            x: bounds.minX + CGFloat(frame.cursor_column) * cell.width,
            y: isFlipped
                ? bounds.minY + CGFloat(frame.cursor_row) * cell.height
                : bounds.maxY - CGFloat(frame.cursor_row + 1) * cell.height,
            width: cell.width,
            height: cell.height
        ).intersection(bounds)
        guard !rect.isEmpty else {
            actualRange?.pointee = NSRange(location: NSNotFound, length: 0)
            return .zero
        }
        actualRange?.pointee = valid
        return window.convertToScreen(convert(rect, to: nil))
    }

    override func doCommand(by selector: Selector) {
        if selector == #selector(NSResponder.cancelOperation(_:)) {
            if interpretingEscape && !hasMarkedText() {
                escapeNeedsTerminalEncoding = true
            } else {
                cancelOperation(nil)
            }
        }
    }

    override func cancelOperation(_ sender: Any?) {
        guard composition.hasMarkedText else { return }
        composition.clear()
        inputContext?.discardMarkedText()
        inputContext?.invalidateCharacterCoordinates()
    }

    func copy(_ sender: Any?) {
        _ = terminalSubmitHostSelection(action: 4)
    }

    func paste(_ sender: Any?) {
        guard allowsDirectTerminalInput else { return }
        if let text = NSPasteboard.general.string(forType: .string), !text.isEmpty {
            _ = terminalSubmitPaste(text)
        }
    }

    private var allowsDirectTerminalInput: Bool {
        let eligibility = seyal_app_snapshot(appHandle).eligibility
        return eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
    }

    private func submitIfAllowed(_ text: String) {
        guard allowsDirectTerminalInput, !text.isEmpty else { return }
        if text.utf8.count > maxCompositionUTF8Bytes { return }
        _ = terminalSubmitCommittedText(text)
    }

    /// SPEC-006 §21.3: held-key overflow rejects the new press under the
    /// existing client-backpressure input-failure wording, VoiceOver-visible.
    private func rejectHeldKeyOverflow() {
        let message = "Input not sent: terminal client is busy. Retry the input."
        setAccessibilityValue(message)
        SeyalAccessibilityAnnouncement.post(message, element: self)
    }

    private func submitNativeMouse(_ event: NSEvent, kind: UInt8, buttonOverride: UInt8? = nil) {
        if !allowsDirectTerminalInput {
            if kind == 1 {
                onRequestComposerFocus?()
            }
            return
        }
        window?.makeFirstResponder(self)
        guard let cell = terminalMouseCell(for: event),
            let actionID = takeNextMouseActionID()
        else { return }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var modifiers: UInt16 = 0
        if flags.contains(.shift) { modifiers |= 1 }
        if flags.contains(.option) { modifiers |= 2 }
        if flags.contains(.control) { modifiers |= 4 }
        let button: UInt8
        if let buttonOverride {
            button = buttonOverride
        } else if let mapped = Self.xtermButton(event.buttonNumber) {
            button = mapped
        } else {
            return
        }
        _ = terminalSubmitMouse(
            kind: kind,
            button: button,
            modifiers: modifiers,
            col: cell.0,
            row: cell.1,
            actionID: actionID
        )
    }

    static func xtermButton(_ buttonNumber: Int) -> UInt8? {
        switch buttonNumber {
        case 0: return 0
        case 1: return 2
        case 2: return 1
        default: return nil
        }
    }

    private func takeNextMouseActionID() -> UInt32? {
        guard let actionID = Self.v2ActionIDBeforeExhaustion(nextMouseActionID) else {
            terminalStopForProtocolRecovery()
            return nil
        }
        nextMouseActionID = actionID + 1
        return actionID
    }

    private func takeNextKeyboardActionID() -> UInt32? {
        guard let actionID = Self.v2ActionIDBeforeExhaustion(nextKeyboardActionID) else {
            terminalStopForProtocolRecovery()
            return nil
        }
        nextKeyboardActionID &+= 1
        return actionID
    }

    private func scheduleDiscardMarkedText() {
        DispatchQueue.main.async { [weak self] in
            self?.inputContext?.discardMarkedText()
        }
    }

    private static func extractPlainString(from value: Any) -> String? {
        if let text = value as? String { return text }
        if let attributed = value as? NSAttributedString { return attributed.string }
        if let value = value as? NSString { return value as String }
        return nil
    }

    /// SPEC-006 §21.5: stop admission before wrapping or replaying action IDs.
    static func v2ActionIDBeforeExhaustion(_ next: UInt32) -> UInt32? {
        (next == 0 || next == .max) ? nil : next
    }

    enum HeldKeyPressPlan: Equatable {
        case overflow
        case rejectUntrackedRepeat
        case submit
    }

    /// New presses occupy a held slot only after V2 admission succeeds.
    /// Repeats of a key that was never admitted are dropped so key-up cannot
    /// synthesize an orphan release.
    static func planHeldKeyPress(
        held: [UInt16: TerminalNativeKeyV2],
        keyCode: UInt16,
        isRepeat: Bool,
        maxHeld: Int
    ) -> HeldKeyPressPlan {
        if held[keyCode] != nil {
            return .submit
        }
        if isRepeat {
            return .rejectUntrackedRepeat
        }
        if held.count >= maxHeld {
            return .overflow
        }
        return .submit
    }

    static func recordHeldKeyIfAdmitted(
        into held: inout [UInt16: TerminalNativeKeyV2],
        keyCode: UInt16,
        key: TerminalNativeKeyV2,
        admitted: Bool
    ) {
        guard admitted, held[keyCode] == nil else { return }
        held[keyCode] = key
    }

    static func takeHeldKeyForV2Release(
        from held: inout [UInt16: TerminalNativeKeyV2],
        keyCode: UInt16,
        v2Supported: Bool
    ) -> TerminalNativeKeyV2? {
        guard v2Supported else {
            held.removeAll(keepingCapacity: true)
            return nil
        }
        return held.removeValue(forKey: keyCode)
    }
}
