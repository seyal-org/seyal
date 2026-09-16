# Pass 9 release packaging inspection

- **Commit:** `88e274bd36aae78ee6460758fa602692fe78dc38`
- **Helper path:** `Seyal.app/Contents/Helpers/seyal-runtime`
- **Direct no-shell launch:** exercised by this orchestrator
- **Redaction:** local workspace path is intentionally redacted; packaging evidence is preserved.

## codesign -dv --verbose=4 (helper)
```
Executable=<workspace>/target/macos-derived-data/Build/Products/Debug/Seyal.app/Contents/Helpers/seyal-runtime
Identifier=dev.seyal.Seyal.runtime
Format=Mach-O thin (arm64)
CodeDirectory v=20400 size=5552 flags=0x2(adhoc) hashes=168+2 location=embedded
VersionPlatform=1
VersionMin=720896
VersionSDK=1705216
Hash type=sha256 size=32
CandidateCDHash sha256=eb24b29e0e76147d3c8c01a63718758080d519a7
CandidateCDHashFull sha256=eb24b29e0e76147d3c8c01a63718758080d519a7508a507c352011772594d6f3
Hash choices=sha256
CMSDigest=eb24b29e0e76147d3c8c01a63718758080d519a7508a507c352011772594d6f3
CMSDigestType=2
Executable Segment base=0
Executable Segment limit=1196032
Executable Segment flags=0x1
Page size=16384
CDHash=eb24b29e0e76147d3c8c01a63718758080d519a7
Signature=adhoc
Info.plist=not bound
TeamIdentifier=not set
Sealed Resources=none
Internal requirements count=0 size=12
```

## codesign --display --entitlements - (helper)
```
Executable=<workspace>/target/macos-derived-data/Build/Products/Debug/Seyal.app/Contents/Helpers/seyal-runtime
warning: Specifying ':' in the path is deprecated and will not work in a future release
```

## codesign --verify --strict --deep (app)
```
```
