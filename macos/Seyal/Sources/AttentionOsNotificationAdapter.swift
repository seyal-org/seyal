import Foundation

#if canImport(UserNotifications)
import UserNotifications
#endif

/// Thin macOS adapter for Attention OS notifications (ADR-015 / SPEC-028 §8).
///
/// Eligibility, copy, rate limits, and jump-target identity are Rust-owned.
/// This type only posts or observes the platform banner. Dismiss and delivery
/// failure must never be treated as Attention resolve, acknowledge, or approve.
enum AttentionOsNotificationAdapter {
    struct Delivery: Equatable {
        var identifier: String
        var title: String
        var body: String
    }

    /// Qualification sink: `deliver:id`, `dismiss:id`, or `fail:id`.
    /// XCTest mutates this from `setUp`/`tearDown`; CI Xcode 16 keeps those
    /// methods nonisolated, so the hook is explicitly unsynchronized test state.
    nonisolated(unsafe) static var qualificationSink: ((String) -> Void)?

    static func deliver(_ delivery: Delivery) {
        qualificationSink?("deliver:\(delivery.identifier)")
        #if canImport(UserNotifications)
        let center = UNUserNotificationCenter.current()
        center.requestAuthorization(options: [.alert, .sound]) { granted, _ in
            guard granted else {
                noteDeliveryFailed(identifier: delivery.identifier)
                return
            }
            let content = UNMutableNotificationContent()
            content.title = delivery.title
            content.body = delivery.body
            let request = UNNotificationRequest(
                identifier: delivery.identifier,
                content: content,
                trigger: nil
            )
            center.add(request) { error in
                if error != nil {
                    noteDeliveryFailed(identifier: delivery.identifier)
                }
            }
        }
        #endif
    }

    /// Banner dismiss is presentation-only (SPEC-028 §8.1 / §12.17).
    static func noteBannerDismissed(identifier: String) {
        qualificationSink?("dismiss:\(identifier)")
    }

    /// OS delivery failure must not erase canonical Attention (§8.5 / §12.22).
    static func noteDeliveryFailed(identifier: String) {
        qualificationSink?("fail:\(identifier)")
    }
}
