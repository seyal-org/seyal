# Pass 9 release packaging inspection

- **Commit:** `5f8108ac6ea1464e5645a00770b163aa524ee6b2`
- **Helper path:** `Seyal.app/Contents/Helpers/seyal-runtime`
- **Direct no-shell launch:** exercised by this orchestrator
- **Redaction:** local workspace path and signing identity values are intentionally redacted; cryptographic/result evidence is preserved.

## codesign -dv --verbose=4 (helper)
```
Executable=<workspace>/target/macos-derived-data/Build/Products/Release/Seyal.app/Contents/Helpers/seyal-runtime
Identifier=dev.seyal.Seyal.runtime
Format=Mach-O thin (arm64)
CodeDirectory v=20500 size=1891 flags=0x10000(runtime) hashes=53+2 location=embedded
VersionPlatform=1
VersionMin=720896
VersionSDK=1705216
Hash type=sha256 size=32
CandidateCDHash sha256=8027fa0937fe8f3d16d48a7494e84f6b56a113e1
CandidateCDHashFull sha256=8027fa0937fe8f3d16d48a7494e84f6b56a113e1d49a552616e34da04c3e1817
Hash choices=sha256
CMSDigest=8027fa0937fe8f3d16d48a7494e84f6b56a113e1d49a552616e34da04c3e1817
CMSDigestType=2
Executable Segment base=0
Executable Segment limit=589824
Executable Segment flags=0x1
Page size=16384
CDHash=8027fa0937fe8f3d16d48a7494e84f6b56a113e1
Signature size=9113
Authority=Apple Development: <redacted>
Authority=Apple Worldwide Developer Relations Certification Authority
Authority=Apple Root CA
Timestamp=4 Sep 2026 at 2:06:10 PM
Info.plist=not bound
TeamIdentifier=<redacted>
Runtime Version=26.5.0
Sealed Resources=none
Internal requirements count=1 size=200
```

## Team identity gate
- **TeamIdentifier:** `<redacted>`

## codesign --display --entitlements - (helper)
```
Executable=<workspace>/target/macos-derived-data/Build/Products/Release/Seyal.app/Contents/Helpers/seyal-runtime
```

## codesign --verify --strict --deep (app)
```
```
