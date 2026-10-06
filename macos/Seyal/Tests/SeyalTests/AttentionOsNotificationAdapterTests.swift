import XCTest

@testable import Seyal

/// SPEC-028 §12.17 / §8: OS adapter dismiss is presentation-only.
final class AttentionOsNotificationAdapterTests: XCTestCase {
    override func tearDown() {
        AttentionOsNotificationAdapter.qualificationSink = nil
        super.tearDown()
    }

    func testBannerDismissDoesNotImplyResolve() {
        var events: [String] = []
        AttentionOsNotificationAdapter.qualificationSink = { events.append($0) }
        AttentionOsNotificationAdapter.deliver(
            .init(identifier: "att-1", title: "Needs input", body: "preview")
        )
        AttentionOsNotificationAdapter.noteBannerDismissed(identifier: "att-1")
        AttentionOsNotificationAdapter.noteDeliveryFailed(identifier: "att-1")
        XCTAssertEqual(events, ["deliver:att-1", "dismiss:att-1", "fail:att-1"])
    }
}
