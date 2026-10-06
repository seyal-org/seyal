import AppKit
import Darwin
import Foundation
import Security
import Sparkle

/// Isolated Sparkle 2.9.6 harness. Rust-owned policy is simulated by a policy
/// file the host reads before every install reply. This process never presents
/// a Sparkle standard window.

private let spikeRoot = URL(fileURLWithPath: "/tmp/seyal-spike-688", isDirectory: true)

private struct SpikePolicy: Equatable {
    var allowDownload = false
    var allowReadyInstall = false
    var callInstallOnQuitHandler = false
    var enforceTeamContinuity = false
    var blockProceed = false

    static func load(from url: URL) -> SpikePolicy {
        guard let data = try? Data(contentsOf: url),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return SpikePolicy()
        }
        func flag(_ key: String) -> Bool { object[key] as? Bool ?? false }
        return SpikePolicy(
            allowDownload: flag("allowDownload"),
            allowReadyInstall: flag("allowReadyInstall"),
            callInstallOnQuitHandler: flag("callInstallOnQuitHandler"),
            enforceTeamContinuity: flag("enforceTeamContinuity"),
            blockProceed: flag("blockProceed")
        )
    }
}

private enum SpikeLog {
    static let url: URL = {
        let directory = spikeRoot.appendingPathComponent(bundleID, isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory.appendingPathComponent("harness.log")
    }()

    static func write(_ message: String) {
        let line = "\(isoNow()) \(message)\n"
        FileHandle.standardError.write(Data(line.utf8))
        if let data = line.data(using: .utf8) {
            if FileManager.default.fileExists(atPath: url.path) {
                if let handle = try? FileHandle(forWritingTo: url) {
                    _ = try? handle.seekToEnd()
                    try? handle.write(contentsOf: data)
                    try? handle.close()
                }
            } else {
                try? data.write(to: url)
            }
        }
    }
}

private var bundleID: String {
    Bundle.main.bundleIdentifier ?? "dev.seyal.spike688.harness"
}

private func isoNow() -> String {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter.string(from: Date())
}

private func spikeDirectory() -> URL {
    let directory = spikeRoot.appendingPathComponent(bundleID, isDirectory: true)
    try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    return directory
}

private func policyURL() -> URL {
    spikeDirectory().appendingPathComponent("policy.json")
}

private func commandURL() -> URL {
    spikeDirectory().appendingPathComponent("command")
}

private func feedURLFile() -> URL {
    spikeDirectory().appendingPathComponent("feed-url")
}

private func runtimeDirectory() -> URL {
    let directory = spikeDirectory().appendingPathComponent("runtime", isDirectory: true)
    try? FileManager.default.createDirectory(
        at: directory,
        withIntermediateDirectories: true,
        attributes: [.posixPermissions: 0o700]
    )
    return directory
}

private func visibleWindows() -> String {
    let descriptions = NSApp.windows.map { window in
        "\(type(of: window)):visible=\(window.isVisible)"
    }
    return descriptions.isEmpty ? "none" : descriptions.joined(separator: ",")
}

private func teamIdentifier(of url: URL) -> String? {
    var code: SecStaticCode?
    let status = SecStaticCodeCreateWithPath(url as CFURL, [], &code)
    guard status == errSecSuccess, let code else { return nil }
    var information: CFDictionary?
    let copied = SecCodeCopySigningInformation(
        code,
        SecCSFlags(rawValue: kSecCSSigningInformation),
        &information
    )
    guard copied == errSecSuccess, let dictionary = information as? [String: Any] else {
        return nil
    }
    return dictionary[kSecCodeInfoTeamIdentifier as String] as? String
}

private func recentBundles(under root: URL, maxDepth: Int) -> [URL] {
    guard let enumerator = FileManager.default.enumerator(
        at: root,
        includingPropertiesForKeys: [.isDirectoryKey],
        options: [.skipsHiddenFiles]
    ) else { return [] }
    var found: [URL] = []
    let rootComponents = root.pathComponents.count
    for case let url as URL in enumerator {
        if url.pathComponents.count - rootComponents > maxDepth {
            enumerator.skipDescendants()
            continue
        }
        if url.pathExtension == "app" {
            found.append(url)
            enumerator.skipDescendants()
        }
    }
    return found
}

@MainActor
final class SpikeUserDriver: NSObject, SPUUserDriver {
    func show(
        _ request: SPUUpdatePermissionRequest,
        reply: @escaping (SUUpdatePermissionResponse) -> Void
    ) {
        _ = request
        SpikeLog.write("user-driver permission-request windows=\(visibleWindows()) reply=deny-without-ui")
        reply(SUUpdatePermissionResponse(automaticUpdateChecks: false, sendSystemProfile: false))
    }

    func showUserInitiatedUpdateCheck(cancellation: @escaping () -> Void) {
        _ = cancellation
        SparkLog.note("user-driver showUserInitiatedUpdateCheck windows=\(visibleWindows())")
    }

    func showUpdateFound(with appcastItem: SUAppcastItem, state: SPUUserUpdateState, reply: @escaping (SPUUserUpdateChoice) -> Void) {
        let policy = SpikePolicy.load(from: policyURL())
        let seyal = seyalProperties(appcastItem)
        SparkLog.note(
            "user-driver showUpdateFound stage=\(state.stage.rawValue) userInitiated=\(state.userInitiated) version=\(appcastItem.versionString) seyal=\(seyal) windows=\(visibleWindows()) before-download=\(state.stage == .notDownloaded)"
        )
        let choice: SPUUserUpdateChoice = policy.allowDownload ? .install : .dismiss
        SparkLog.note("user-driver update-choice=\(choice.rawValue) allowDownload=\(policy.allowDownload)")
        reply(choice)
    }

    func showUpdateReleaseNotes(with downloadData: SPUDownloadData) {
        SparkLog.note("user-driver release-notes bytes=\(downloadData.data.count) windows=\(visibleWindows())")
    }

    func showUpdateReleaseNotesFailedToDownloadWithError(_ error: Error) {
        SparkLog.note("user-driver release-notes-error \(error.localizedDescription) windows=\(visibleWindows())")
    }

    func showUpdateNotFoundWithError(_ error: Error, acknowledgement: @escaping () -> Void) {
        SparkLog.note("user-driver update-not-found \(error.localizedDescription) windows=\(visibleWindows())")
        acknowledgement()
    }

    func showUpdaterError(_ error: Error, acknowledgement: @escaping () -> Void) {
        let nsError = error as NSError
        SparkLog.note(
            "user-driver updater-error domain=\(nsError.domain) code=\(nsError.code) \(nsError.localizedDescription) windows=\(visibleWindows())"
        )
        acknowledgement()
    }

    func showDownloadInitiated(cancellation: @escaping () -> Void) {
        _ = cancellation
        SparkLog.note("user-driver download-initiated windows=\(visibleWindows())")
    }

    func showDownloadDidReceiveExpectedContentLength(_ expectedContentLength: UInt64) {
        SparkLog.note("user-driver download-length=\(expectedContentLength)")
    }

    func showDownloadDidReceiveData(ofLength length: UInt64) {
        SparkLog.note("user-driver download-chunk=\(length)")
    }

    func showDownloadDidStartExtractingUpdate() {
        SparkLog.note("user-driver extract-start windows=\(visibleWindows())")
    }

    func showExtractionReceivedProgress(_ progress: Double) {
        SparkLog.note(String(format: "user-driver extract-progress=%.2f", progress))
    }

    func showReady(toInstallAndRelaunch reply: @escaping (SPUUserUpdateChoice) -> Void) {
        let policy = SpikePolicy.load(from: policyURL())
        let hostTeam = teamIdentifier(of: Bundle.main.bundleURL) ?? "none"
        let extracted = extractedAppTeams()
        SparkLog.note(
            "user-driver ready-to-install hostTeam=\(hostTeam) extracted=\(extracted) windows=\(visibleWindows())"
        )
        if policy.enforceTeamContinuity {
            let foreign = extracted.contains { team, _ in team != hostTeam }
            let unknown = extracted.isEmpty
            if foreign || unknown {
                SparkLog.note("user-driver team-continuity-blocked foreign=\(foreign) unknown=\(unknown)")
                reply(.skip)
                return
            }
        }
        let choice: SPUUserUpdateChoice = policy.allowReadyInstall ? .install : .dismiss
        SparkLog.note("user-driver ready-choice=\(choice.rawValue) allowReadyInstall=\(policy.allowReadyInstall)")
        reply(choice)
    }

    func showInstallingUpdate(withApplicationTerminated applicationTerminated: Bool, retryTerminatingApplication: @escaping () -> Void) {
        _ = retryTerminatingApplication
        SparkLog.note("user-driver installing terminated=\(applicationTerminated) windows=\(visibleWindows())")
    }

    func showUpdateInstalledAndRelaunched(_ relaunched: Bool, acknowledgement: @escaping () -> Void) {
        SparkLog.note("user-driver installed relaunched=\(relaunched) windows=\(visibleWindows())")
        acknowledgement()
    }

    func dismissUpdateInstallation() {
        SparkLog.note("user-driver dismiss windows=\(visibleWindows())")
    }

    private func seyalProperties(_ item: SUAppcastItem) -> String {
        let pairs = item.propertiesDictionary.compactMap { key, value -> String? in
            let name = String(describing: key)
            guard name.contains("seyal") else { return nil }
            return "\(name)=\(value)"
        }
        return pairs.isEmpty ? "absent" : pairs.joined(separator: "|")
    }

    private func extractedAppTeams() -> [(String, String)] {
        var roots: [URL] = []
        if let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first {
            roots.append(caches.appendingPathComponent(bundleID, isDirectory: true))
        }
        roots.append(spikeDirectory())
        var results: [(String, String)] = []
        for root in roots where FileManager.default.fileExists(atPath: root.path) {
            for app in recentBundles(under: root, maxDepth: 8) {
                let team = teamIdentifier(of: app) ?? "none"
                results.append((team, app.lastPathComponent))
            }
        }
        return results
    }
}

private enum SparkLog {
    static func note(_ message: String) {
        SpikeLog.write(message)
    }
}

@MainActor
final class SpikeDelegate: NSObject, SPUUpdaterDelegate {
    func feedURLString(for updater: SPUUpdater) -> String? {
        _ = updater
        let raw = (try? String(contentsOf: feedURLFile(), encoding: .utf8))?.trimmingCharacters(in: .whitespacesAndNewlines)
        SparkLog.note("delegate feed=\(raw ?? "missing")")
        return raw
    }

    func updaterShouldPromptForPermissionToCheck(forUpdates updater: SPUUpdater) -> Bool {
        _ = updater
        SparkLog.note("delegate permission-prompt=false")
        return false
    }

    func updater(_ updater: SPUUpdater, didFinishLoading appcast: SUAppcast) {
        _ = updater
        let summaries = appcast.items.map { item -> String in
            let seyal = item.propertiesDictionary.keys
                .map { String(describing: $0) }
                .filter { $0.contains("seyal") }
                .joined(separator: ",")
            return "version=\(item.versionString) seyalKeys=\(seyal.isEmpty ? "none" : seyal) file=\(item.fileURL?.absoluteString ?? "none")"
        }
        SparkLog.note("delegate appcast-loaded count=\(appcast.items.count) \(summaries.joined(separator: " ;; ")) windows=\(visibleWindows())")
    }

    func updater(_ updater: SPUUpdater, didFindValidUpdate item: SUAppcastItem) {
        _ = updater
        SparkLog.note("delegate valid-update version=\(item.versionString) signing=\(item.signingValidationStatus.rawValue)")
    }

    func updater(_ updater: SPUUpdater, shouldProceedWithUpdate updateItem: SUAppcastItem, updateCheck: SPUUpdateCheck) throws {
        _ = updater
        _ = updateCheck
        let policy = SpikePolicy.load(from: policyURL())
        let seyal = updateItem.propertiesDictionary
            .filter { String(describing: $0.key).contains("seyal") }
            .map { "\(String(describing: $0.key))=\($0.value)" }
            .joined(separator: "|")
        SparkLog.note("delegate should-proceed version=\(updateItem.versionString) seyal=\(seyal.isEmpty ? "absent" : seyal) block=\(policy.blockProceed)")
        if policy.blockProceed {
            throw NSError(
                domain: "dev.seyal.spike688",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "host policy blocked update before download"]
            )
        }
    }

    func updater(_ updater: SPUUpdater, willDownloadUpdate item: SUAppcastItem, with request: NSMutableURLRequest) {
        _ = updater
        SparkLog.note("delegate will-download version=\(item.versionString) url=\(request.url?.absoluteString ?? "none")")
    }

    func updater(_ updater: SPUUpdater, didExtractUpdate item: SUAppcastItem) {
        _ = updater
        SparkLog.note("delegate did-extract version=\(item.versionString)")
    }

    func updater(_ updater: SPUUpdater, willInstallUpdate item: SUAppcastItem) {
        _ = updater
        SparkLog.note("delegate will-install version=\(item.versionString) windows=\(visibleWindows())")
    }

    func updater(
        _ updater: SPUUpdater,
        willInstallUpdateOnQuit item: SUAppcastItem,
        immediateInstallationBlock immediateInstallHandler: @escaping () -> Void
    ) -> Bool {
        _ = updater
        let policy = SpikePolicy.load(from: policyURL())
        SparkLog.note(
            "delegate will-install-on-quit version=\(item.versionString) callHandler=\(policy.callInstallOnQuitHandler) windows=\(visibleWindows())"
        )
        if policy.callInstallOnQuitHandler {
            immediateInstallHandler()
        }
        return true
    }

    func updaterShouldRelaunchApplication(_ updater: SPUUpdater) -> Bool {
        _ = updater
        let policy = SpikePolicy.load(from: policyURL())
        let relaunch = policy.allowReadyInstall || policy.callInstallOnQuitHandler
        SparkLog.note("delegate should-relaunch=\(relaunch)")
        return relaunch
    }

    func updater(_ updater: SPUUpdater, didAbortWithError error: Error) {
        _ = updater
        let nsError = error as NSError
        SparkLog.note("delegate abort domain=\(nsError.domain) code=\(nsError.code) \(nsError.localizedDescription)")
    }

    func updater(_ updater: SPUUpdater, didFinishUpdateCycleFor updateCheck: SPUUpdateCheck, error: Error?) {
        _ = updater
        _ = updateCheck
        if let error {
            let nsError = error as NSError
            SparkLog.note("delegate cycle-finished error domain=\(nsError.domain) code=\(nsError.code) \(nsError.localizedDescription)")
        } else {
            SparkLog.note("delegate cycle-finished ok")
        }
    }
}

@MainActor
final class SpikeApp: NSObject, NSApplicationDelegate {
    private let driver = SpikeUserDriver()
    private let delegate = SpikeDelegate()
    private var updater: SPUUpdater?
    private var runtimePID: pid_t = 0

    func applicationDidFinishLaunching(_ notification: Notification) {
        _ = notification
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "?"
        SparkLog.note("launch version=\(version) bundle=\(bundleID) windows=\(visibleWindows())")
        if helloAttach() {
            SparkLog.note("hello-attach existing-runtime pid-not-spawned")
        } else {
            spawnRuntime()
        }
        let updater = SPUUpdater(
            hostBundle: Bundle.main,
            applicationBundle: Bundle.main,
            userDriver: driver,
            delegate: delegate
        )
        self.updater = updater
        let autoFile = spikeDirectory().appendingPathComponent("auto-download")
        let autoFlag = (try? String(contentsOf: autoFile, encoding: .utf8))?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if ProcessInfo.processInfo.environment["SPIKE_AUTO_DOWNLOAD"] == "1" || autoFlag == "1" {
            updater.automaticallyDownloadsUpdates = true
            SparkLog.note("auto-download=enabled")
        }
        do {
            try updater.start()
            SparkLog.note("updater-started")
        } catch {
            SparkLog.note("updater-start-failed \(error.localizedDescription)")
        }
        Timer.scheduledTimer(withTimeInterval: 0.4, repeats: true) { _ in
            Task { @MainActor in
                self.pollCommand()
            }
        }
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        _ = sender
        SparkLog.note("app-should-terminate runtimePID=\(runtimePID) windows=\(visibleWindows())")
        return .terminateNow
    }

    private func pollCommand() {
        let url = commandURL()
        guard let raw = try? String(contentsOf: url, encoding: .utf8) else { return }
        let command = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !command.isEmpty else { return }
        try? "".write(to: url, atomically: true, encoding: .utf8)
        SparkLog.note("command \(command)")
        switch command {
        case "check":
            updater?.checkForUpdates()
        case "quit":
            NSApp.terminate(nil)
        case "ping":
            SparkLog.note("pong version=\(Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "?") windows=\(visibleWindows())")
        default:
            SparkLog.note("unknown-command")
        }
    }

    private func helloAttach() -> Bool {
        let socketPath = runtimeDirectory().appendingPathComponent("control.sock").path
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { return false }
        defer { close(fd) }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = socketPath.utf8CString
        guard bytes.count <= MemoryLayout.size(ofValue: address.sun_path) else { return false }
        withUnsafeMutablePointer(to: &address.sun_path) { pointer in
            pointer.withMemoryRebound(to: CChar.self, capacity: bytes.count) { raw in
                for (index, byte) in bytes.enumerated() {
                    raw[index] = byte
                }
            }
        }
        let length = socklen_t(MemoryLayout<sockaddr_un>.size)
        let connected = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { raw in
                connect(fd, raw, length)
            }
        }
        guard connected == 0 else { return false }
        var caps = currentHelloCapabilities().littleEndian
        var payload = Data(bytes: &caps, count: 4)
        payload.append(Data(count: 4))
        var frame = Data("SEYALIPC".utf8)
        frame.append(uint16LE(1))
        frame.append(uint16LE(0))
        frame.append(uint16LE(1))
        frame.append(uint16LE(0))
        frame.append(uint32LE(UInt32(payload.count)))
        frame.append(uint32LE(0))
        frame.append(payload)
        let written = frame.withUnsafeBytes { buffer -> Int in
            guard let base = buffer.baseAddress else { return -1 }
            return frame.count == 0 ? 0 : Darwin.write(fd, base, frame.count)
        }
        guard written == frame.count else { return false }
        var header = [UInt8](repeating: 0, count: 24)
        guard readFull(fd, &header) else { return false }
        guard header.starts(with: Array("SEYALIPC".utf8)) else { return false }
        let messageType = UInt16(header[12]) | (UInt16(header[13]) << 8)
        let payloadLength = Int(UInt32(header[16]) | (UInt32(header[17]) << 8) | (UInt32(header[18]) << 16) | (UInt32(header[19]) << 24))
        guard messageType == 2, payloadLength == 32 else {
            SparkLog.note("hello-rejected type=\(messageType) len=\(payloadLength)")
            return false
        }
        var body = [UInt8](repeating: 0, count: payloadLength)
        guard readFull(fd, &body) else { return false }
        let runtimeID = body.prefix(16).map { String(format: "%02x", $0) }.joined()
        SparkLog.note("hello-ok runtime_id=\(runtimeID) capabilities-bytes=\(body.count)")
        return true
    }

    private func spawnRuntime() {
        let helper = Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/seyal-runtime")
        guard FileManager.default.isExecutableFile(atPath: helper.path) else {
            SparkLog.note("runtime-missing")
            return
        }
        let directory = runtimeDirectory().path
        var actions: posix_spawn_file_actions_t?
        var attributes: posix_spawnattr_t?
        guard posix_spawn_file_actions_init(&actions) == 0, posix_spawnattr_init(&attributes) == 0 else {
            SparkLog.note("runtime-spawn-init-failed")
            return
        }
        defer {
            posix_spawn_file_actions_destroy(&actions)
            posix_spawnattr_destroy(&attributes)
        }
        let nullFD = open("/dev/null", O_RDWR | O_CLOEXEC)
        guard nullFD >= 0 else { return }
        defer { close(nullFD) }
        for descriptor in [STDIN_FILENO, STDOUT_FILENO, STDERR_FILENO] {
            posix_spawn_file_actions_adddup2(&actions, nullFD, descriptor)
        }
        let flags = Int16(POSIX_SPAWN_CLOEXEC_DEFAULT | POSIX_SPAWN_SETPGROUP)
        posix_spawnattr_setflags(&attributes, flags)
        posix_spawnattr_setpgroup(&attributes, 0)
        let arguments = [helper.path, "--runtime-dir", directory]
        var argv = arguments.map { strdup($0) }
        argv.append(nil)
        defer { argv.forEach { free($0) } }
        var environment = ["PATH=/usr/bin:/bin:/usr/sbin:/sbin"]
        if let home = ProcessInfo.processInfo.environment["HOME"] { environment.append("HOME=\(home)") }
        if let user = ProcessInfo.processInfo.environment["USER"] { environment.append("USER=\(user)") }
        if let shell = ProcessInfo.processInfo.environment["SHELL"] { environment.append("SHELL=\(shell)") }
        var envp = environment.map { strdup($0) }
        envp.append(nil)
        defer { envp.forEach { free($0) } }
        var pid = pid_t()
        let status = helper.path.withCString { executable in
            argv.withUnsafeMutableBufferPointer { argvBuffer in
                envp.withUnsafeMutableBufferPointer { envBuffer in
                    posix_spawn(
                        &pid,
                        executable,
                        &actions,
                        &attributes,
                        argvBuffer.baseAddress,
                        envBuffer.baseAddress
                    )
                }
            }
        }
        guard status == 0 else {
            SparkLog.note("runtime-spawn-failed status=\(status)")
            return
        }
        runtimePID = pid
        let pidURL = spikeDirectory().appendingPathComponent("runtime.pid")
        try? "\(pid)\n".write(to: pidURL, atomically: true, encoding: .utf8)
        SparkLog.note("runtime-spawned pid=\(pid)")
    }
}

private func currentHelloCapabilities() -> UInt32 {
    // Mirrors seyal_client requested_capabilities(blocks, extended key, viewport, provisioning).
    (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 9) | (1 << 10)
}

private func uint16LE(_ value: UInt16) -> Data {
    var little = value.littleEndian
    return Data(bytes: &little, count: 2)
}

private func uint32LE(_ value: UInt32) -> Data {
    var little = value.littleEndian
    return Data(bytes: &little, count: 4)
}

private func readFull(_ fd: Int32, _ buffer: inout [UInt8]) -> Bool {
    var offset = 0
    let total = buffer.count
    while offset < total {
        let count = buffer.withUnsafeMutableBytes { raw -> Int in
            guard let base = raw.baseAddress else { return -1 }
            return Darwin.read(fd, base.advanced(by: offset), total - offset)
        }
        if count <= 0 { return false }
        offset += count
    }
    return true
}

@main
struct SpikeMain {
    static func main() {
        let app = NSApplication.shared
        let delegate = SpikeApp()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}
