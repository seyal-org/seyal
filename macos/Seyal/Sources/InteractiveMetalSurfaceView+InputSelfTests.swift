import AppKit

@MainActor
extension InteractiveMetalSurfaceView {
    static func xtermButtonSelfTest() -> Bool {
        xtermButton(0) == 0
            && xtermButton(1) == 2
            && xtermButton(2) == 1
            && xtermButton(3) == nil
            && xtermButton(-1) == nil
    }

    static func pass7InputSelfTest() -> Bool {
        controlNormalizationSelfTest()
            && compositionUTF16SelfTest()
            && compositionBoundsSelfTest()
            && composedSubstringSelfTest()
            && compositionMarkedCommitSelfTest()
            && compositionCancelAbandonSelfTest()
            && compositionReplacementCommitSelfTest()
            && compositionCandidateCoordinateSelfTest()
            && compositionDetachDiscardsPreeditSelfTest()
            && semanticKeyMatrixSelfTest()
            && keyReleaseMetadataSelfTest()
            && heldKeyboardCapacitySelfTest()
            && heldKeyAdmissionSelfTest()
            && capabilityLossDropsHeldKeyReleaseSelfTest()
            && RustDisplayBridge.pasteAdmissionSelfTest()
            && xtermButtonSelfTest()
    }

    static func controlNormalizationSelfTest() -> Bool {
        let control: NSEvent.ModifierFlags = [.control]
        let shifted: NSEvent.ModifierFlags = [.control, .shift]
        let caps: NSEvent.ModifierFlags = [.control, .capsLock]
        guard
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: control, charactersIgnoringModifiers: "a") == 0x41,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: caps, charactersIgnoringModifiers: "z") == 0x5a,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: shifted, charactersIgnoringModifiers: "@") == 0x40,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: shifted, charactersIgnoringModifiers: "^") == 0x5e,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: shifted, charactersIgnoringModifiers: "_") == 0x5f,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: shifted, charactersIgnoringModifiers: "?") == 0x3f,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: control, charactersIgnoringModifiers: " ") == 0x20,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: control, charactersIgnoringModifiers: "[") == 0x5b,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: control, charactersIgnoringModifiers: "\\") == 0x5c,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: control, charactersIgnoringModifiers: "]") == 0x5d,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: [.control, .option], charactersIgnoringModifiers: "a") == nil,
            TerminalNativeKeyClassifier.controlASCII(
                modifierFlags: control, charactersIgnoringModifiers: "å") == nil
        else {
            return false
        }
        return TerminalNativeKeyClassifier.controlASCII(
            modifierFlags: control, charactersIgnoringModifiers: "q") == 0x51
    }

    static func compositionUTF16SelfTest() -> Bool {
        var document = CompositionDocument()
        do {
            try document.setMarkedText(
                "😀x",
                selectedRange: NSRange(location: 2, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        } catch {
            return false
        }
        guard document.utf16Length == 3,
            document.markedRange == NSRange(location: 0, length: 3),
            document.selectedRange == NSRange(location: 2, length: 0)
        else {
            return false
        }
        do {
            try document.setMarkedText(
                "A",
                selectedRange: NSRange(location: 1, length: 0),
                replacementRange: NSRange(location: 2, length: 1)
            )
        } catch {
            return false
        }
        return document.text == "😀A" && document.selectedRange == NSRange(location: 3, length: 0)
    }

    static func compositionBoundsSelfTest() -> Bool {
        var document = CompositionDocument()
        let original = document
        let oversized = String(repeating: "x", count: maxCompositionUTF8Bytes + 1)
        do {
            try document.setMarkedText(
                oversized,
                selectedRange: NSRange(location: 0, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
            return false
        } catch CompositionMutationError.tooLarge {
            guard document == original else { return false }
        } catch {
            return false
        }
        do {
            try document.setMarkedText(
                "x",
                selectedRange: NSRange(location: 2, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
            return false
        } catch CompositionMutationError.invalidRange {
            return document == original
        } catch {
            return false
        }
    }

    static func composedSubstringSelfTest() -> Bool {
        var document = CompositionDocument()
        do {
            try document.setMarkedText(
                "e\u{301}",
                selectedRange: NSRange(location: 2, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        } catch {
            return false
        }
        guard
            let (substring, range) = document.attributedSubstring(for: NSRange(location: 1, length: 1))
        else {
            return false
        }
        return substring.string == "e\u{301}"
            && range == NSRange(location: 0, length: 2)
            && document.attributedSubstring(for: NSRange(location: 3, length: 1)) == nil
    }

    // Document-model invariants only; these do not exercise NSTextInputClient
    // callbacks or establish SPEC-011 headed IME fixtures 37-41.
    static func compositionMarkedCommitSelfTest() -> Bool {
        var document = CompositionDocument()
        do {
            try document.setMarkedText(
                "ni",
                selectedRange: NSRange(location: 2, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        } catch {
            return false
        }
        guard document.hasMarkedText, document.text == "ni" else { return false }
        let committed = document.text
        document.clear()
        return !document.hasMarkedText && committed == "ni" && document.text.isEmpty
    }

    static func compositionCancelAbandonSelfTest() -> Bool {
        var document = CompositionDocument()
        do {
            try document.setMarkedText(
                "´",
                selectedRange: NSRange(location: 1, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        } catch {
            return false
        }
        guard document.hasMarkedText else { return false }
        document.clear()
        return !document.hasMarkedText && document.text.isEmpty
            && document.markedRange.location == NSNotFound
    }

    static func compositionReplacementCommitSelfTest() -> Bool {
        var document = CompositionDocument()
        do {
            try document.setMarkedText(
                "abc",
                selectedRange: NSRange(location: 3, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
            try document.setMarkedText(
                "XYZ",
                selectedRange: NSRange(location: 3, length: 0),
                replacementRange: NSRange(location: 1, length: 1)
            )
        } catch {
            return false
        }
        guard document.text == "aXYZc" else { return false }
        document.clear()
        return !document.hasMarkedText
    }

    static func compositionCandidateCoordinateSelfTest() -> Bool {
        var document = CompositionDocument()
        do {
            try document.setMarkedText(
                "😀x",
                selectedRange: NSRange(location: 2, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        } catch {
            return false
        }
        guard document.validatesReplacementRange(NSRange(location: 0, length: 3)) else {
            return false
        }
        guard document.validatedCoordinateRange(NSRange(location: 0, length: 2)) != nil else {
            return false
        }
        guard document.validatedCoordinateRange(NSRange(location: NSNotFound, length: 0)) == nil
        else {
            return false
        }
        return document.validatedCoordinateRange(NSRange(location: 3, length: 1)) == nil
    }

    static func compositionDetachDiscardsPreeditSelfTest() -> Bool {
        var document = CompositionDocument()
        do {
            try document.setMarkedText(
                "preedit",
                selectedRange: NSRange(location: 7, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        } catch {
            return false
        }
        document.clear()
        return !document.hasMarkedText && document.text.isEmpty
            && document.selectedRange == NSRange(location: 0, length: 0)
    }

    static func semanticKeyMatrixSelfTest() -> Bool {
        TerminalNativeKeyClassifier.semanticKey(
            specialKey: .upArrow, charactersIgnoringModifiers: nil, modifierFlags: []) == .arrowUp
            && TerminalNativeKeyClassifier.semanticKey(
                specialKey: .upArrow, charactersIgnoringModifiers: nil, modifierFlags: [.shift])
                == nil
            && TerminalNativeKeyClassifier.semanticKey(
                specialKey: .enter, charactersIgnoringModifiers: nil, modifierFlags: [.numericPad])
                == .enter
            && TerminalNativeKeyClassifier.semanticKey(
                specialKey: .backTab, charactersIgnoringModifiers: nil, modifierFlags: [.shift])
                == nil
            && TerminalNativeKeyClassifier.semanticKey(
                specialKey: .delete, charactersIgnoringModifiers: nil, modifierFlags: []) == nil
            && TerminalNativeKeyClassifier.v2(
                keyCode: 36, specialKey: .enter, charactersIgnoringModifiers: nil, characters: nil,
                modifierFlags: [], optionAsAlt: false)?.kind == 1
            && TerminalNativeKeyClassifier.v2(
                keyCode: 48, specialKey: .tab, charactersIgnoringModifiers: nil, characters: nil,
                modifierFlags: [.shift], optionAsAlt: false
            ).map { $0.kind == 2 && $0.modifiers == 1 } == true
            && TerminalNativeKeyClassifier.v2(
                keyCode: 53, specialKey: nil, charactersIgnoringModifiers: "\u{1b}",
                characters: nil, modifierFlags: [], optionAsAlt: false)?.kind == 4
            && TerminalNativeKeyClassifier.v2(
                keyCode: 36, specialKey: .enter, charactersIgnoringModifiers: nil, characters: nil,
                modifierFlags: [.control], optionAsAlt: false
            ).map { $0.kind == 1 && $0.modifiers == 4 } == true
            && TerminalNativeKeyClassifier.v2(
                keyCode: 51, specialKey: .backspace, charactersIgnoringModifiers: nil,
                characters: nil, modifierFlags: [.control], optionAsAlt: false
            ).map { $0.kind == 3 && $0.modifiers == 4 } == true
            && InteractiveMetalSurfaceView.v2ActionIDBeforeExhaustion(1) == 1
            && InteractiveMetalSurfaceView.v2ActionIDBeforeExhaustion(0) == nil
            && InteractiveMetalSurfaceView.v2ActionIDBeforeExhaustion(.max) == nil
            && TerminalNativeKeyClassifier.v2(
                keyCode: 0, specialKey: nil, charactersIgnoringModifiers: "a", characters: "a",
                modifierFlags: [], optionAsAlt: false) == nil
            && TerminalNativeKeyClassifier.v2(
                keyCode: 0, specialKey: nil, charactersIgnoringModifiers: "a", characters: "A",
                modifierFlags: [.shift, .option], optionAsAlt: true)?.shiftedASCII == 65
    }

    static func heldKeyboardCapacitySelfTest() -> Bool {
        var held: [UInt16: TerminalNativeKeyV2] = [:]
        for index in 0..<maxHeldKeyboardKinds {
            held[UInt16(index)] = TerminalNativeKeyV2(
                kind: 17, modifiers: 0, value: 0x61, shiftedASCII: 0)
        }
        let trackedRepeatAllowed =
            planHeldKeyPress(
                held: held, keyCode: 0, isRepeat: true, maxHeld: maxHeldKeyboardKinds) == .submit
        let newPressRejected =
            planHeldKeyPress(
                held: held, keyCode: 300, isRepeat: false, maxHeld: maxHeldKeyboardKinds)
            == .overflow
        return trackedRepeatAllowed && newPressRejected
    }

    static func heldKeyAdmissionSelfTest() -> Bool {
        let key = TerminalNativeKeyV2(kind: 5, modifiers: 0, value: 0, shiftedASCII: 0)
        var held: [UInt16: TerminalNativeKeyV2] = [:]
        guard
            planHeldKeyPress(held: held, keyCode: 1, isRepeat: false, maxHeld: 2) == .submit
        else { return false }
        recordHeldKeyIfAdmitted(into: &held, keyCode: 1, key: key, admitted: false)
        guard held.isEmpty else { return false }
        guard
            planHeldKeyPress(held: held, keyCode: 1, isRepeat: true, maxHeld: 2)
                == .rejectUntrackedRepeat
        else { return false }
        recordHeldKeyIfAdmitted(into: &held, keyCode: 1, key: key, admitted: true)
        guard held[1] != nil else { return false }
        guard planHeldKeyPress(held: held, keyCode: 1, isRepeat: true, maxHeld: 2) == .submit
        else { return false }
        recordHeldKeyIfAdmitted(into: &held, keyCode: 1, key: key, admitted: true)
        recordHeldKeyIfAdmitted(into: &held, keyCode: 2, key: key, admitted: true)
        return planHeldKeyPress(held: held, keyCode: 3, isRepeat: false, maxHeld: 2) == .overflow
    }

    static func keyReleaseMetadataSelfTest() -> Bool {
        guard
            let key = TerminalNativeKeyClassifier.v2(
                keyCode: 0, specialKey: nil, charactersIgnoringModifiers: "a", characters: "A",
                modifierFlags: [.shift, .option], optionAsAlt: true)
        else { return false }
        var held: [UInt16: TerminalNativeKeyV2] = [0: key]
        let released = held.removeValue(forKey: 0)
        return released?.shiftedASCII == 65 && held.isEmpty
    }

    static func capabilityLossDropsHeldKeyReleaseSelfTest() -> Bool {
        guard
            let key = TerminalNativeKeyClassifier.v2(
                keyCode: 0, specialKey: nil, charactersIgnoringModifiers: "a", characters: "A",
                modifierFlags: [.shift, .option], optionAsAlt: true)
        else { return false }
        var held: [UInt16: TerminalNativeKeyV2] = [0: key]
        let released = takeHeldKeyForV2Release(from: &held, keyCode: 0, v2Supported: false)
        return released == nil && held.isEmpty
    }
}
