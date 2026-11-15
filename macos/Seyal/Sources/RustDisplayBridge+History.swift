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
    blockID: UInt64,
    startUnit: UInt32 = 0,
    unitLimit: UInt32 = 0
  ) -> Int32 {
    guard isConnected, handle != 0 else { return -4 }
    // Finding E: (0, 0) is sealed-row indexed retrieval; startUnit is the
    // sealed index from BlockViewportPlan clipping.
    let sealedIndexMode = startLine == 0 && endLine == 0
    guard blockID > 0, sealedIndexMode || (startLine > 0 && endLine >= startLine) else {
      return -4
    }
    // Address the named Controller directly. `seyal_bridge_select` only moves
    // poll/flush; focused history FFI would hit FOCUSED_DISPLAY_HANDLE (#1426).
    let requestID = seyal_bridge_next_history_request_id_for(handle)
    guard requestID != 0 else { return -4 }
    let result = finishAddressedMutation(
      seyal_bridge_request_history_range_for(
        handle, blockID, startLine, endLine, 512, 131_072, startUnit),
      forDisplayHandle: handle
    )
    if result == 0 {
      let requestKey = PaneHistoryRequestKey(
        displayHandle: handle, paneID: paneID, requestID: requestID)
      let clipEnd: UInt32 =
        sealedIndexMode && unitLimit > 0
        ? startUnit &+ unitLimit
        : 0
      requestedHistoryRanges[requestKey] = (
        blockID, startLine, endLine, startUnit, clipEnd
      )
    }
    return result
  }

  /// Submit all visible RegionGrid clips in one Rust-owned viewport request.
  /// The returned RegionId rows enter the existing bounded native history
  /// cache under one shared request ID.
  func requestBlockViewportContent(
    forDisplayHandle handle: UInt64,
    planGeneration: UInt64,
    clips: [(regionID: UInt64, firstRow: UInt32, rowCount: UInt32)]
  ) -> UInt64 {
    guard isConnected, handle != 0, planGeneration != 0, !clips.isEmpty else { return 0 }
    // Rust may return an existing request ID when this exact generation and
    // clip set is already in flight. The next-ID accessor is only a prediction
    // and must not be used as correlation authority.
    let requestID = seyal_bridge_request_block_viewport_content_for(handle, planGeneration)
    guard requestID != 0 else { return 0 }
    recordBlockViewportContentRequest(
      forDisplayHandle: handle, returnedRequestID: requestID, clips: clips)
    return requestID
  }

  /// Correlate every selected Rust clip with the ID Rust actually returned.
  /// Deduplication may reuse an in-flight ID rather than the next predicted ID.
  func recordBlockViewportContentRequest(
    forDisplayHandle handle: UInt64,
    returnedRequestID requestID: UInt64,
    clips: [(regionID: UInt64, firstRow: UInt32, rowCount: UInt32)]
  ) {
    guard handle != 0, requestID != 0 else { return }
    requestedHistoryRanges = requestedHistoryRanges.filter {
      $0.key.displayHandle != handle || $0.key.cacheBlockID == 0
    }
    historyRevisions = historyRevisions.filter { requestedHistoryRanges[$0.key] != nil }
    historyContinuations = historyContinuations.filter { requestedHistoryRanges[$0.key] != nil }
    for clip in clips where clip.regionID != 0 && clip.rowCount > 0 {
      let key = PaneHistoryRequestKey(
        displayHandle: handle,
        paneID: paneID,
        requestID: requestID,
        cacheBlockID: clip.regionID
      )
      requestedHistoryRanges[key] = (
        clip.regionID, 0, 0, clip.firstRow, clip.firstRow &+ clip.rowCount
      )
    }
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
      key.displayHandle != handle || key.cacheBlockID != 0 || blockIDs.contains(value.blockID)
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
    // Idle typing/echo polls must not pay FFI when no HISTORY is outstanding.
    guard !pending.isEmpty else { return }
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
      let leads = historyLeadCount(rows)
      _ = seyal_bridge_history_range_consume_for_handle(
        handle, metadata.block_id, metadata.request_id)
      requestedHistoryRanges.removeValue(forKey: requestKey)
      historyRevisions.removeValue(forKey: requestKey)
      historyContinuations.removeValue(forKey: requestKey)
      let truncated = metadata.reserved == 1
      // Finding E sealed-index mode: advance by delivered sealed rows, not
      // padded lead counts (Unicode/reflow). Legacy LineId spans still use
      // lead-count continuation. Plan windows must not Truncate-merge the
      // whole Block (#1484 100k).
      let sealedIndexMode = request.startLine == 0 && request.endLine == 0
      let pageAdvance: UInt32 = sealedIndexMode
        ? UInt32(rows.count)
        : leads
      let published: NativeHistoryRange
      if sealedIndexMode {
        published = NativeHistoryRange(
          startLine: UInt64(request.startUnit),
          endLine: UInt64(request.startUnit) &+ UInt64(rows.count),
          blockID: chunk.blockID,
          requestID: chunk.requestID,
          revision: chunk.revision,
          rows: chunk.rows,
          status: chunk.status,
          sourceDisplayHandle: chunk.sourceDisplayHandle
        )
      } else {
        published = mergeHistoryRange(previous?.range, chunk)
      }
      if truncated, pageAdvance > 0, isConnected {
        let baseUnit = previous?.startUnit ?? request.startUnit
        let nextStart = baseUnit &+ pageAdvance
        let withinPlanWindow =
          request.clipEndUnit == 0 || nextStart < request.clipEndUnit
        if withinPlanWindow {
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
              requestedHistoryRanges[nextKey] = (
                request.blockID, request.startLine, request.endLine, nextStart,
                request.clipEndUnit
              )
              historyContinuations[nextKey] = (nextStart, published)
            }
          }
        }
      }
      onHistory(published)
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
