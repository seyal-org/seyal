import Foundation

@MainActor
extension RustDisplayBridge {
  static func pasteAdmissionCode(_ text: String) -> Int32 {
    let byteCount = text.utf8.count
    if byteCount == 0 { return -4 }
    if byteCount > 65_536 { return -14 }
    return 0
  }

  func submitPaste(_ text: String) -> Int32 {
    let admission = Self.pasteAdmissionCode(text)
    guard admission == 0 else {
      onStatusChanged()
      return admission
    }
    guard isConnected, selectPresentedClient() else {
      onStatusChanged()
      return -10
    }
    let byteCount = text.utf8.count
    let count = UInt32(byteCount)
    let result =
      text.utf8.withContiguousStorageIfAvailable { buffer -> Int32 in
        seyal_bridge_submit_paste(buffer.baseAddress, count)
      }
      ?? Array(text.utf8).withUnsafeBufferPointer { buffer in
        seyal_bridge_submit_paste(buffer.baseAddress, count)
      }
    return finishMutation(result)
  }

  @discardableResult
  func submitHostSelection(
    action: UInt8,
    kind: UInt8 = 0,
    startCol: UInt16 = 0,
    startRow: UInt16 = 0,
    endCol: UInt16 = 0,
    endRow: UInt16 = 0
  ) -> Int32 {
    guard isConnected, selectPresentedClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(
      seyal_bridge_submit_host_selection(action, kind, startCol, startRow, endCol, endRow)
    )
  }

  @discardableResult
  func submitHostSearch(_ needle: String, forward: Bool) -> Int32 {
    guard isConnected, selectPresentedClient() else {
      onStatusChanged()
      return -10
    }
    let byteCount = needle.utf8.count
    guard byteCount <= Int(UInt32.max) else {
      onStatusChanged()
      return -14
    }
    let count = UInt32(byteCount)
    let result =
      needle.utf8.withContiguousStorageIfAvailable { buffer -> Int32 in
        seyal_bridge_submit_host_search(buffer.baseAddress, count, forward ? 1 : 0)
      }
      ?? Array(needle.utf8).withUnsafeBufferPointer { buffer in
        seyal_bridge_submit_host_search(buffer.baseAddress, count, forward ? 1 : 0)
      }
    return finishMutation(result)
  }

  func copiedText() -> String? {
    guard selectPresentedClient() else { return nil }
    let copied = seyal_bridge_copied_text()
    guard copied.len > 0, let utf8 = copied.utf8 else { return nil }
    let buffer = UnsafeBufferPointer(start: utf8, count: Int(copied.len))
    return String(decoding: buffer, as: UTF8.self)
  }

  @discardableResult
  func consumeCopiedText() -> Int32 {
    guard selectPresentedClient() else { return -1 }
    return seyal_bridge_copied_text_consume()
  }

  @discardableResult
  func submitComposerCommand(_ text: String) -> Int32 {
    guard isConnected, selectPresentedClient() else {
      onStatusChanged()
      return -10
    }
    let byteCount = text.utf8.count
    guard byteCount <= Int(UInt32.max) else {
      onStatusChanged()
      return -14
    }
    let count = UInt32(byteCount)
    let result =
      text.utf8.withContiguousStorageIfAvailable { buffer -> Int32 in
        seyal_bridge_submit_composer(buffer.baseAddress, count)
      }
      ?? Array(text.utf8).withUnsafeBufferPointer { buffer in
        seyal_bridge_submit_composer(buffer.baseAddress, count)
      }
    return finishMutation(result)
  }

  @discardableResult
  func submitKey(kind: UInt16, scalar: UInt32) -> Int32 {
    guard isConnected, selectPresentedClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(seyal_bridge_submit_key(kind, scalar))
  }

  func supportsKeyV2() -> Bool {
    guard isConnected, selectPresentedClient() else { return false }
    return seyal_bridge_supports_key_v2() != 0
  }

  @discardableResult
  func submitKeyV2(kind: UInt16, modifiers: UInt16, value: UInt32, event: UInt8, shiftedASCII: UInt32, actionID: UInt32) -> Int32 {
    guard isConnected, selectPresentedClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(seyal_bridge_submit_key_v2(kind, modifiers, value, event, shiftedASCII, actionID))
  }

  func mouseCell(
    pixelX: Double,
    pixelYFromTop: Double,
    viewportWidth: Double,
    viewportHeight: Double,
    horizontalInsets: Double,
    verticalInsets: Double,
    cellWidth: Double,
    cellHeight: Double
  ) -> (UInt16, UInt16)? {
    var col: UInt16 = 0
    var row: UInt16 = 0
    let ok = seyal_bridge_mouse_cell(
      pixelX,
      pixelYFromTop,
      viewportWidth,
      viewportHeight,
      horizontalInsets,
      verticalInsets,
      cellWidth,
      cellHeight,
      &col,
      &row
    )
    return ok != 0 ? (col, row) : nil
  }

  @discardableResult
  func submitMouse(
    kind: UInt8,
    button: UInt8,
    modifiers: UInt16,
    col: UInt16,
    row: UInt16,
    actionID: UInt32
  ) -> Int32 {
    guard isConnected, selectPresentedClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(seyal_bridge_submit_mouse(kind, button, modifiers, col, row, actionID))
  }

  @discardableResult
  func proposeGeometry(
    viewportWidth: Double,
    viewportHeight: Double,
    horizontalInsets: Double,
    verticalInsets: Double,
    cellWidth: Double,
    cellHeight: Double
  ) -> Int32 {
    guard isConnected, selectPresentedClient() else {
      onStatusChanged()
      return -10
    }
    // A rejected or not-yet-writable resize is not socket loss. `finishMutation`
    // would stop this Controller, and a layout pass on every bounds change then
    // drops the live session before Flow eligibility or a second Pane can attach.
    let result = seyal_bridge_propose_geometry(
      viewportWidth,
      viewportHeight,
      horizontalInsets,
      verticalInsets,
      cellWidth,
      cellHeight
    )
    synchronizeWriteReadinessSource()
    onStatusChanged()
    return result
  }

  @discardableResult
  func retryResize() -> Int32 {
    guard isConnected, selectPresentedClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(seyal_bridge_retry_resize())
  }

  func inputFailureCode() -> Int32 {
    guard isConnected, selectPresentedClient() else { return 4 }
    return seyal_bridge_input_failure()
  }

  func resizeFailureCode() -> Int32 {
    guard isConnected, selectPresentedClient() else { return 201 }
    return seyal_bridge_resize_failure()
  }

  func finishMutation(_ result: Int32, targetHandle: UInt64? = nil) -> Int32 {
    // Capture before `onStatusChanged()` so a focus callback cannot redirect
    // teardown to a different Pane (#1433).
    let handle = targetHandle ?? presentedClientHandle
    synchronizeWriteReadinessSource()
    onStatusChanged()
    // Mutations never return -18 (only poll_prepare quarantines) and -4 is
    // input validation; only -3/-10 mean the target's socket is gone.
    guard result == -3 || result == -10 else { return result }
    // Scope teardown to the mutation target — never window-wide `stop()` for
    // a sibling Controller's real disconnect (#1433).
    guard handle != 0, handle != clientHandle else {
      onError(result)
      stop()
      return result
    }
    // Scope teardown to the one Pane that actually failed, mirroring the
    // already-handle-scoped terminal path in armAuxiliaryReadSources(). Rust's
    // own per-Pane recovery coordinator (pane_recovery.rs) owns reconnecting
    // this handle; this window's primary `stop(reconnect:)` dial is specific
    // to `clientHandle` and must not be invoked for a sibling's failure.
    cancelAuxiliarySources(for: handle)
    seyal_bridge_disconnect_handle(handle)
    _ = selectClient()
    publishCurrentFrame()
    synchronizeWriteReadinessSource()
    armAuxiliaryReadSources()
    onStatusChanged()
    return result
  }

  func drainReadyDisplayWork() {
    guard isConnected, selectClient() else { return }
    defer {
      synchronizeWriteReadinessSource()
      onStatusChanged()
    }

    // Rust bounds each poll by frame count and bytes. A small outer bound
    // prevents one high-volume terminal from monopolizing the AppKit queue;
    // unread socket data will immediately retrigger this dispatch source.
    for _ in 0..<8 {
      // Presentation/history reads select the focused terminal below. Keep
      // readiness polling pinned to this bridge's original Controller.
      guard selectClient() else { return }
      let result = seyal_bridge_poll()
      // Shell exit (-6): Lifecycle Finalized arrives after the final display /
      // history frames in the same poll. Publish those history ranges and the
      // Block timeline while the client is still registered (unregister waits
      // for stop — #1379 clear→exit). Other losses unregister inside poll;
      // APPLY_RUNTIME_BLOCKS with no client retains the composer timeline.
      //
      // Protocol (-4) and BlockMetadataConflict (-18) close the connection in
      // Rust (`is_terminal_client_loss`) because the offending frame is never
      // consumed; leaving this source armed would spin the main thread
      // (SPEC-007 §12.1/§12.2, #1433). Other negatives stay registered and
      // retry on the next poll.
      if result < 0, !Self.pollResultClosesConnection(result) {
        return
      }
      if result < 0 {
        _ = selectPresentedClient()
        publishHistoryRanges()
        onTimeline()
        // Rebuild may have issued fresh history requests for settled Blocks;
        // one more publish picks up anything already buffered on the client.
        _ = selectPresentedClient()
        publishHistoryRanges()
        onError(result)
        stop()
        return
      }
      runtimeBlockMetadata = currentBlockMetadata()
      _ = selectPresentedClient()
      publishHistoryRanges()
      publishComposerResult()
      publishComposerStatus()
      if let text = copiedText() {
        onCopiedText?(text)
        _ = consumeCopiedText()
      }
      let revision = seyal_bridge_block_timeline_revision()
      if revision != lastTimelineRevision {
        lastTimelineRevision = revision
        onTimeline()
      }
      if result == 1 {
        publishCurrentFrame()
        // The primary read source is this bridge's own Controller. Auxiliary
        // sources skip that handle, so an unfocused Pane that still owns it
        // would otherwise keep the AX grid from host creation.
        onRegistryHandlePolled?(clientHandle)
        armAuxiliaryReadSources()
        armProvisioningWakeupSource()
        continue
      }
      // result == 0: drained.
      armAuxiliaryReadSources()
      armProvisioningWakeupSource()
      return
    }
  }

  func synchronizeWriteReadinessSource() {
    guard isConnected else {
      writeSource?.cancel()
      writeSource = nil
      for source in auxiliaryWriteSources.values {
        source.cancel()
      }
      auxiliaryWriteSources.removeAll()
      return
    }

    synchronizeWriteSource(
      handle: clientHandle,
      fileDescriptor: socketFileDescriptor,
      existing: &writeSource
    )

    var live = Set<UInt64>()
    var cursor: UInt64 = 0
    while true {
      let handle = seyal_bridge_next_handle(cursor)
      if handle == 0 { break }
      cursor = handle
      live.insert(handle)
      if handle == clientHandle { continue }
      let fd = seyal_bridge_socket_fd_for(handle)
      let wants = seyal_bridge_wants_write_for(handle)
      if wants < 0 {
        onError(wants)
        stop()
        return
      }
      if wants != 1 || fd < 0 {
        if let source = auxiliaryWriteSources.removeValue(forKey: handle) {
          source.cancel()
        }
        continue
      }
      if auxiliaryWriteSources[handle] != nil { continue }
      let source = DispatchSource.makeWriteSource(fileDescriptor: fd, queue: .main)
      source.setEventHandler { [weak self] in
        seyalRunAsMainActorFromMainQueue {
          guard let self, self.isConnected else { return }
          let result = seyal_bridge_flush_writable_for(handle)
          guard result == 0 else {
            self.onError(result)
            // #1433: match drainReadyDisplayWork/finishMutation -- a non-
            // terminal flush rejection (e.g. -18) is not registry loss and
            // must not tear anything down; re-arm and retry on the next
            // writable event instead of spinning a disconnect/reconnect.
            guard Self.isTerminalPollResult(result) else {
              self.synchronizeWriteReadinessSource()
              self.onStatusChanged()
              return
            }
            self.stop()
            return
          }
          self.synchronizeWriteReadinessSource()
          self.onStatusChanged()
        }
      }
      source.setCancelHandler { [teardown = teardown!] in
        teardown.sourceCancelled()
      }
      teardown.sourceCreated()
      auxiliaryWriteSources[handle] = source
      source.resume()
    }
    for (handle, source) in auxiliaryWriteSources where !live.contains(handle) {
      source.cancel()
      auxiliaryWriteSources.removeValue(forKey: handle)
    }
  }

  func synchronizeWriteSource(
    handle: UInt64,
    fileDescriptor: Int32,
    existing: inout DispatchSourceWrite?
  ) {
    let wantsWrite = seyal_bridge_wants_write_for(handle)
    if wantsWrite < 0 {
      onError(wantsWrite)
      stop()
      return
    }
    guard wantsWrite == 1 else {
      existing?.cancel()
      existing = nil
      return
    }
    guard existing == nil, fileDescriptor >= 0 else { return }

    let source = DispatchSource.makeWriteSource(
      fileDescriptor: fileDescriptor,
      queue: .main
    )
    source.setEventHandler { [weak self] in
      seyalRunAsMainActorFromMainQueue {
        guard let self, self.isConnected else { return }
        let result = seyal_bridge_flush_writable_for(handle)
        guard result == 0 else {
          self.onError(result)
          // #1433: see the matching comment in synchronizeWriteReadinessSource's
          // auxiliary write handler above -- a non-terminal flush rejection is
          // not registry loss.
          guard Self.isTerminalPollResult(result) else {
            self.synchronizeWriteReadinessSource()
            self.onStatusChanged()
            return
          }
          self.stop()
          return
        }
        self.synchronizeWriteReadinessSource()
        self.onStatusChanged()
      }
    }
    source.setCancelHandler { [teardown = teardown!] in
      teardown.sourceCancelled()
    }
    teardown.sourceCreated()
    existing = source
    source.resume()
  }

  func flushReadyControlWork() {
    guard isConnected else { return }
    let result = seyal_bridge_flush_writable_for(clientHandle)
    guard result == 0 else {
      onError(result)
      // #1433: a non-terminal flush rejection is not registry loss; retry via
      // re-arm instead of disconnecting a live primary Controller.
      guard Self.isTerminalPollResult(result) else {
        synchronizeWriteReadinessSource()
        onStatusChanged()
        return
      }
      stop()
      return
    }
    synchronizeWriteReadinessSource()
    onStatusChanged()
  }
}
