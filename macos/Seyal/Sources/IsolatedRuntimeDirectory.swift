import Foundation

/// Explicit isolated Runtime namespace for this process.
///
/// Production Seyal.app without `--runtime-dir` keeps the canonical per-user
/// endpoint. XCTest hosts synthesize a unique directory so headed
/// `make ui-test` / `native-macos-smoke` can run while a user Runtime is
/// already active. `make check` does not launch Seyal.app. Environment
/// variables never select that endpoint.
enum IsolatedRuntimeDirectory {
  static let flag = "--runtime-dir"

  private static let testHostDirectory: String = makeUniqueDirectory(prefix: "s860th")

  static func installIfNeeded(
    arguments: [String] = ProcessInfo.processInfo.arguments,
    testHostLoaded: Bool = NSClassFromString("XCTestCase") != nil
  ) {
    guard let directory = selectedDirectory(from: arguments, testHostLoaded: testHostLoaded)
    else {
      return
    }
    directory.withCString { pointer in
      let status = seyal_bridge_set_runtime_dir(pointer)
      precondition(status == 0, "\(flag) requires an absolute Runtime directory")
    }
  }

  static func helperArguments(
    from arguments: [String] = ProcessInfo.processInfo.arguments,
    testHostLoaded: Bool = NSClassFromString("XCTestCase") != nil,
    forwardHelperCommand: Bool = false
  ) -> [String] {
    guard let directory = selectedDirectory(from: arguments, testHostLoaded: testHostLoaded)
    else {
      return []
    }
    var forwarded = [flag, directory]
    // Production / Release Seyal.app must not accept a Runtime command from
    // app argv. The unit-test host loads XCTest. Debug XCUI sets
    // `forwardHelperCommand` via a Debug-only launch-environment gate;
    // Release compiles that gate out. Flow tests pass `/bin/zsh` because a
    // bash account shell is full-pane Raw.
    if testHostLoaded || forwardHelperCommand {
      if let index = arguments.firstIndex(of: flag) {
        let valueIndex = arguments.index(after: index)
        if valueIndex < arguments.endIndex {
          let commandStart = arguments.index(after: valueIndex)
          if commandStart < arguments.endIndex {
            forwarded.append(contentsOf: arguments[commandStart...])
          }
        }
      }
      // XCTest host (component tests) often has no trailing command and no
      // `--runtime-dir` in process argv — only a synthesized directory. Under
      // true P1 empty-argv, the helper would create no execution and headed
      // component smoke would hang. Supply a Flow-capable default shell for
      // the test host only; production empty-argv stays empty.
      if testHostLoaded, forwarded.count == 2 {
        forwarded.append("/bin/zsh")
      }
    }
    return forwarded
  }

  static func explicitDirectory(from arguments: [String]) -> String? {
    guard let index = arguments.firstIndex(of: flag) else { return nil }
    let valueIndex = arguments.index(after: index)
    guard valueIndex < arguments.endIndex else { return nil }
    let value = arguments[valueIndex]
    guard value.hasPrefix("/") else { return nil }
    return value
  }

  static func selectedDirectory(from arguments: [String], testHostLoaded: Bool) -> String? {
    if arguments.contains(flag) {
      guard let directory = explicitDirectory(from: arguments) else {
        fatalError("\(flag) requires an absolute directory path")
      }
      return directory
    }
    if testHostLoaded {
      return testHostDirectory
    }
    return nil
  }

  private static func makeUniqueDirectory(prefix: String) -> String {
    let url = URL(fileURLWithPath: "/tmp").appendingPathComponent(
      "\(prefix)-\(ProcessInfo.processInfo.processIdentifier)",
      isDirectory: true
    )
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    try? FileManager.default.setAttributes(
      [.posixPermissions: 0o700],
      ofItemAtPath: url.path
    )
    return url.path
  }
}
