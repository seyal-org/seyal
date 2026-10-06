# M004 spike evidence — Sparkle update prototype (issue 688)

Isolated prototype evidence attached to Proposed ADR-022 / SPEC-029 (`Refs #688`). It is not a production implementation of #677, and it does not amend SPEC-003 or SPEC-004. Harness sources live on non-mergeable `spike/688-sparkle-proto`.

Paths, signing identities, Team IDs, and key material are redacted. Throwaway EdDSA seeds lived only in a throwaway keychain and seed files under the spike temp directory. Nothing from that keychain is in git.

## Setup

| Item | Result |
| --- | --- |
| Base | `origin/master` at `b503154d` |
| Branch | `spike/688-sparkle-proto` (no upstream; not pushed) |
| Sparkle | 2.9.6, SPM binary target. Official zip sha256 `8d5fb41d960b43f4a68aa14126bf62b098544ec8d191cdcc73eb14e63a8e7606` |
| Signing identity | **Apple Development** only. This Mac has no Developer ID identity. Every result below is Apple Development unless marked ad-hoc. |
| Feed | Throwaway loopback HTTP for Sparkle. The same directory was also served over HTTPS with a throwaway self-signed certificate. `security add-trusted-cert` hit an interactive trust prompt and was aborted (`cert-trust=interactive-or-failed`). `curl --cacert` of the HTTPS copy matched the HTTP appcast bytes. Signed-feed checks are the EdDSA signature, independent of that TLS trust. |
| Archives | Full DMG only. No delta enclosures. |
| Sparkle keys | `SURequireSignedFeed=YES`, `SUVerifyUpdateBeforeExtraction=YES`, `SUSignedFeedFailureExpirationInterval=0`, `SUEnableSystemProfiling=NO`, `SUScheduledImpatientCheckInterval=15` |
| App | Stub `SeyalSpike.app`, non-sandboxed, optional Sparkle XPC services removed, real `seyal-runtime` at `Contents/Helpers` with identifier `dev.seyal.Seyal.runtime`. Policy and runtime directory live outside the bundle. |

## G1 — Harness: pass

Sparkle 2.9.6 is linked from the official SPM artifact. The stub is non-sandboxed (`com.apple.security.app-sandbox` absent). Nested Mach-O images, all signed inside-out with the Apple Development identity and hardened runtime (`flags=0x10000(runtime)`), `codesign --verify --deep --strict` valid:

- `Contents/MacOS/SpikeHarness` (arm64)
- `Sparkle.framework` universal slice (`macos-arm64_x86_64`)
- `Autoupdate` (universal)
- `Updater.app` (universal)
- `Contents/Helpers/seyal-runtime` (arm64)

`XPC_PRESENT` in the inventory is empty: no `Downloader.xpc` or `Installer.xpc`. Versions exercised: `CFBundleVersion` 100, then 101 (and 102 for the rotation hop).

## G2 — Resident Runtime survival: pass

v100 spawned `seyal-runtime` in its own process group (`POSIX_SPAWN_SETPGROUP`), with a live login shell, `vim`, and a tight high-output bash loop. Sparkle then installed v101 and relaunched the stub.

| Check | Result |
| --- | --- |
| Same Runtime PID after the swap | Yes. PID stayed alive and was reparented to launchd. |
| Executions | Probe prepared 3 executions before the swap and reattached all 3 after it, including a later reattach at 931 seconds. Same runtime id. Server hello capabilities `0x6ff`. |
| Old bundle | The mapped helper has `nlink=0`. Its path is under Sparkle's Installation cache and is no longer on disk. The installed v101 helper is a different inode. The process `argv` still shows the original helper path from spawn. |
| CODESIGNING kill | Unified log for this PID across the soak has no code-signing invalidation or kill. The process was still alive at 931 seconds (~15.5 minutes) with 3 children. |
| New GUI attach | v101 did not spawn a runtime. It completed the current master hello (8-byte capability payload, ServerHello type 2, 32 bytes) and logged attach to the existing runtime. |

The 2 hour target was not reached. The session is time-boxed, and the runtime was left running rather than held idle for the full interval. At the evidence sample it was healthy.

Current master hello, recorded as evidence only: the client sends capabilities `CAP_COMMAND_BLOCKS | CAP_BLOCK_METADATA | CAP_GRAPHEME_DISPLAY | CAP_EXTENDED_TERMINAL_KEY | CAP_VIEWPORT_LINE_IDS | CAP_EXECUTION_PROVISIONING`. No SPEC-003 §16 or SPEC-004 change was made.

The update replaced the app bundle. The runtime directory and its socket sit outside the bundle and kept the same runtime id and executions. This spike does not claim a live PTY handoff protocol.

## G3 — Host-gated install: pass, with a required reply

Custom `SPUUserDriver` plus `SPUUpdaterDelegate`. Every logged driver callback reported `windows=none`. No Sparkle standard update window appeared.

| Path | Host reply | After explicit quit |
| --- | --- | --- |
| Manual, before download | `Dismiss` at stage not-downloaded | Stayed version 100 |
| Ready to install | `Dismiss` | Became version 101 |
| Ready to install, foreign identity | `Skip` (`enforceTeamContinuity`) | Stayed version 100, including after the quit handler ran |
| Install-on-quit callback | Not invoked when the cycle had already finished via Dismiss or Skip | The staged Dismiss case still installed on quit |

`willInstallUpdateOnQuit` is not a cancel point once the driver has finished the cycle. Sparkle still installs a staged update on termination after `Dismiss`. The host holds that path by replying `Skip`, or by refusing the download before extraction. The impatient interval (15s) produced no second driver callback and no window during a 25s wait after Dismiss.

So the custom driver can hold every observed install and relaunch path. The holding replies are refuse-before-download and `Skip`. `Dismiss` after extraction does not hold quit-install.

## G4 — Signed compatibility metadata: pass

A custom `seyal:compatibility` element in the `seyal` namespace was present on the appcast item in `didFinishLoadingAppcast` and `shouldProceedWithUpdate`, both before `willDownloadUpdate`. With `blockProceed`, the host saw the element and no download started.

`sign_update --verify` accepted the intact feed. Changing `min-hello=1` to `min-hello=999` (896 signed bytes vs 898 read) made `sign_update --verify` fail. The app then aborted with `SUSparkleErrorDomain` code 1000 ("feed is improperly signed") and did not download.

## G5 — Trust semantics: 1-of-2

Identity class for the host and for the rotation builds: **Apple Development**. The different-identity build is **ad-hoc** (`Signature=adhoc`, Team ID not set). A second Apple Team ID was not available.

| Experiment | Result |
| --- | --- |
| Valid EdDSA, ad-hoc archive, team gate off | Installed. Version became 101. Sparkle accepted a different code-signing identity. |
| Same archive, host `enforceTeamContinuity` | `showReady` saw a foreign team, replied `Skip`, version stayed 100. |
| Rotation while the old key is still held | v101 archive signed by key A, app embeds key B: installed. v102 archive signed only by key B: installed. |
| Lost-key fallback | Appcast XML signed by key A (download started, feed signature valid). Archive signed by key B. `SUVerifyUpdateBeforeExtraction` rejected it with `SUSparkleErrorDomain` code 4005. Version stayed 100. |

Finding: **1-of-2**. When the EdDSA signature validates, Sparkle does not also require a matching Team ID. Team continuity is a host check in `showReady`, before install. The Developer ID + same-team lost-key fallback was not available on this machine and the Apple Development attempt was rejected.

## Not run

G6–G9 were not executed. G8 notarization is blocked here: no Developer ID identity and no notarization credentials. Interactive trust of the throwaway HTTPS root also failed, so Sparkle's own checks used loopback HTTP plus the signed feed.

Still open for later #677 / the architecture record: production signing and notarization, a real HTTPS feed trust story, interrupted and corrupt installs, downgrade/rollback, size and scheduling cost, and the Rust-owned product UX on the replies this spike measured (`Skip` or refuse-before-download). No production crate or `macos/` product path was edited. No mergeable pull request was opened.
