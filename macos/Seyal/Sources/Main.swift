import AppKit
import Darwin

@main
enum SeyalMain {
    static func main() {
        IsolatedRuntimeDirectory.installIfNeeded()
        if CommandLine.arguments.contains("--renderer-benchmark") {
            let passed = RendererValidation.runBenchmark()
            Darwin.exit(passed ? 0 : 1)
        }
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let delegate = AppDelegate()
        app.delegate = delegate
        // NSApplication.delegate is weak. Keep the delegate (and its window
        // host) alive for the full event loop, including while the app has no
        // windows and is waiting for Dock or New Window re-entry.
        withExtendedLifetime(delegate) {
            app.run()
        }
    }
}
