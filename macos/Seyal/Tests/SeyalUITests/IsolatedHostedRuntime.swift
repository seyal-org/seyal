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

  /// UI test runners are sandboxed and cannot create children directly under
  /// `/tmp`. Its private temporary directory is writable by the runner, and
  /// the non-sandboxed app under test can still use the explicit path.
  private static var temporaryRoot: URL {
    FileManager.default.temporaryDirectory
  }

  static func makeDirectory() -> String {
    let url = temporaryRoot.appendingPathComponent(
      // Keep the AF_UNIX control socket path below sun_path's limit even
      // inside XCTest's long sandboxed temporary-directory path.
      "r\(UUID().uuidString.prefix(8))",
      isDirectory: true
    )
    XCTAssertLessThanOrEqual(
      url.path.utf8.count + 1 + "control.sock".utf8.count,
      103,
      "isolated Runtime socket path must fit sockaddr_un.sun_path"
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
    let url = temporaryRoot.appendingPathComponent(
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
    // Multi-window / multi-PTY cases can leave the previous Seyal.app shutting
    // down for >1s. Launching the next isolated host before `.notRunning`
    // races Runtime bootstrap and surfaces as `connection=disconnected
    // runtime=none` on the first waitForUsablePty (C2b ⌘W after ⌘N×2 on CI).
    terminate()
    XCTAssertTrue(
      wait(for: .notRunning, timeout: 8),
      "prior Seyal.app must exit before the next isolated Runtime launch"
    )
    // Always mint a fresh `--runtime-dir`. Reconnect-without-kill cases reuse
    // arguments via `terminate()` + `launch()` and must not call this helper.
    stripIsolatedRuntimeLaunchArguments()
    launchArguments += IsolatedHostedRuntime.makeLaunchArguments()
    for (key, value) in environment {
      launchEnvironment[key] = value
    }
    launchEnvironment[IsolatedHostedRuntime.forwardCommandEnvironmentKey] = "1"
    // Refresh the shell fixture each isolated launch so a prior env entry
    // cannot point at a deleted temp path after suite cleanup.
    if environment[IsolatedHostedRuntime.runtimeConfigEnvironmentKey] == nil {
      launchEnvironment[IsolatedHostedRuntime.runtimeConfigEnvironmentKey] =
        IsolatedHostedRuntime.writeShellConfig()
    }
    launch()
    return self
  }

  private func stripIsolatedRuntimeLaunchArguments() {
    guard let flagIndex = launchArguments.firstIndex(of: IsolatedHostedRuntime.flag) else {
      return
    }
    // `--runtime-dir <dir> [/bin/zsh]`
    var end = flagIndex + 1
    if end < launchArguments.count {
      end += 1
    }
    if end < launchArguments.count, launchArguments[end].hasPrefix("/") {
      end += 1
    }
    launchArguments.removeSubrange(flagIndex..<end)
  }
}
