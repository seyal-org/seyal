import Foundation

@MainActor
extension RustDisplayBridge {
  func requestHistoryRange(startLine: UInt64, endLine: UInt64, blockID: UInt64) -> Int32 {
    let handle = windowFocusedDisplayHandle != 0 ? windowFocusedDisplayHandle : clientHandle
    guard isConnected, handle != 0 else { return -4 }
    return requestHistoryRange(
      forDisplayHandle: handle,
      startLine: startLine,
      endLine: endLine,
      blockID: blockID
    )
  }

  /// Request completed-Block history for a specific registry Controller.
  /// Multipane Flow hosts have no per-leaf bridge; they borrow this one (#1424/#1426).
  func requestHistoryRange(
    forDisplayHandle handle: UInt64,
    startLine: UInt64,
    endLine: UInt64,
    blockID: UInt64
  ) -> Int32 {
    guard isConnected, handle != 0 else { return -4 }
    guard blockID > 0, startLine > 0, endLine >= startLine else { return -4 }
    // Address the named Controller directly. `seyal_bridge_select` only moves
    // poll/flush; focused history FFI would hit FOCUSED_DISPLAY_HANDLE (#1426).
    let requestID = seyal_bridge_next_history_request_id_for(handle)
    guard requestID != 0 else { return -4 }
    let result = finishAddressedMutation(
      seyal_bridge_request_history_range_for(
        handle, blockID, startLine, endLine, 512, 131_072, 0),
      forDisplayHandle: handle
    )
    if result == 0 {
      let requestKey = PaneHistoryRequestKey(
        displayHandle: handle, paneID: paneID, requestID: requestID)
      requestedHistoryRanges[requestKey] = (blockID, startLine, endLine)
    }
    return result
  }

  /// Discard this bridge's own focused-Controller stale requests.
  func discardHistoryRequests(except blockIDs: Set<UInt64>) {
    let handle = windowFocusedDisplayHandle != 0 ? windowFocusedDisplayHandle : clientHandle
    discardHistoryRequests(except: blockIDs, forDisplayHandle: handle)
  }

  /// Discard stale pending history requests for one registry Controller only.
  /// `requestedHistoryRanges` is shared across every Flow host borrowing this
  /// bridge (#1424/#1426); filtering by Block id alone — without scoping to
  /// `handle` — evicted other panes' in-flight requests whose Block ids are
  /// simply absent from the caller's own retained set, permanently blanking
  /// their Block bodies (#1426 residual).
  func discardHistoryRequests(except blockIDs: Set<UInt64>, forDisplayHandle handle: UInt64) {
    requestedHistoryRanges = requestedHistoryRanges.filter { key, value in
      key.displayHandle != handle || blockIDs.contains(value.blockID)
    }
    historyRevisions = historyRevisions.filter { requestKey, _ in
      requestedHistoryRanges[requestKey] != nil
    }
    historyContinuations = historyContinuations.filter { requestKey, _ in
      requestedHistoryRanges[requestKey] != nil
    }
  }

  /// Retire only correlations owned by a Controller whose registry entry was
  /// lost. Other visible Pane histories keep their in-flight requests.
  func discardHistoryRequests(forDisplayHandle handle: UInt64) {
    requestedHistoryRanges = requestedHistoryRanges.filter { $0.key.displayHandle != handle }
    historyRevisions = historyRevisions.filter { $0.key.displayHandle != handle }
    historyContinuations = historyContinuations.filter { $0.key.displayHandle != handle }
  }

  /// Deliver a completed Rust-owned Block pasteboard string, if any (#1010 / #1474).
  func publishBlockCopy() {
    guard isConnected else { return }
    let copy = seyal_bridge_take_block_copy()
    guard copy.len > 0, let bytes = copy.utf8 else { return }
    let text = String(
      decoding: UnsafeBufferPointer(start: bytes, count: Int(copy.len)),
      as: UTF8.self
    )
    onHistoryCopy?(copy.block_id, text)
    onCopiedText?(text)
  }

  /// Publish buffered history for the focused/presented Controller.
  func publishHistoryRanges() {
    let handle = windowFocusedDisplayHandle != 0 ? windowFocusedDisplayHandle : clientHandle
    guard handle != 0 else { return }
    publishHistoryRanges(forDisplayHandle: handle)
  }

  /// Publish buffered history for one registry Controller (#1426).
  func publishHistoryRanges(forDisplayHandle handle: UInt64) {
    guard isConnected, handle != 0 else { return }
    let pending = requestedHistoryRanges.filter { $0.key.displayHandle == handle }
    for (requestKey, request) in Array(pending) {
      let metadata = seyal_bridge_history_range_peek_for_handle(
        handle, request.blockID, requestKey.requestID)
      guard metadata.block_id != 0,
        metadata.request_id != 0,
        metadata.block_id == request.blockID,
        metadata.request_id == requestKey.requestID,
        metadata.revision > 0,
        historyRevisions[requestKey]?.revision != metadata.revision
          || historyRevisions[requestKey]?.requestID != metadata.request_id
      else { continue }
      let sidecar = seyal_bridge_history_range_sidecar_for_handle(
        handle, metadata.block_id, metadata.request_id)
      let sidecarData: Data
      if sidecar.len > 0, let bytes = sidecar.bytes {
        sidecarData = Data(bytes: bytes, count: Int(sidecar.len))
      } else {
        sidecarData = Data()
      }
      let rows = (0..<Int(metadata.row_count)).compactMap { index -> [NativeHistoryRange.Cell]? in
        let row = seyal_bridge_history_range_row_for_handle(
          handle,
          metadata.block_id,
          metadata.request_id,
          UInt32(index)
        )
        guard row.line_id != 0, let cells = row.cells else { return nil }
        return Array(UnsafeBufferPointer(start: cells, count: Int(row.cell_count))).map {
          NativeHistoryRange.Cell(
            scalar: $0.scalar,
            foreground: $0.foreground,
            background: $0.background,
            flags: $0.flags,
            graphemeUtf8: historyGraphemeUtf8(cell: $0, sidecar: sidecarData)
          )
        }
      }
      historyRevisions[requestKey] = (metadata.revision, metadata.request_id)
      let chunk = NativeHistoryRange(
        startLine: metadata.start_line == 0 ? request.startLine : metadata.start_line,
        endLine: metadata.end_line == 0 ? request.endLine : metadata.end_line,
        blockID: metadata.block_id,
        requestID: metadata.request_id,
        revision: metadata.revision,
        rows: rows,
        status: metadata.reserved,
        sourceDisplayHandle: handle
      )
      let previous = historyContinuations[requestKey]
      let merged = mergeHistoryRange(previous?.range, chunk)
      let leads = historyLeadCount(rows)
      _ = seyal_bridge_history_range_consume_for_handle(
        handle, metadata.block_id, metadata.request_id)
      requestedHistoryRanges.removeValue(forKey: requestKey)
      historyRevisions.removeValue(forKey: requestKey)
      historyContinuations.removeValue(forKey: requestKey)
      let truncated = metadata.reserved == 1
      if truncated, leads > 0, isConnected {
        let nextStart = (previous?.startUnit ?? 0) + leads
        let nextID = seyal_bridge_next_history_request_id_for(handle)
        if nextID != 0 {
          let continued = finishAddressedMutation(
            seyal_bridge_request_history_range_for(
              handle, request.blockID, request.startLine, request.endLine, 512, 131_072,
              nextStart),
            forDisplayHandle: handle
          )
          if continued == 0 {
            let nextKey = PaneHistoryRequestKey(
              displayHandle: handle, paneID: paneID, requestID: nextID)
            requestedHistoryRanges[nextKey] = request
            historyContinuations[nextKey] = (nextStart, merged)
          }
        }
      }
      onHistory(merged)
    }
  }

  @discardableResult
  func submitCommittedText(_ text: String) -> Int32 {
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
    let handle = windowFocusedDisplayHandle != 0 ? windowFocusedDisplayHandle : clientHandle
    let result =
      text.utf8.withContiguousStorageIfAvailable { buffer -> Int32 in
        seyal_bridge_submit_utf8_for(handle, buffer.baseAddress, count)
      }
      ?? Array(text.utf8).withUnsafeBufferPointer { buffer in
        seyal_bridge_submit_utf8_for(handle, buffer.baseAddress, count)
      }
    return finishMutation(result)
  }

  /// Empty paste is invalid (`-4`); UTF-8 above `MAX_INPUT_BYTES` is CommitTooLarge (`-14`).
}
