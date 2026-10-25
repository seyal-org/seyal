import XCTest

/// XCUI launches a separate Seyal.app that does not load XCTest, so isolation
/// must be explicit `--runtime-dir` launch arguments.
///
/// Each `XCUIApplication` gets its own directory so sequential cases do not
/// inherit a dirty PTY. `terminate` + `launch()` keeps the same arguments so
/// the relaunch-reconnect case stays on one fixture Runtime.
enum IsolatedHostedRuntime {
  static let flag = "--runtime-dir"
  /// Must match `BundledRuntimeLauncher.uiTestForwardRuntimeCommandEnvironmentKey`
  /// (Debug-only in Seyal.app). Release builds ignore this variable.
  /// Xcode leaves `XCTestConfigurationFilePath` on the test runner, so the
  /// app under test only sees a variable this launch sets.
  static let forwardCommandEnvironmentKey = "SEYAL_UI_TEST_FORWARD_RUNTIME_COMMAND"

  static func makeDirectory() -> String {
    let url = URL(fileURLWithPath: "/tmp").appendingPathComponent(
      "s860ui-\(ProcessInfo.processInfo.processIdentifier)-\(UUID().uuidString.prefix(8))",
      isDirectory: true
    )
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    try? FileManager.default.setAttributes(
      [.posixPermissions: 0o700],
      ofItemAtPath: url.path
    )
    return url.path
  }

  /// `/bin/zsh` is the helper command so Flow tests run with trusted
  /// integration. The account `pw_shell` on hosted runners is bash, which
  /// SPEC-008 presents as full-pane Raw. The argv command only seeds the
  /// first execution; profile 0 (Split Right) reads `[shell].program`.
  static func makeLaunchArguments() -> [String] { [flag, makeDirectory(), "/bin/zsh"] }

  static let runtimeConfigEnvironmentKey = "SEYAL_UI_TEST_RUNTIME_CONFIG"

  /// Fixture `[shell].program` so every profile-0 execution is zsh.
  /// A missing file must fail the test; the helper ignores a path that is not on disk
  /// and would otherwise fall back to the account shell.
  static func writeShellConfig() -> String {
    let url = URL(fileURLWithPath: "/tmp").appendingPathComponent(
      "s936-shell-\(ProcessInfo.processInfo.processIdentifier)-\(UUID().uuidString.prefix(8)).toml"
    )
    let text = "[shell]\nprogram = \"/bin/zsh\"\n"
    do {
      try text.write(to: url, atomically: true, encoding: .utf8)
    } catch {
      XCTFail("profile-0 zsh fixture was not written: \(error)")
    }
    if !FileManager.default.fileExists(atPath: url.path) {
      XCTFail("profile-0 zsh fixture is missing at \(url.path)")
    }
    return url.path
  }
}

extension XCUIApplication {
  @discardableResult
  func launchIsolatedHost(environment: [String: String] = [:]) -> XCUIApplication {
    terminate()
    if !launchArguments.contains(IsolatedHostedRuntime.flag) {
      launchArguments += IsolatedHostedRuntime.makeLaunchArguments()
    }
    for (key, value) in environment {
      launchEnvironment[key] = value
    }
    launchEnvironment[IsolatedHostedRuntime.forwardCommandEnvironmentKey] = "1"
    if launchEnvironment[IsolatedHostedRuntime.runtimeConfigEnvironmentKey] == nil {
      launchEnvironment[IsolatedHostedRuntime.runtimeConfigEnvironmentKey] =
        IsolatedHostedRuntime.writeShellConfig()
    }
    launch()
    return self
  }
}
