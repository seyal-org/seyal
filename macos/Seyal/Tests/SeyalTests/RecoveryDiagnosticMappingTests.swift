import XCTest

@testable import Seyal

/// #1421 — chrome recovery tokens must stay aligned with SeyalAppRecoveryDiagnostic
/// and BundledRuntimeLaunchError (opaque `blocked · attempts N` is insufficient).
final class RecoveryDiagnosticMappingTests: XCTestCase {
    func testDiagnosticLabelsCoverStableNonSecretCodes() {
        XCTAssertEqual(
            ProductChromeHostView.recoveryDiagnosticLabel(
                UInt32(SEYAL_APP_RECOVERY_DIAG_HELPER_MISSING.rawValue)
            ),
            "helperMissing"
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryDiagnosticLabel(
                UInt32(SEYAL_APP_RECOVERY_DIAG_HELPER_PATH_INVALID.rawValue)
            ),
            "helperPathInvalid"
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryDiagnosticLabel(
                UInt32(SEYAL_APP_RECOVERY_DIAG_HELPER_TRUST_INVALID.rawValue)
            ),
            "helperTrustInvalid"
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryDiagnosticLabel(
                UInt32(SEYAL_APP_RECOVERY_DIAG_LAUNCH_DENIED.rawValue)
            ),
            "launchDenied"
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryDiagnosticLabel(
                UInt32(SEYAL_APP_RECOVERY_DIAG_LAUNCH_FAILED.rawValue)
            ),
            "launchFailed"
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryDiagnosticLabel(
                UInt32(SEYAL_APP_RECOVERY_DIAG_RUNTIME_STARTUP_FAILED.rawValue)
            ),
            "runtimeStartupFailed"
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryDiagnosticLabel(
                UInt32(SEYAL_APP_RECOVERY_DIAG_ENDPOINT_REJECTED.rawValue)
            ),
            "endpointRejected"
        )
        XCTAssertNil(
            ProductChromeHostView.recoveryDiagnosticLabel(
                UInt32(SEYAL_APP_RECOVERY_DIAG_NONE.rawValue)
            )
        )
        XCTAssertNil(ProductChromeHostView.recoveryDiagnosticLabel(9_999))
    }

    func testLaunchCodeMapsTypedHelperErrorsAndFailsClosed() {
        XCTAssertEqual(
            ProductChromeHostView.recoveryLaunchCode(from: nil),
            SEYAL_APP_RECOVERY_LAUNCH_HELPER_MISSING
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryLaunchCode(from: .helperMissing),
            SEYAL_APP_RECOVERY_LAUNCH_HELPER_MISSING
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryLaunchCode(from: .helperPathInvalid),
            SEYAL_APP_RECOVERY_LAUNCH_HELPER_PATH_INVALID
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryLaunchCode(from: .helperTrustInvalid),
            SEYAL_APP_RECOVERY_LAUNCH_HELPER_TRUST_INVALID
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryLaunchCode(from: .launchDenied),
            SEYAL_APP_RECOVERY_LAUNCH_DENIED
        )
        XCTAssertEqual(
            ProductChromeHostView.recoveryLaunchCode(from: .launchFailed(42)),
            SEYAL_APP_RECOVERY_LAUNCH_FAILED
        )
    }
}
