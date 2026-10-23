import Foundation

struct ComposerStatusRelayTracker {
  private var lastKey: (revision: UInt64, attachmentLow: UInt64, attachmentHigh: UInt64)?

  mutating func shouldRelay(
    revision: UInt64,
    attachmentIDLow: UInt64,
    attachmentIDHigh: UInt64
  ) -> Bool {
    guard revision != 0 else { return false }
    if let previous = lastKey,
      previous.revision == revision,
      previous.attachmentLow == attachmentIDLow,
      previous.attachmentHigh == attachmentIDHigh
    {
      return false
    }
    lastKey = (revision, attachmentIDLow, attachmentIDHigh)
    return true
  }

  mutating func clear() -> Bool {
    guard lastKey != nil else { return false }
    lastKey = nil
    return true
  }
}

@MainActor
extension RustDisplayBridge {
  func currentFrame() -> SeyalPreparedFrame? {
    guard isConnected, selectClient() else { return nil }
    let frame = seyal_bridge_frame()
    guard frame.cells != nil, frame.cell_count > 0 else { return nil }
    return frame
  }

  /// Builds the initial PreparedSurface after attach. Idempotent.
  @discardableResult
  func ensurePreparedSurface() -> Bool {
    guard isConnected, selectClient() else { return false }
    return seyal_bridge_ensure_prepared() == 0
  }

  func publishCurrentFrame() {
    guard let frame = currentFrame() else { return }
    onFrame(frame)
  }

  /// Minimal read-only Pass 8 presentation seam. The rich command transcript
  /// remains the independent Pass 7.1 timeline above.
  func currentBlockMetadata() -> RuntimeBlockMetadata? {
    guard isConnected, selectClient() else { return nil }
    let value = seyal_bridge_execution_block_metadata()
    guard value.revision > 0,
      value.start_line_id > 0,
      let state = RuntimeBlockMetadata.State(rawValue: value.state)
    else { return nil }
    return RuntimeBlockMetadata(
      blockIDLow: value.block_id_low,
      blockIDHigh: value.block_id_high,
      revision: value.revision,
      startLineID: value.start_line_id,
      state: state
    )
  }

  func currentTimeline() -> [NativeBlockRecord] {
    guard isConnected, selectClient() else { return [] }
    let count = Int(seyal_bridge_block_count())
    return (0..<count).compactMap { index in
      let record = seyal_bridge_block_record(UInt32(index))
      guard record.id != 0, record.command != nil else { return nil }
      let command =
        String(
          bytes: UnsafeBufferPointer(
            start: record.command,
            count: Int(record.command_len)
          ),
          encoding: .utf8
        ) ?? ""
      return NativeBlockRecord(
        id: record.id,
        command: command,
        state: record.state == 0 ? .running : .completed,
        startLine: record.start_line,
        endLine: record.end_line == 0 ? nil : record.end_line,
        exitStatus: record.exit_status
      )
    }
  }

  func nextComposerRequestID() -> UInt64 {
    guard isConnected, selectClient() else { return 0 }
    return seyal_bridge_next_composer_request_id()
  }

  /// Relay a newer Runtime composer eligibility for the focused display
  /// attachment. Rust owns the status meaning and revision; the host only
  /// scopes deduplication to the attachment that produced it.
  func publishComposerStatus() {
    guard isConnected, selectClient() else { return }
    let status = seyal_bridge_composer_status()
    let shouldRelay = composerStatusRelayTracker.shouldRelay(
      revision: status.revision,
      attachmentIDLow: status.attachment_id_low,
      attachmentIDHigh: status.attachment_id_high
    )
    guard shouldRelay else { return }
    onComposerStatus(
      NativeComposerStatus(eligibility: status.eligibility, revision: status.revision)
    )
  }

  /// Transport lost: the relayed fact no longer describes a live attachment.
  func clearComposerStatus() {
    guard composerStatusRelayTracker.clear() else { return }
    onComposerStatus(.cleared)
  }

  func publishComposerResult() {
    guard isConnected, selectClient() else { return }
    let result = seyal_bridge_composer_result()
    guard result.request_id != 0,
      result.request_id != lastComposerResultRequestID
    else { return }
    lastComposerResultRequestID = result.request_id
    let code: NativeComposerResult.Code
    switch result.code {
    case 0: code = .accepted
    case 1: code = .busy
    case 2: code = .unsupported
    case 3: code = .backpressure
    default: code = .invalid
    }
    onComposerResult(
      NativeComposerResult(
        requestID: result.request_id,
        blockID: result.block_id,
        code: code
      ))
  }
}
