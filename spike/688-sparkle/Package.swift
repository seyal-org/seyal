// swift-tools-version: 6.0
import PackageDescription

// Isolated spike. The binary is the official Sparkle 2.9.6 SPM artifact
// (Sparkle-for-Swift-Package-Manager.zip, sha256
// 8d5fb41d960b43f4a68aa14126bf62b098544ec8d191cdcc73eb14e63a8e7606).
// Upstream Package.swift is only a pointer at that zip. The zip is linked
// here as an SPM binary target because a fresh HTTPS fetch of the same URL
// stalled in this session after the checksum had already been verified.
let package = Package(
    name: "SpikeHarness",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "SpikeHarness",
            dependencies: ["Sparkle"],
            swiftSettings: [
                .swiftLanguageMode(.v5)
            ]
        ),
        .binaryTarget(
            name: "Sparkle",
            path: "Sparkle.xcframework"
        )
    ]
)
