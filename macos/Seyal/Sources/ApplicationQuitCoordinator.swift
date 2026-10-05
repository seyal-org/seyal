import AppKit
import Foundation

/// ADR-018 §4 quit backstop. Arms exactly one one-shot timer and replies once.
@MainActor
final class ApplicationQuitCoordinator {
    private(set) var replyCount = 0
    /// Deadline carried by `BoundedDetachThenTerminate`, once the backstop is armed.
    private(set) var armedDeadlineMs: UInt64 = 0
    private(set) var didArmBackstop = false
    private var awaitingReply = false
    private var backstopWorkItem: DispatchWorkItem?
    private var replyHandler: (() -> Void)?

    /// Production path: forward RequestQuit, arm one backstop, cleanup, then
    /// reply on the first of cleanup-complete or backstop. Forwarding failure
    /// replies immediately. Late completion after reply is ignored.
    @discardableResult
    func beginTerminateLater(
        forwardRequestQuit: () -> Result<UInt64, Error>,
        performNativeCleanup: () -> Void,
        ackUntilCleanupComplete: () -> Bool,
        reply: @escaping () -> Void
    ) -> NSApplication.TerminateReply {
        replyHandler = reply
        switch forwardRequestQuit() {
        case .failure:
            replyOnce()
            return .terminateLater
        case .success(let deadlineMs):
            awaitingReply = true
            armedDeadlineMs = deadlineMs
            didArmBackstop = true
            let work = DispatchWorkItem { [weak self] in
                self?.replyOnce()
            }
            backstopWorkItem = work
            let capped = min(deadlineMs, UInt64(Int.max))
            DispatchQueue.main.asyncAfter(
                deadline: .now() + .milliseconds(Int(capped)),
                execute: work
            )
            performNativeCleanup()
            if ackUntilCleanupComplete() {
                replyOnce()
            }
            return .terminateLater
        }
    }

    /// Late Rust cleanup-complete after the backstop already replied is ignored.
    func signalCleanupComplete() {
        guard awaitingReply else { return }
        replyOnce()
    }

    private func replyOnce() {
        guard replyCount == 0 else { return }
        replyCount += 1
        awaitingReply = false
        backstopWorkItem?.cancel()
        backstopWorkItem = nil
        let handler = replyHandler
        replyHandler = nil
        handler?()
    }
}
