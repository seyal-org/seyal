import Foundation

@MainActor
extension RustDisplayBridge {
  func requestHistoryRange(startLine: UInt64, endLine: UInt64, blockID: UInt64) -> Int32 {
    guard isConnected, selectClient(),
      blockID > 0, startLine > 0, endLine >= startLine
    else { return -4 }
    let requestID = seyal_bridge_next_history_request_id()
    guard requestID != 0 else { return -4 }
    let result = finishMutation(
      seyal_bridge_request_history_range(blockID, startLine, endLine, 512, 131_072, 0))
    if result == 0 {
      let requestKey = PaneHistoryRequestKey(paneID: paneID, requestID: requestID)
      requestedHistoryRanges[requestKey] = (blockID, startLine, endLine)
    }
    return result
  }

  func discardHistoryRequests(except blockIDs: Set<UInt64>) {
    requestedHistoryRanges = requestedHistoryRanges.filter { blockIDs.contains($0.value.blockID) }
    historyRevisions = historyRevisions.filter { requestKey, _ in
      requestedHistoryRanges[requestKey] != nil
    }
    historyContinuations = historyContinuations.filter { requestKey, _ in
      requestedHistoryRanges[requestKey] != nil
    }
  }

  func publishHistoryRanges() {
    guard isConnected, selectClient() else { return }
    for (requestKey, request) in Array(requestedHistoryRanges) {
      let metadata = seyal_bridge_history_range_peek_for(request.blockID, requestKey.requestID)
      guard metadata.block_id != 0,
        metadata.request_id != 0,
        metadata.block_id == request.blockID,
        metadata.request_id == requestKey.requestID,
        metadata.revision > 0,
        historyRevisions[requestKey]?.revision != metadata.revision
          || historyRevisions[requestKey]?.requestID != metadata.request_id
      else { continue }
      let sidecar = seyal_bridge_history_range_sidecar_for(metadata.block_id, metadata.request_id)
      let sidecarData: Data
      if sidecar.len > 0, let bytes = sidecar.bytes {
        sidecarData = Data(bytes: bytes, count: Int(sidecar.len))
      } else {
        sidecarData = Data()
      }
      let rows = (0..<Int(metadata.row_count)).compactMap { index -> [NativeHistoryRange.Cell]? in
        let row = seyal_bridge_history_range_row_for(
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
        status: metadata.reserved
      )
      let previous = historyContinuations[requestKey]
      let merged = mergeHistoryRange(previous?.range, chunk)
      let leads = historyLeadCount(rows)
      _ = seyal_bridge_history_range_consume(metadata.block_id, metadata.request_id)
      requestedHistoryRanges.removeValue(forKey: requestKey)
      historyRevisions.removeValue(forKey: requestKey)
      historyContinuations.removeValue(forKey: requestKey)
      let truncated = metadata.reserved == 1
      if truncated, leads > 0, isConnected, selectClient() {
        let nextStart = (previous?.startUnit ?? 0) + leads
        let nextID = seyal_bridge_next_history_request_id()
        if nextID != 0 {
          let continued = finishMutation(
            seyal_bridge_request_history_range(
              request.blockID, request.startLine, request.endLine, 512, 131_072, nextStart))
          if continued == 0 {
            let nextKey = PaneHistoryRequestKey(paneID: paneID, requestID: nextID)
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
        seyal_bridge_submit_utf8(buffer.baseAddress, count)
      }
      ?? Array(text.utf8).withUnsafeBufferPointer { buffer in
        seyal_bridge_submit_utf8(buffer.baseAddress, count)
      }
    return finishMutation(result)
  }

  /// Empty paste is invalid (`-4`); UTF-8 above `MAX_INPUT_BYTES` is CommitTooLarge (`-14`).
}
