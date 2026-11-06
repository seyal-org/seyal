import Darwin
import Foundation

/// Explicit isolated Runtime namespace for this process.
///
/// Production Seyal.app without `--runtime-dir` keeps the canonical per-user
/// endpoint. XCTest hosts synthesize a unique directory so headed
/// `make ui-test` / `native-macos-smoke` can run while a user Runtime is
/// already active. `make check` does not launch Seyal.app. Environment
/// variables never select that endpoint.
///
/// Explicit `--runtime-dir` leaves are host-prepared as `0700` before the
/// helper binds: Runtime discovery still fail-closes on insecure leaves and
/// does not chmod-repair (#1428).
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
    // Host prep is best-effort here: a failed prepare still installs the
    // absolute path so Runtime can report the typed discovery error.
    _ = prepareExplicitRuntimeDirectory(directory)
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
    // Best-effort host prep so disposable `/tmp/...` recipes and XCTest dirs
    // are 0700 before the helper's create_verified_runtime_dir runs.
    _ = prepareExplicitRuntimeDirectory(directory)
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

  /// Create missing parents and ensure the leaf is an euid-owned `0700` directory.
  /// Existing parents such as `/tmp` are accepted without chmod; the leaf is
  /// never a symlink and never left group/world-accessible.
  @discardableResult
  static func prepareExplicitRuntimeDirectory(_ path: String) -> Bool {
    guard path.hasPrefix("/"), path != "/" else { return false }
    let url = URL(fileURLWithPath: path, isDirectory: true).standardizedFileURL
    let parent = url.deletingLastPathComponent().path
    guard ensureExistingOrCreateParents(parent) else { return false }
    return ensureOwnedPrivateLeaf(url.path)
  }

  private static func makeUniqueDirectory(prefix: String) -> String {
    let url = URL(fileURLWithPath: "/tmp").appendingPathComponent(
      "\(prefix)-\(ProcessInfo.processInfo.processIdentifier)",
      isDirectory: true
    )
    // Static type init must not trap: XCTest host load would crash the runner.
    _ = prepareExplicitRuntimeDirectory(url.path)
    return url.path
  }

  private static func ensureExistingOrCreateParents(_ path: String) -> Bool {
    if path.isEmpty || path == "/" { return true }
    var st = stat()
    // Follow symlinks for parents: Darwin `/tmp` is a symlink to
    // `/private/tmp`. Leaf verification still uses lstat so a symlink
    // runtime-dir remains fail-closed.
    if stat(path, &st) == 0 {
      return (st.st_mode & S_IFMT) == S_IFDIR
    }
    guard errno == ENOENT else { return false }
    let parent = URL(fileURLWithPath: path).deletingLastPathComponent().path
    let parentPath = parent == path ? "/" : parent
    guard ensureExistingOrCreateParents(parentPath) else { return false }
    if mkdir(path, 0o700) == 0 { return true }
    if errno == EEXIST {
      return stat(path, &st) == 0 && (st.st_mode & S_IFMT) == S_IFDIR
    }
    return false
  }

  private static func ensureOwnedPrivateLeaf(_ path: String) -> Bool {
    var st = stat()
    if lstat(path, &st) == 0 {
      guard (st.st_mode & S_IFMT) == S_IFDIR else { return false }
      guard st.st_uid == geteuid() else { return false }
      return chmod(path, 0o700) == 0
    }
    guard errno == ENOENT else { return false }
    if mkdir(path, 0o700) == 0 { return true }
    if errno == EEXIST {
      return ensureOwnedPrivateLeaf(path)
    }
    return false
  }
}
