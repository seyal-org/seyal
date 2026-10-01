import Foundation

/// Thin native rendering of Rust-owned launch-policy product copy (ADR-015 / #1119).
///
/// Swift never invents failure or warning text and never decides which
/// result/detail codes are failures vs warnings. It only borrows the UTF-8 that
/// `seyal_launch_policy_copies` already selected.
enum LaunchPolicyProductCopy {
    /// Capacity for the FFI fill buffer; Rust owns the real selection count.
    private static let copyCapacity: UInt32 = 8

    /// Bounded failure string for create-result `result_code` / `detail_code`.
    static func failureMessage(resultCode: UInt16, detailCode: UInt32) -> String {
        messages(resultCode: resultCode, detailCode: detailCode).first ?? ""
    }

    /// Bounded warning strings for a successful create (`result_code == 0`) bitfield.
    static func warningMessages(detailCode: UInt32) -> [String] {
        messages(resultCode: 0, detailCode: detailCode)
    }

    /// All Rust-selected copies for a create-result pair.
    static func messages(resultCode: UInt16, detailCode: UInt32) -> [String] {
        var buffer = Array(
            repeating: SeyalLaunchPolicyCopy(text: nil, text_len: 0, reserved: 0),
            count: Int(copyCapacity)
        )
        let count = buffer.withUnsafeMutableBufferPointer { ptr in
            seyal_launch_policy_copies(resultCode, detailCode, ptr.baseAddress, copyCapacity)
        }
        return (0..<Int(count)).compactMap { index in
            let text = utf8String(buffer[index])
            return text.isEmpty ? nil : text
        }
    }

    private static func utf8String(_ borrowed: SeyalLaunchPolicyCopy) -> String {
        guard let pointer = borrowed.text, borrowed.text_len > 0 else { return "" }
        let buffer = UnsafeBufferPointer(start: pointer, count: Int(borrowed.text_len))
        return String(bytes: buffer, encoding: .utf8) ?? ""
    }
}
