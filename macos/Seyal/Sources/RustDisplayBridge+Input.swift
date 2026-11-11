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

  func finishMutation(_ result: Int32) -> Int32 {
    synchronizeWriteReadinessSource()
    onStatusChanged()
    if result == -18 {
      // A mutation conflict is retryable. Polling the unconsumed frame owns
      // connection teardown; never stop the whole window from a sibling write.
      onError(result)
    } else if Self.isTerminalPollResult(result) {
      onError(result)
      stop()
    }
    return result
  }

  /// Finish work addressed to one Controller without treating a sibling's
  /// error as a failure of this bridge's primary connection. Multipane hosts
  /// share the bridge, while readiness and history work are handle-addressed.
  /// The target Controller's own read source owns loss detection and recovery;
  /// only errors for `clientHandle` may stop this bridge (#1426).
  @discardableResult
  func finishAddressedMutation(_ result: Int32, forDisplayHandle handle: UInt64) -> Int32 {
    guard result != 0 else {
      // A successful mutation can still leave control frames queued after a
      // nonblocking write. Arm this Controller's auxiliary write source now;
      // otherwise a history request for a quiet sibling can wait forever for
      // a read event that cannot arrive until the request is flushed.
      synchronizeWriteReadinessSource()
      return result
    }
    guard handle == clientHandle else {
      guard Self.isTerminalPollResult(result) else {
        onStatusChanged()
        return result
      }
      if let source = auxiliaryWriteSources.removeValue(forKey: handle) {
        // Stop a level-triggered writable error loop, including a handle
        // removed between readiness and flush (reported as -1). The same
        // Controller's read source owns loss detection; this primary Pane's
        // error callback must not receive a sibling's failure.
        source.cancel()
      }
      onStatusChanged()
      return result
    }
    return finishMutation(result)
  }

  func drainReadyDisplayWork() {
    guard isConnected else { return }
    guard selectClient() else {
      _ = handlePrimaryRegistryRemovalIfNeeded()
      return
    }
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
      let polledHandle = clientHandle
      guard selectClient() else {
        _ = handlePrimaryRegistryRemovalIfNeeded()
        return
      }
      let result = seyal_bridge_poll()
      if result < 0, !Self.pollResultClosesConnection(result) {
        onError(result)
        return
      }
      // Shell exit (-6): Lifecycle Finalized arrives after the final display /
      // history frames in the same poll. Publish those history ranges and the
      // Block timeline while the client is still registered (unregister waits
      // for stop — #1379 clear→exit). Other losses unregister inside poll;
      // APPLY_RUNTIME_BLOCKS with no client retains the composer timeline.
      if result < 0 {
        // This readiness source polled clientHandle even when another pane was
        // focused. Drain its completed history before a shell-exit disconnect;
        // focused-display lookup would strand or misroute the response (#1426).
        publishHistoryRanges(forDisplayHandle: polledHandle)
        _ = selectPresentedClient()
        onTimeline()
        // Rebuild may have issued fresh history requests for settled Blocks;
        // one more addressed publish picks up buffered responses on the polled
        // Controller without reading a sibling's same-numbered Block.
        publishHistoryRanges(forDisplayHandle: polledHandle)
        onError(result)
        stop(reconnect: result == -18)
        return
      }
      runtimeBlockMetadata = currentBlockMetadata()
      // Publish this poll source before selecting the focused presentation
      // client. Pane-local Block ids are only meaningful on their Controller.
      publishHistoryRanges(forDisplayHandle: polledHandle)
      _ = selectPresentedClient()
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
    if !isConnected || clientHandle == 0 {
      writeSource?.cancel()
      writeSource = nil
    } else {
      synchronizeWriteSource(
        handle: clientHandle,
        fileDescriptor: socketFileDescriptor,
        existing: &writeSource
      )
    }

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
        finishAddressedMutation(wants, forDisplayHandle: handle)
        continue
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
          guard let self, self.auxiliaryWriteSources[handle] != nil else { return }
          let result = seyal_bridge_flush_writable_for(handle)
          guard result == 0 else {
            self.finishAddressedMutation(result, forDisplayHandle: handle)
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
          self.finishMutation(result)
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
      stop(reconnect: result == -18)
      return
    }
    synchronizeWriteReadinessSource()
    onStatusChanged()
  }
}
