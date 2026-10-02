import Foundation

/// Thin native rendering of Rust-owned launch-policy product copy (ADR-015 / #1119).
///
/// Swift never invents failure or warning text. It only borrows the UTF-8 that
/// `seyal_launch_policy_*_copy` already chose.
public enum LaunchPolicyProductCopy {
    /// Bounded failure string for create-result `result_code` / `detail_code`.
    public static func failureMessage(resultCode: UInt16, detailCode: UInt32) -> String {
        let borrowed = seyal_launch_policy_failure_copy(resultCode, detailCode)
        return utf8String(borrowed)
    }

    /// Bounded warning strings for a `Created.detail_code` bitfield.
    public static func warningMessages(detailCode: UInt32) -> [String] {
        (0..<2).compactMap { bit in
            guard detailCode & (1 << bit) != 0 else { return nil }
            let borrowed = seyal_launch_policy_warning_copy(UInt32(bit))
            let text = utf8String(borrowed)
            return text.isEmpty ? nil : text
        }
    }

    /// Surface already-decided Rust copy through the host log (non-secret only).
    public static func surface(resultCode: UInt16, detailCode: UInt32) {
        if resultCode == 17 {
            NSLog("Seyal launch policy: %@", failureMessage(resultCode: resultCode, detailCode: detailCode))
            return
        }
        if resultCode == 0 {
            for line in warningMessages(detailCode: detailCode) {
                NSLog("Seyal launch policy: %@", line)
            }
        }
    }

    private static func utf8String(_ borrowed: SeyalLaunchPolicyCopy) -> String {
        guard let pointer = borrowed.text, borrowed.text_len > 0 else { return "" }
        let buffer = UnsafeBufferPointer(start: pointer, count: Int(borrowed.text_len))
        return String(bytes: buffer, encoding: .utf8) ?? ""
    }
}
