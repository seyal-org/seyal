import AppKit

let maxCompositionUTF8Bytes = 65_536

enum CompositionMutationError: Error, Equatable {
    case invalidRange
    case tooLarge
}

struct CompositionDocument: Equatable {
    private(set) var text = ""
    private(set) var selection = NSRange(location: 0, length: 0)

    var utf16Length: Int { (text as NSString).length }

    var hasMarkedText: Bool { utf16Length > 0 }

    var markedRange: NSRange {
        hasMarkedText
            ? NSRange(location: 0, length: utf16Length)
            : NSRange(location: NSNotFound, length: 0)
    }

    var selectedRange: NSRange {
        hasMarkedText ? selection : NSRange(location: 0, length: 0)
    }

    mutating func clear() {
        text = ""
        selection = NSRange(location: 0, length: 0)
    }

    mutating func setMarkedText(
        _ inserted: String,
        selectedRange insertedSelection: NSRange,
        replacementRange requestedReplacement: NSRange
    ) throws {
        let currentLength = utf16Length
        let replacement: NSRange
        if requestedReplacement.location == NSNotFound {
            guard requestedReplacement.length == 0 else {
                throw CompositionMutationError.invalidRange
            }
            replacement = selectedRange
        } else {
            guard let valid = Self.validatedRange(requestedReplacement, upperBound: currentLength) else {
                throw CompositionMutationError.invalidRange
            }
            replacement = valid
        }

        let insertedLength = (inserted as NSString).length
        guard Self.validatedRange(insertedSelection, upperBound: insertedLength) != nil else {
            throw CompositionMutationError.invalidRange
        }

        let mutable = NSMutableString(string: text)
        mutable.replaceCharacters(in: replacement, with: inserted)
        let candidate = mutable as String
        guard candidate.utf8.count <= maxCompositionUTF8Bytes else {
            throw CompositionMutationError.tooLarge
        }

        let (absoluteLocation, overflow) = replacement.location.addingReportingOverflow(
            insertedSelection.location
        )
        guard !overflow else {
            throw CompositionMutationError.invalidRange
        }
        let absoluteSelection = NSRange(location: absoluteLocation, length: insertedSelection.length)
        guard Self.validatedRange(absoluteSelection, upperBound: (candidate as NSString).length) != nil else {
            throw CompositionMutationError.invalidRange
        }

        text = candidate
        selection = absoluteSelection
    }

    func validatesReplacementRange(_ range: NSRange) -> Bool {
        if range.location == NSNotFound {
            return range.length == 0
        }
        return Self.validatedRange(range, upperBound: utf16Length) != nil
    }

    func attributedSubstring(for proposedRange: NSRange) -> (NSAttributedString, NSRange)? {
        guard proposedRange.location != NSNotFound else { return nil }
        let length = utf16Length
        guard let proposedEnd = Self.checkedEnd(proposedRange) else { return nil }
        if proposedRange.location > length
            || (proposedRange.location == length && proposedRange.length > 0)
        {
            return nil
        }
        let boundedStart = min(proposedRange.location, length)
        let boundedEnd = min(proposedEnd, length)
        let bounded = NSRange(location: boundedStart, length: boundedEnd - boundedStart)
        if bounded.length == 0 {
            return (NSAttributedString(string: ""), bounded)
        }
        let storage = text as NSString
        let composed = storage.rangeOfComposedCharacterSequences(for: bounded)
        guard let valid = Self.validatedRange(composed, upperBound: length) else { return nil }
        return (NSAttributedString(string: storage.substring(with: valid)), valid)
    }

    func validatedCoordinateRange(_ range: NSRange) -> NSRange? {
        guard range.location != NSNotFound else { return nil }
        let length = utf16Length
        guard let end = Self.checkedEnd(range), range.location <= length else { return nil }
        if range.length == 0 {
            return NSRange(location: range.location, length: 0)
        }
        if range.location == length {
            return nil
        }
        let bounded = NSRange(location: range.location, length: min(end, length) - range.location)
        return (text as NSString).rangeOfComposedCharacterSequences(for: bounded)
    }

    private static func validatedRange(_ range: NSRange, upperBound: Int) -> NSRange? {
        guard range.location != NSNotFound,
            range.location <= upperBound,
            let end = checkedEnd(range),
            end <= upperBound
        else {
            return nil
        }
        return range
    }

    private static func checkedEnd(_ range: NSRange) -> Int? {
        let (end, overflow) = range.location.addingReportingOverflow(range.length)
        return overflow ? nil : end
    }
}

