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
  static func makeLaunchArguments(command: String = "/bin/zsh") -> [String] {
    [flag, makeDirectory(), command]
  }

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
  /// Blocks-first (#1426): supported shells rest in Flow. Treat the Blocks
  /// transcript as the Flow oracle — the composer dock may be retracted while
  /// Busy or Block-scoped DirectInput owns a Running Block (#1466), which is
  /// still Flow and must not be mistaken for Raw resting.
  ///
  /// `presentation.toggle_raw` has no builtin Cmd+Enter / View menu admission
  /// (`supported_shell_menu_and_builtins_do_not_expose_raw_presentation`). When
  /// unsupported Raw offers palette "Use Flow", use that escape instead.
  func ensureFlowBlocksPresentation(timeout: TimeInterval = 8) {
    let blocks = descendants(matching: .any)["seyal-blocks"].firstMatch
    let transcript = descendants(matching: .any)["seyal-blocks-scroll"].firstMatch
    let composer = descendants(matching: .any)["seyal-composer"].firstMatch

    if flowBlocksSurfaceVisible(blocks: blocks, transcript: transcript) {
      return
    }
    if composer.waitForExistence(timeout: min(1, timeout)), composer.isHittable {
      return
    }

    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
      if flowBlocksSurfaceVisible(blocks: blocks, transcript: transcript) {
        return
      }
      if composer.exists, composer.isHittable {
        return
      }
      RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    }

    // Unsupported Raw may expose palette "Use Flow" (resting_palette_choice).
    typeKey("k", modifierFlags: .command)
    let palette = descendants(matching: .any)["seyal-command-palette"].firstMatch
    if palette.waitForExistence(timeout: 2) {
      typeText("Use Flow")
      typeKey("\r", modifierFlags: [])
      waitBrieflyForFlowLatch(0.4)
      typeKey("\u{1b}", modifierFlags: []) // dismiss if still open
    }

    XCTAssertTrue(
      flowBlocksSurfaceVisible(blocks: blocks, transcript: transcript)
        || (composer.exists && composer.isHittable),
      "Flow/Blocks XCUI must show the Blocks transcript (or a hittable composer); "
        + "Busy/DirectInput may hide the dock without leaving Flow"
    )
  }

  private func flowBlocksSurfaceVisible(
    blocks: XCUIElement,
    transcript: XCUIElement
  ) -> Bool {
    blocks.exists && transcript.exists && transcript.frame.height > 120
  }

  private func waitBrieflyForFlowLatch(_ seconds: TimeInterval) {
    RunLoop.current.run(until: Date().addingTimeInterval(seconds))
  }

  @discardableResult
  func launchIsolatedHost(
    environment: [String: String] = [:],
    helperCommand: String = "/bin/zsh"
  ) -> XCUIApplication {
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
    launchArguments += IsolatedHostedRuntime.makeLaunchArguments(command: helperCommand)
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
