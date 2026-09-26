import Foundation

struct RuntimeBlockMetadata: Equatable, Sendable {
  enum State: UInt8, Sendable {
    case current = 1
    case completed = 2
  }

  let blockIDLow: UInt64
  let blockIDHigh: UInt64
  let revision: UInt64
  let startLineID: UInt64
  let state: State
}

/// UI registries must include the Pane namespace because Runtime block and
/// history request numbers are only unique within their owning execution.
struct PaneBlockKey: Hashable, Sendable {
  let paneID: String
  let blockID: UInt64

  init(paneID: String, blockID: UInt64) {
    self.paneID = paneID
    self.blockID = blockID
  }

  var accessibilityIdentifier: String {
    "pane.\(paneID).block.\(blockID)"
  }
}

struct PaneHistoryRequestKey: Hashable, Sendable {
  let paneID: String
  let requestID: UInt64

  init(paneID: String, requestID: UInt64) {
    self.paneID = paneID
    self.requestID = requestID
  }
}

struct NativeBlockRecord: Equatable {
  enum State: Equatable {
    case running
    case completed
  }

  let id: UInt64
  let command: String
  let state: State
  let startLine: UInt64
  let endLine: UInt64?
  let exitStatus: Int32
}

struct NativeHistoryRange: Equatable {
  struct Cell: Equatable {
    let scalar: UInt32
    let foreground: UInt32
    let background: UInt32
    let flags: UInt16
    let graphemeUtf8: Data

    init(
      scalar: UInt32,
      foreground: UInt32,
      background: UInt32,
      flags: UInt16,
      graphemeUtf8: Data = Data()
    ) {
      self.scalar = scalar
      self.foreground = foreground
      self.background = background
      self.flags = flags
      self.graphemeUtf8 = graphemeUtf8
    }
  }

  let startLine: UInt64
  let endLine: UInt64
  let blockID: UInt64
  let requestID: UInt64
  let revision: UInt64
  let rows: [[Cell]]
  let status: UInt32

  init(
    startLine: UInt64,
    endLine: UInt64,
    blockID: UInt64,
    requestID: UInt64,
    revision: UInt64,
    rows: [[Cell]],
    status: UInt32 = 0
  ) {
    self.startLine = startLine
    self.endLine = endLine
    self.blockID = blockID
    self.requestID = requestID
    self.revision = revision
    self.rows = rows
    self.status = status
  }
}

func mergeHistoryRange(
  _ previous: NativeHistoryRange?,
  _ chunk: NativeHistoryRange
) -> NativeHistoryRange {
  guard let previous else { return chunk }
  if previous.rows.isEmpty {
    return NativeHistoryRange(
      startLine: previous.startLine,
      endLine: chunk.endLine,
      blockID: chunk.blockID,
      requestID: chunk.requestID,
      revision: chunk.revision,
      rows: chunk.rows,
      status: chunk.status
    )
  }
  if chunk.rows.isEmpty {
    return NativeHistoryRange(
      startLine: previous.startLine,
      endLine: previous.endLine,
      blockID: chunk.blockID,
      requestID: chunk.requestID,
      revision: chunk.revision,
      rows: previous.rows,
      status: chunk.status
    )
  }
  var rows = previous.rows
  if previous.endLine == chunk.startLine {
    rows[rows.count - 1] = rows[rows.count - 1] + chunk.rows[0]
    rows.append(contentsOf: chunk.rows.dropFirst())
  } else {
    rows.append(contentsOf: chunk.rows)
  }
  return NativeHistoryRange(
    startLine: previous.startLine,
    endLine: chunk.endLine == 0 ? previous.endLine : chunk.endLine,
    blockID: chunk.blockID,
    requestID: chunk.requestID,
    revision: chunk.revision,
    rows: rows,
    status: chunk.status
  )
}

func historyLeadCount(_ rows: [[NativeHistoryRange.Cell]]) -> UInt32 {
  rows.reduce(into: 0) { count, row in
    count += UInt32(row.filter { $0.scalar != 0 || !$0.graphemeUtf8.isEmpty }.count)
  }
}

private let historyCellSidecarFlag: UInt16 = 1 << 7

func historyGraphemeUtf8(cell: SeyalHistoryCell, sidecar: Data) -> Data {
  guard cell.flags & historyCellSidecarFlag != 0 else { return Data() }
  let offset = Int(cell.reserved)
  guard offset + 2 <= sidecar.count else { return Data() }
  let length = Int(sidecar[offset]) | (Int(sidecar[offset + 1]) << 8)
  let start = offset + 2
  let end = start + length
  guard length > 0, end <= sidecar.count else { return Data() }
  return sidecar.subdata(in: start..<end)
}

struct NativeComposerResult: Equatable {
  enum Code: Equatable {
    case accepted
    case busy
    case unsupported
    case backpressure
    case invalid
  }

  let requestID: UInt64
  let blockID: UInt64
  let code: Code
}

/// Runtime-published composer eligibility, relayed verbatim to the Rust
/// application root (#978). `eligibility` is `SeyalAppComposerEligibility`;
/// `.cleared` (revision 0) means the transport is gone and Rust must treat
/// the composer as busy until Runtime republishes.
struct NativeComposerStatus: Equatable {
  let eligibility: UInt8
  let revision: UInt64

  static let cleared = NativeComposerStatus(eligibility: 0, revision: 0)
}

/// Geometry for one canonical Block projection on the Pane-owned Metal
/// surface. The surface consumes a complete frame, so lifecycle updates never
/// replace one history buffer at a time or expose a partially updated order.
struct NativeTranscriptRegion: Equatable {
  let id: UInt64
  let origin: NSPoint
  let clip: NSRect
}

struct NativeTranscriptFrame: Equatable {
  let revision: UInt64
  let regions: [NativeTranscriptRegion]
  let surfaceIdentity: ObjectIdentifier?

  init(
    revision: UInt64,
    regions: [NativeTranscriptRegion],
    surfaceIdentity: ObjectIdentifier? = nil
  ) {
    self.revision = revision
    self.regions = regions
    self.surfaceIdentity = surfaceIdentity
  }

  var regionIDs: [UInt64] { regions.map(\.id) }

  var isValid: Bool {
    guard regions.allSatisfy({ $0.id != 0 && $0.clip.width >= 0 && $0.clip.height >= 0 }) else {
      return false
    }
    return Set(regionIDs).count == regionIDs.count
  }

  func applyingDuplicateID(_ id: UInt64) -> NativeTranscriptFrame {
    NativeTranscriptFrame(
      revision: revision,
      regions: regions + [NativeTranscriptRegion(id: id, origin: .zero, clip: .zero)],
      surfaceIdentity: surfaceIdentity
    )
  }
}

// Cancellation handlers may outlive RustDisplayBridge and deinit is
// nonisolated in Swift 6. The coordinator serializes its accounting and
// schedules the thread-local Rust disconnect on the main queue.
