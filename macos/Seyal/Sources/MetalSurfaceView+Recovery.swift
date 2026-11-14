import AppKit
import Foundation

extension MetalSurfaceView {
  func addWindowLifecycleObservers(for window: NSWindow) {
    NotificationCenter.default.addObserver(
      self,
      selector: #selector(windowActivationNotificationReceived),
      name: NSApplication.didBecomeActiveNotification,
      object: NSApp
    )
    NotificationCenter.default.addObserver(
      self,
      selector: #selector(windowOcclusionChanged),
      name: NSWindow.didChangeOcclusionStateNotification,
      object: window
    )
    NotificationCenter.default.addObserver(
      self,
      selector: #selector(windowActivationNotificationReceived),
      name: NSWindow.didBecomeKeyNotification,
      object: window
    )
    NotificationCenter.default.addObserver(
      self,
      selector: #selector(windowActivationNotificationReceived),
      name: NSWindow.didBecomeMainNotification,
      object: window
    )
  }

  func removeWindowLifecycleObservers(from window: NSWindow) {
    NotificationCenter.default.removeObserver(
      self,
      name: NSApplication.didBecomeActiveNotification,
      object: NSApp
    )
    NotificationCenter.default.removeObserver(
      self,
      name: NSWindow.didChangeOcclusionStateNotification,
      object: window
    )
    NotificationCenter.default.removeObserver(
      self,
      name: NSWindow.didBecomeKeyNotification,
      object: window
    )
    NotificationCenter.default.removeObserver(
      self,
      name: NSWindow.didBecomeMainNotification,
      object: window
    )
  }

  /// Recovery remains pinned to the accepted execution after its transport
  /// disconnects. An implicit first attach is only a bootstrap; its accepted
  /// identity must not be forgotten and replaced with whichever execution is
  /// running later. Shell exit does not clear this pin: New shell is
  /// SPEC-004 §18 Create+Bind, not `open_first`.
  var recoveryExecutionIdentity: String? {
    if let requestedExecutionIdentity { return requestedExecutionIdentity }
    guard let bridge else { return nil }
    return Self.identityString((
      low: bridge.lastRecoveryResult.executionIDLow,
      high: bridge.lastRecoveryResult.executionIDHigh
    ))
  }

  /// Snapshot recovery stage (`SeyalAppRecoveryStage`). Zero handle → disconnected.
  var runtimeRecoveryStage: UInt16 {
    guard recoveryAppHandle != 0 else {
      return UInt16(SEYAL_APP_RECOVERY_DISCONNECTED.rawValue)
    }
    return seyal_app_snapshot(recoveryAppHandle).recovery_stage
  }

  var isRustRecoveryEpisodeActive: Bool {
    guard recoveryAppHandle != 0 else { return false }
    let snap = seyal_app_snapshot(recoveryAppHandle)
    if snap.recovery_effect != 0 { return true }
    return runtimeRecoveryStageIsEpisodeActive(snap.recovery_stage)
  }

  func cancelBridgeReconnect() {
    guard recoveryAppHandle != 0 else { return }
    recoveryPresentationPending = false
    (self as? InteractiveMetalSurfaceView)?.resetComposerRecoveryFocusRetries()
    _ = applyRuntimeRecoveryAction(recoveryAppHandle, kind: SEYAL_APP_ACTION_CANCEL_RECOVERY)
    onRecoveryEffectsPending?()
  }

  func startAutomaticBridgeRecoveryIfNeeded() {
    guard !suppressesAutomaticBridgeRecovery,
      !isDetachingRuntimeConnection,
      shouldAttachRuntime,
      bridge?.isConnected == false,
      // stop() keeps the old clientHandle until both dispatch-source cancel
      // handlers complete. Waiting for zero prevents consuming a retry on our
      // own in-progress detach/controller cleanup.
      bridge?.clientHandle == 0,
      recoveryAppHandle != 0,
      !isRustRecoveryEpisodeActive,
      runtimeRecoveryStage != UInt16(SEYAL_APP_RECOVERY_EXHAUSTED.rawValue),
      runtimeRecoveryStage != UInt16(SEYAL_APP_RECOVERY_BLOCKED.rawValue),
      runtimeRecoveryStage != UInt16(SEYAL_APP_RECOVERY_EXECUTION_ENDED.rawValue)
    else { return }
    beginRustRecoveryEpisode()
  }

  /// Explicit user retry starts a new bounded foreground recovery episode.
  /// Automatic exhaustion never invokes this method recursively.
  ///
  /// Shell exit stays on New shell (Create+Bind). Exhausted or blocked
  /// startup may begin another episode. Automatic reconnect stays suppressed
  /// while the stage is ExecutionEnded.
  @discardableResult
  func retryRuntimeConnection() -> Bool {
    guard shouldAttachRuntime,
      bridge?.isConnected != true,
      recoveryAppHandle != 0
    else { return bridge?.isConnected == true }
    // New shell after exit / blocked bootstrap: finish teardown, then open.
    if bridge?.clientHandle != 0 || isDetachingRuntimeConnection {
      bridge?.completeEagerClientDisconnect()
      isDetachingRuntimeConnection = false
    }
    if runtimeRecoveryStage == UInt16(SEYAL_APP_RECOVERY_EXECUTION_ENDED.rawValue) {
      // New shell owns Create+Bind. Reopening the dead id is not a retry.
      return false
    }
    beginRustRecoveryEpisode()
    return bridge?.isConnected == true
  }

  /// A command entered while disconnected is an explicit recovery action, but
  /// it must never synchronously connect/handshake/attach on the AppKit thread.
  @discardableResult
  func ensureTerminalBridgeConnected() -> Bool {
    guard bridge?.isConnected != true else { return true }
    guard !isDetachingRuntimeConnection,
      shouldAttachRuntime,
      bridge?.clientHandle == 0,
      recoveryAppHandle != 0
    else { return false }
    if !isRustRecoveryEpisodeActive,
      runtimeRecoveryStage != UInt16(SEYAL_APP_RECOVERY_BLOCKED.rawValue),
      runtimeRecoveryStage != UInt16(SEYAL_APP_RECOVERY_EXECUTION_ENDED.rawValue)
    {
      beginRustRecoveryEpisode()
    }
    return false
  }

  /// Sole BeginRecovery requester for this surface. ProductChrome drains effects
  /// via `onRecoveryEffectsPending` → `driveRecovery` (open+adopt).
  func beginRustRecoveryEpisode() {
    guard recoveryAppHandle != 0 else { return }
    let now = runtimeRecoveryNowMillis()
    _ = applyRuntimeRecoveryAction(recoveryAppHandle, kind: SEYAL_APP_ACTION_BEGIN_RECOVERY) {
      $0.target_generation = now
    }
    onRecoveryEffectsPending?()
  }

  @discardableResult
  func adoptRecoveredHandle(_ opened: RuntimeRecoveryOpenedHandle) -> Bool {
    bridge?.continuityAppHandle = recoveryAppHandle
    let adopted = bridge?.adoptRecoveredHandle(opened) ?? false
    if adopted {
      recoveryPresentationPending = true
      (self as? InteractiveMetalSurfaceView)?.resetComposerRecoveryFocusRetries()
    }
    return adopted
  }

  /// SPEC-009 §10: Restoring then Usable after connect + native restore.
  /// Steady-state frames (pending == false) return immediately with zero
  /// `seyal_app_snapshot` FFI; only an adopted-but-not-yet-Usable surface
  /// continues into presentation advancement.
  @discardableResult
  func advanceRecoveryPresentationIfReady() -> Bool {
    guard recoveryPresentationPending else { return true }
    (self as? InteractiveMetalSurfaceView)?.consumeComposerFocusEligibilityChangeBeforeRecovery()
    guard recoveryAppHandle != 0,
      bridge?.isConnected == true,
      hasPreparedState
    else { return true }

    // One snapshot for this pending turn; never re-enter via the stage property.
    let stage = seyal_app_snapshot(recoveryAppHandle).recovery_stage

    if stage == UInt16(SEYAL_APP_RECOVERY_RECONSTRUCTING.rawValue) {
      _ = applyRuntimeRecoveryAction(
        recoveryAppHandle,
        kind: SEYAL_APP_ACTION_ADVANCE_RECOVERY_STAGE
      ) {
        $0.reserved = UInt32(SEYAL_APP_RECOVERY_RESTORING.rawValue)
      }
    }

    let afterRestore = seyal_app_snapshot(recoveryAppHandle).recovery_stage
    guard shouldRender else {
      if afterRestore == UInt16(SEYAL_APP_RECOVERY_USABLE.rawValue) {
        recoveryPresentationPending = false
        (self as? InteractiveMetalSurfaceView)?.resetComposerRecoveryFocusRetries()
      }
      return true
    }

    guard restoreNativeInteractionAfterRendererReady() else {
      return false
    }
    _ = applyRuntimeRecoveryAction(
      recoveryAppHandle,
      kind: SEYAL_APP_ACTION_ADVANCE_RECOVERY_STAGE
    ) {
      $0.reserved = UInt32(SEYAL_APP_RECOVERY_USABLE.rawValue)
    }
    recoveryPresentationPending = false
    (self as? InteractiveMetalSurfaceView)?.resetComposerRecoveryFocusRetries()
    refreshRecoveryAccessibilityValue()
    onRecoveryEffectsPending?()
    return true
  }

  /// Makes the authoritative recovery state observable to VoiceOver and to
  /// native acceptance automation without introducing a second terminal
  /// model or changing the Runtime protocol.
  ///
  /// Prepared-frame rows/columns/cursor are mirrored read-only so headed
  /// Flow-scroll acceptance (#865 / SPEC-008 §3.1) can prove transcript
  /// scrolling does not mutate PTY size or terminal cursor state.
  ///
  /// The GPU sample for `history-gpu-ink` is expensive — only call this from
  /// the lazy AX-read path, never per Candidate-D frame (#1484 P0).
  func composeRecoveryAccessibilityValue() -> String {
    let connection = terminalBridgeIsConnected ? "usable" : "disconnected"
    let runtime = terminalRuntimeIdentity ?? "none"
    let execution = terminalExecutionIdentity ?? "none"
    let attachment = terminalAttachmentIdentity ?? "none"
    let alternate = lastAlternateScreen == true ? "true" : "false"
    let paint = renderer.inspectFlowPaint()
    let flowPaint = paint.accessibilityToken
    let historyInstances = paint.historyInstanceCount
    let historyRegionsPainted = paint.paintedHistoryRegionCount
    let historyClips = paint.historyClipsDistinct ? "ok" : "collapse"
    let atlasReady = renderer.glyphAtlas.texture != nil
    let gpuPaint = paint.historyInstanceCount > 0
      ? inspectFlowPaint(sampleGPU: true)
      : paint
    let historyGpuInk =
      gpuPaint.opaquePixelsInsideClips + gpuPaint.opaquePixelsOutsideClips
    let geometry: String
    if let frame = terminalCurrentFrame(), frame.rows > 0, frame.columns > 0 {
      geometry =
        "rows=\(frame.rows) columns=\(frame.columns) "
        + "cursor=\(frame.cursor_row),\(frame.cursor_column) "
        + "cursor-visible=\(frame.cursor_visible != 0 ? "true" : "false")"
    } else {
      geometry = "rows=0 columns=0 cursor=0,0 cursor-visible=false"
    }
    return "process=\(ProcessInfo.processInfo.processIdentifier) connection=\(connection) "
      + "runtime=\(runtime) execution=\(execution) "
      + "attachment=\(attachment) alternate-screen=\(alternate) "
      + "\(geometry) flow-paint=\(flowPaint) history-instances=\(historyInstances) "
      + "history-regions-painted=\(historyRegionsPainted) history-clips=\(historyClips) "
      + "history-atlas=\(atlasReady ? "ok" : "nil") history-gpu-ink=\(historyGpuInk)"
  }
}
