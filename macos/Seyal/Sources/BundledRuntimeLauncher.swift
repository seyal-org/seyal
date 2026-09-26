import Darwin
import Foundation
import Security

enum BundledRuntimeLaunchError: Error, Equatable {
  case helperMissing
  case helperPathInvalid
  case helperTrustInvalid
  case launchDenied
  case launchFailed(Int32)

  /// Stable, non-secret native diagnostic codes. These are deliberately
  /// separate from protocol/server error codes so the UI can present a
  /// bounded, actionable reconnect failure.
  var nativeCode: Int32 {
    switch self {
    case .helperMissing: return -30
    case .helperPathInvalid: return -31
    case .helperTrustInvalid: return -32
    case .launchDenied: return -33
    case .launchFailed: return -34
    }
  }
}

private final class BundledRuntimeLaunchErrorBox: NSObject {
  let error: BundledRuntimeLaunchError

  init(_ error: BundledRuntimeLaunchError) {
    self.error = error
  }
}

/// Launches the one permanent Runtime helper without transferring Runtime or
/// execution lifetime to the GUI. The Rust RecoveryCoordinator owns episode-level
/// launch-once accounting; this type owns package trust and spawn hygiene.
final class BundledRuntimeLauncher {
  static let helperIdentifier = "dev.seyal.Seyal.runtime"
  static let helperRelativePath = "Contents/Helpers/seyal-runtime"
  static let systemPath = "/usr/bin:/bin:/usr/sbin:/sbin"

  /// The launch is synchronous and currently invoked by the recovery
  /// coordinator on one executor. Retain its typed outcome only until that
  /// caller consumes it; this is an execution-local result relay, not Runtime
  /// state and never participates in terminal authority.
  private static let launchErrorThreadKey = "dev.seyal.runtime-launch-error"

  static func consumeLastLaunchError() -> BundledRuntimeLaunchError? {
    let dictionary = Thread.current.threadDictionary
    defer { dictionary.removeObject(forKey: launchErrorThreadKey) }
    return (dictionary[launchErrorThreadKey] as? BundledRuntimeLaunchErrorBox)?.error
  }

  private static func recordLaunchError(_ error: BundledRuntimeLaunchError?) {
    let dictionary = Thread.current.threadDictionary
    if let error {
      dictionary[launchErrorThreadKey] = BundledRuntimeLaunchErrorBox(error)
    } else {
      dictionary.removeObject(forKey: launchErrorThreadKey)
    }
  }

  @discardableResult
  func launch(bundleURL: URL = Bundle.main.bundleURL) -> Result<pid_t, BundledRuntimeLaunchError> {
    do {
      let helperURL = try Self.validateHelperPath(bundleURL: bundleURL)
      try Self.validateCodeSignature(bundleURL: bundleURL, helperURL: helperURL)
      let environment = try Self.launchEnvironment()
      let pid = try Self.spawn(helperURL: helperURL, environment: environment)
      reapWhenExited(pid)
      Self.recordLaunchError(nil)
      return .success(pid)
    } catch let error as BundledRuntimeLaunchError {
      Self.recordLaunchError(error)
      return .failure(error)
    } catch {
      Self.recordLaunchError(.launchDenied)
      return .failure(.launchDenied)
    }
  }

  static func validateHelperPath(
    bundleURL: URL,
    fileManager: FileManager = .default
  ) throws -> URL {
    let canonicalBundle = bundleURL.resolvingSymlinksInPath().standardizedFileURL
    let canonicalHelpers = canonicalBundle
      .appendingPathComponent("Contents/Helpers", isDirectory: true)
      .standardizedFileURL
    let helper = bundleURL.appendingPathComponent(helperRelativePath, isDirectory: false)

    var metadata = stat()
    guard lstat(helper.path, &metadata) == 0 else { throw BundledRuntimeLaunchError.helperMissing }
    guard metadata.st_mode & S_IFMT == S_IFREG else {
      throw BundledRuntimeLaunchError.helperPathInvalid
    }

    let canonicalHelper = helper.resolvingSymlinksInPath().standardizedFileURL
    guard canonicalHelper.lastPathComponent == "seyal-runtime",
      canonicalHelper.deletingLastPathComponent() == canonicalHelpers,
      fileManager.isExecutableFile(atPath: canonicalHelper.path)
    else {
      throw BundledRuntimeLaunchError.helperPathInvalid
    }
    return canonicalHelper
  }

  static func launchEnvironment(
    inherited: [String: String] = ProcessInfo.processInfo.environment
  ) throws -> [String: String] {
    let uid = geteuid()
    guard let account = getpwuid(uid),
      let namePointer = account.pointee.pw_name,
      let homePointer = account.pointee.pw_dir,
      let shellPointer = account.pointee.pw_shell
    else { throw BundledRuntimeLaunchError.launchDenied }

    let name = String(cString: namePointer)
    let home = String(cString: homePointer)
    let shell = String(cString: shellPointer)
    guard !name.isEmpty, home.first == "/", shell.first == "/" else {
      throw BundledRuntimeLaunchError.launchDenied
    }

    let temporaryDirectory = try darwinUserTemporaryDirectory(uid: uid)
    var result = [
      "HOME": home,
      "USER": name,
      "LOGNAME": name,
      "SHELL": shell,
      "TMPDIR": temporaryDirectory,
      "PATH": systemPath,
    ]
    for key in ["LANG", "LC_CTYPE"] {
      if let value = inherited[key], isValidLocale(value) {
        result[key] = value
      }
    }
    return result
  }

  static func isValidLocale(_ value: String) -> Bool {
    guard !value.isEmpty, value.utf8.count <= 128 else { return false }
    return value.unicodeScalars.allSatisfy { !CharacterSet.controlCharacters.contains($0) }
  }

  private static func darwinUserTemporaryDirectory(uid: uid_t) throws -> String {
    let required = confstr(_CS_DARWIN_USER_TEMP_DIR, nil, 0)
    guard required > 1 else { throw BundledRuntimeLaunchError.launchDenied }
    var buffer = [CChar](repeating: 0, count: required)
    guard confstr(_CS_DARWIN_USER_TEMP_DIR, &buffer, required) == required else {
      throw BundledRuntimeLaunchError.launchDenied
    }
    let path = String(decoding: buffer.dropLast().map { UInt8(bitPattern: $0) }, as: UTF8.self)
    var metadata = stat()
    guard path.first == "/", lstat(path, &metadata) == 0,
      metadata.st_mode & S_IFMT == S_IFDIR,
      metadata.st_uid == uid,
      metadata.st_mode & (S_IWGRP | S_IWOTH) == 0
    else { throw BundledRuntimeLaunchError.launchDenied }
    return path
  }

  private struct SignatureFacts {
    let code: SecStaticCode
    let identifier: String
    let teamIdentifier: String?
    let isAdHoc: Bool
    let hasEntitlements: Bool
  }

  private static func signatureFacts(for url: URL) throws -> SignatureFacts {
    var code: SecStaticCode?
    guard SecStaticCodeCreateWithPath(url as CFURL, [], &code) == errSecSuccess,
      let code
    else { throw BundledRuntimeLaunchError.helperTrustInvalid }
    let validationFlags = SecCSFlags(rawValue: kSecCSStrictValidate | kSecCSCheckAllArchitectures)
    guard SecStaticCodeCheckValidity(code, validationFlags, nil)
      == errSecSuccess
    else { throw BundledRuntimeLaunchError.helperTrustInvalid }

    var rawInformation: CFDictionary?
    guard SecCodeCopySigningInformation(
      code,
      SecCSFlags(rawValue: kSecCSSigningInformation),
      &rawInformation
    ) == errSecSuccess,
      let information = rawInformation as? [String: Any],
      let identifier = information[kSecCodeInfoIdentifier as String] as? String,
      let rawFlags = information[kSecCodeInfoFlags as String] as? NSNumber
    else { throw BundledRuntimeLaunchError.helperTrustInvalid }

    let entitlements = information[kSecCodeInfoEntitlementsDict as String] as? [String: Any]
    return SignatureFacts(
      code: code,
      identifier: identifier,
      teamIdentifier: information[kSecCodeInfoTeamIdentifier as String] as? String,
      // CS_ADHOC is a stable code-signing flag from <Security/CodeSigning.h>
      // but is not imported into Swift by every supported SDK.
      isAdHoc: rawFlags.uint32Value & 0x2 != 0,
      hasEntitlements: !(entitlements?.isEmpty ?? true)
    )
  }

  private static func validateCodeSignature(bundleURL: URL, helperURL: URL) throws {
    try evaluateHelperTrust(
      bundleURL: bundleURL,
      helperURL: helperURL,
      enforceReleaseRules: {
        #if DEBUG
          false
        #else
          true
        #endif
      }()
    )
  }

  /// Merge-acceptance / security-minimum seam. Production callers use
  /// `validateCodeSignature`; tests may force Release rules to prove ad-hoc
  /// helpers are rejected outside Debug.
  static func evaluateHelperTrust(
    bundleURL: URL,
    helperURL: URL,
    enforceReleaseRules: Bool
  ) throws {
    let app = try signatureFacts(for: bundleURL.resolvingSymlinksInPath())
    let helper = try signatureFacts(for: helperURL)
    guard helper.identifier == helperIdentifier, !helper.hasEntitlements else {
      throw BundledRuntimeLaunchError.helperTrustInvalid
    }

    if !enforceReleaseRules, app.isAdHoc && helper.isAdHoc { return }

    guard !app.isAdHoc, !helper.isAdHoc,
      let appTeam = app.teamIdentifier,
      !appTeam.isEmpty,
      helper.teamIdentifier == appTeam
    else { throw BundledRuntimeLaunchError.helperTrustInvalid }

    let requirementText =
      "anchor apple generic and identifier \"\(helperIdentifier)\" "
      + "and certificate leaf[subject.OU] = \"\(appTeam)\""
    var requirement: SecRequirement?
    guard SecRequirementCreateWithString(requirementText as CFString, [], &requirement)
      == errSecSuccess,
      let requirement,
      SecStaticCodeCheckValidity(
        helper.code,
        SecCSFlags(rawValue: kSecCSStrictValidate | kSecCSCheckAllArchitectures),
        requirement
      ) == errSecSuccess
    else { throw BundledRuntimeLaunchError.helperTrustInvalid }
  }

    static func helperArgv(
      executable: String,
      processArguments: [String] = ProcessInfo.processInfo.arguments,
      testHostLoaded: Bool = NSClassFromString("XCTestCase") != nil
    ) -> [String] {
      var argv = [executable]
      argv.append(
        contentsOf: IsolatedRuntimeDirectory.helperArguments(
          from: processArguments,
          testHostLoaded: testHostLoaded
        )
      )
      return argv
    }

    private static func spawn(helperURL: URL, environment: [String: String]) throws -> pid_t {
    let nullFD = open("/dev/null", O_RDWR | O_CLOEXEC)
    guard nullFD >= 0 else { throw BundledRuntimeLaunchError.launchDenied }
    defer { close(nullFD) }

    var actions: posix_spawn_file_actions_t?
    var attributes: posix_spawnattr_t?
    guard posix_spawn_file_actions_init(&actions) == 0,
      posix_spawnattr_init(&attributes) == 0
    else { throw BundledRuntimeLaunchError.launchDenied }
    defer {
      posix_spawn_file_actions_destroy(&actions)
      posix_spawnattr_destroy(&attributes)
    }

    for descriptor in [STDIN_FILENO, STDOUT_FILENO, STDERR_FILENO] {
      guard posix_spawn_file_actions_adddup2(&actions, nullFD, descriptor) == 0 else {
        throw BundledRuntimeLaunchError.launchDenied
      }
    }
    if nullFD > STDERR_FILENO {
      guard posix_spawn_file_actions_addclose(&actions, nullFD) == 0 else {
        throw BundledRuntimeLaunchError.launchDenied
      }
    }

    let flags = Int16(POSIX_SPAWN_CLOEXEC_DEFAULT | POSIX_SPAWN_SETPGROUP)
    guard posix_spawnattr_setflags(&attributes, flags) == 0,
      posix_spawnattr_setpgroup(&attributes, 0) == 0
    else { throw BundledRuntimeLaunchError.launchDenied }

    let executable = helperURL.path
    var arguments: [UnsafeMutablePointer<CChar>?] = helperArgv(executable: executable)
      .map { strdup($0) as UnsafeMutablePointer<CChar>? }
    arguments.append(nil)
    var environmentPointers = environment
      .sorted { $0.key < $1.key }
      .map { strdup("\($0.key)=\($0.value)") as UnsafeMutablePointer<CChar>? }
    environmentPointers.append(nil)
    defer {
      arguments.compactMap { $0 }.forEach { free(UnsafeMutableRawPointer($0)) }
      environmentPointers.compactMap { $0 }.forEach { free(UnsafeMutableRawPointer($0)) }
    }

    var pid = pid_t()
    let status = executable.withCString { executablePointer in
      arguments.withUnsafeMutableBufferPointer { argumentBuffer in
        environmentPointers.withUnsafeMutableBufferPointer { environmentBuffer in
          posix_spawn(
            &pid,
            executablePointer,
            &actions,
            &attributes,
            argumentBuffer.baseAddress!,
            environmentBuffer.baseAddress!
          )
        }
      }
    }
    guard status == 0 else { throw BundledRuntimeLaunchError.launchFailed(status) }
    return pid
  }

  private func reapWhenExited(_ pid: pid_t) {
    let source = DispatchSource.makeProcessSource(
      identifier: pid,
      eventMask: .exit,
      queue: DispatchQueue.global(qos: .utility)
    )
    // The source intentionally retains itself until exit so GUI object teardown
    // cannot turn source lifetime into Runtime lifetime. It only reaps the PID.
    source.setEventHandler { [source] in
      var status: Int32 = 0
      while waitpid(pid, &status, 0) == -1 && errno == EINTR {}
      source.cancel()
    }
    source.resume()
  }
}
