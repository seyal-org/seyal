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
    guard isConnected, selectClient() else {
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
    guard isConnected, selectClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(
      seyal_bridge_submit_host_selection(action, kind, startCol, startRow, endCol, endRow)
    )
  }

  @discardableResult
  func submitHostSearch(_ needle: String, forward: Bool) -> Int32 {
    guard isConnected, selectClient() else {
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
    guard selectClient() else { return nil }
    let copied = seyal_bridge_copied_text()
    guard copied.len > 0, let utf8 = copied.utf8 else { return nil }
    let buffer = UnsafeBufferPointer(start: utf8, count: Int(copied.len))
    return String(decoding: buffer, as: UTF8.self)
  }

  @discardableResult
  func consumeCopiedText() -> Int32 {
    guard selectClient() else { return -1 }
    return seyal_bridge_copied_text_consume()
  }

  @discardableResult
  func submitComposerCommand(_ text: String) -> Int32 {
    guard isConnected, selectClient() else {
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
    guard isConnected, selectClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(seyal_bridge_submit_key(kind, scalar))
  }

  func supportsKeyV2() -> Bool {
    guard isConnected, selectClient() else { return false }
    return seyal_bridge_supports_key_v2() != 0
  }

  @discardableResult
  func submitKeyV2(kind: UInt16, modifiers: UInt16, value: UInt32, event: UInt8, shiftedASCII: UInt32, actionID: UInt32) -> Int32 {
    guard isConnected, selectClient() else {
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
    guard isConnected, selectClient() else {
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
    cellHeight: Double,
    meaningfulLayoutEpoch: Bool
  ) -> Int32 {
    guard isConnected, selectClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(
      seyal_bridge_propose_geometry(
        viewportWidth,
        viewportHeight,
        horizontalInsets,
        verticalInsets,
        cellWidth,
        cellHeight,
        meaningfulLayoutEpoch ? 1 : 0
      )
    )
  }

  @discardableResult
  func retryResize() -> Int32 {
    guard isConnected, selectClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(seyal_bridge_retry_resize())
  }

  func inputFailureCode() -> Int32 {
    guard isConnected, selectClient() else { return 4 }
    return seyal_bridge_input_failure()
  }

  func resizeFailureCode() -> Int32 {
    guard isConnected, selectClient() else { return 201 }
    return seyal_bridge_resize_failure()
  }

  func finishMutation(_ result: Int32) -> Int32 {
    synchronizeWriteReadinessSource()
    onStatusChanged()
    if result == -18 {
      onError(result)
      stop(reconnect: true)
    } else if result == -3 || result == -10 {
      onError(result)
      stop()
    }
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
      let result = seyal_bridge_poll()
      runtimeBlockMetadata = currentBlockMetadata()
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
        continue
      }
      if result == 0 {
        return
      }

      onError(result)
      stop(reconnect: result == -18)
      return
    }
  }

  func synchronizeWriteReadinessSource() {
    guard isConnected, selectClient() else {
      writeSource?.cancel()
      writeSource = nil
      return
    }

    let wantsWrite = seyal_bridge_wants_write()
    if wantsWrite < 0 {
      onError(wantsWrite)
      stop()
      return
    }
    guard wantsWrite == 1 else {
      writeSource?.cancel()
      writeSource = nil
      return
    }
    guard writeSource == nil, socketFileDescriptor >= 0 else { return }

    let source = DispatchSource.makeWriteSource(
      fileDescriptor: socketFileDescriptor,
      queue: .main
    )
    source.setEventHandler { [weak self] in
      seyalRunAsMainActorFromMainQueue {
        self?.flushReadyControlWork()
      }
    }
    source.setCancelHandler { [teardown = teardown!] in
      teardown.sourceCancelled()
    }
    teardown.sourceCreated()
    writeSource = source
    source.resume()
  }

  func flushReadyControlWork() {
    guard isConnected, selectClient() else { return }
    let result = seyal_bridge_flush_writable()
    guard result == 0 else {
      onError(result)
      stop(reconnect: result == -18)
      return
    }
    synchronizeWriteReadinessSource()
    onStatusChanged()
  }
}
