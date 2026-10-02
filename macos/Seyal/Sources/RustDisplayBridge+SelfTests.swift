import Foundation

@MainActor
extension RustDisplayBridge {
  static func teardownReconnectStateSelfTest() -> Bool {
    var disconnects = 0
    let coordinator = RustBridgeTeardownCoordinator {
      disconnects += 1
    }
    coordinator.sourceCreated()
    coordinator.requestDisconnect()
    coordinator.requestDisconnect()
    guard coordinator.disconnectPending, disconnects == 0 else { return false }
    coordinator.sourceCancelled()
    let deadline = Date().addingTimeInterval(1)
    while disconnects == 0 && Date() < deadline {
      RunLoop.current.run(until: Date().addingTimeInterval(0.01))
    }
    return !coordinator.disconnectPending
      && coordinator.activeSourceCount == 0
      && disconnects == 1
  }

  static func pasteAdmissionSelfTest() -> Bool {
    pasteAdmissionCode("") == -4
      && pasteAdmissionCode("a") == 0
      && pasteAdmissionCode(String(repeating: "x", count: 65_536)) == 0
      && pasteAdmissionCode(String(repeating: "x", count: 65_537)) == -14
  }
}
