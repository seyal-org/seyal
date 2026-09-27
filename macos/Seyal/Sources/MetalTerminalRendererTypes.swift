import Foundation
import Metal
import QuartzCore

/// Run `operation` while already on the GCD main queue, without
/// `MainActor.assumeIsolated`.
///
/// Apple platforms serialize MainActor work on the main dispatch queue, but
/// Swift 6.0 / Xcode 16.4's `assumeIsolated` checks `_taskIsCurrentExecutor`
/// rather than the queue. AppKit run-loop callbacks (CAMetalDisplayLink,
/// Timer) and GCD main-queue work therefore Trace/BPT if they call
/// `assumeIsolated` or enter a MainActor-isolated protocol witness, even
/// though mutual exclusion still holds. Use only after
/// `dispatchPrecondition(condition: .onQueue(.main))`.

func seyalRunAsMainActorFromMainQueue(_ operation: @MainActor () -> Void) {
    dispatchPrecondition(condition: .onQueue(.main))
    withoutActuallyEscaping(operation) { (fn: @escaping @MainActor () -> Void) in
        let raw = unsafeBitCast(fn, to: (() -> Void).self)
        raw()
    }
}

/// Single-slot GPU completion mailbox (`maximumFramesInFlight == 1`).
///
/// Metal completion handlers run off the main queue; main-queue code drains.
/// Uses a lock-free atomic slot instead of `NSLock` so present/update hot paths
/// do not take a blocking Foundation lock.
final class GPUCompletionMailbox: @unchecked Sendable {
    /// 0 = empty, 1 = success pending, 2 = failure pending
    private let slot = UnsafeMutablePointer<Int32>.allocate(capacity: 1)

    init() {
        slot.initialize(to: 0)
    }

    deinit {
        slot.deinitialize(count: 1)
        slot.deallocate()
    }

    func push(failed: Bool) {
        let value: Int32 = failed ? 2 : 1
        while true {
            let current = slot.pointee
            if OSAtomicCompareAndSwap32Barrier(current, value, slot) {
                return
            }
        }
    }

    func drain() -> [Bool] {
        while true {
            let current = slot.pointee
            if current == 0 {
                return []
            }
            if OSAtomicCompareAndSwap32Barrier(current, 0, slot) {
                return [current == 2]
            }
        }
    }
}

let preparedBoldFlag: UInt16 = 1 << 0
let preparedUnderlineFlag: UInt16 = 1 << 1
let instanceGlyphFlag: UInt32 = 1 << 0
let instanceUnderlineFlag: UInt32 = 1 << 1
let instanceCursorFlag: UInt32 = 1 << 2
let instanceWideGlyphFlag: UInt32 = 1 << 3

struct TerminalInstance {
    var origin: SIMD2<Float>
    var size: SIMD2<Float>
    var uvRect: SIMD4<Float>
    var foreground: UInt32
    var background: UInt32
    var flags: UInt32
    var atlasSlice: UInt32
}

struct DamageMask: Equatable {
    var word0: UInt64 = 0
    var word1: UInt64 = 0
    var word2: UInt64 = 0
    var word3: UInt64 = 0

    var isEmpty: Bool {
        word0 == 0 && word1 == 0 && word2 == 0 && word3 == 0
    }

    mutating func formUnion(_ other: DamageMask) {
        word0 |= other.word0
        word1 |= other.word1
        word2 |= other.word2
        word3 |= other.word3
    }

    mutating func markAll(rows: Int) {
        word0 = 0
        word1 = 0
        word2 = 0
        word3 = 0
        for row in 0..<min(rows, 256) {
            mark(row: row)
        }
    }

    mutating func mark(row: Int) {
        guard row >= 0, row < 256 else { return }
        let bit = UInt64(1) << UInt64(row & 63)
        switch row >> 6 {
        case 0: word0 |= bit
        case 1: word1 |= bit
        case 2: word2 |= bit
        default: word3 |= bit
        }
    }

    func contains(row: Int) -> Bool {
        guard row >= 0, row < 256 else { return false }
        let bit = UInt64(1) << UInt64(row & 63)
        switch row >> 6 {
        case 0: return word0 & bit != 0
        case 1: return word1 & bit != 0
        case 2: return word2 & bit != 0
        default: return word3 & bit != 0
        }
    }
}

struct NativePreparedFrame {
    /// Owned cell copy. Bridge frames are copied at construction so Rust
    /// `PreparedCell` storage never escapes into long-lived Swift state.
    let cells: [SeyalPreparedCell]
    /// Length-prefixed UTF-8 payloads for multi-scalar lead cells (SPEC-011 §12).
    let graphemeUtf8: Data
    let generation: UInt64
    let rows: Int
    let columns: Int
    let cursorRow: Int
    let cursorColumn: Int
    let cursorVisible: Bool
    let alternateScreen: Bool
    let fullRebuild: Bool
    let damage: DamageMask

    init?(bridgeFrame: SeyalPreparedFrame) {
        let rows = Int(bridgeFrame.rows)
        let columns = Int(bridgeFrame.columns)
        let count = Int(bridgeFrame.cell_count)
        guard rows > 0,
              columns > 0,
              rows <= 256,
              columns <= 512,
              count == rows * columns,
              let pointer = bridgeFrame.cells
        else {
            return nil
        }
        // Synchronous consume: copy before any later poll can invalidate Rust.
        cells = Array(UnsafeBufferPointer(start: pointer, count: count))
        if bridgeFrame.grapheme_utf8_len > 0, let graphemePtr = bridgeFrame.grapheme_utf8 {
            graphemeUtf8 = Data(
                bytes: graphemePtr,
                count: Int(bridgeFrame.grapheme_utf8_len)
            )
        } else {
            graphemeUtf8 = Data()
        }
        generation = bridgeFrame.generation
        self.rows = rows
        self.columns = columns
        cursorRow = Int(bridgeFrame.cursor_row)
        cursorColumn = Int(bridgeFrame.cursor_column)
        cursorVisible = bridgeFrame.cursor_visible != 0
        alternateScreen = bridgeFrame.alternate_screen != 0
        fullRebuild = bridgeFrame.full_rebuild != 0
        damage = DamageMask(
            word0: bridgeFrame.damage_word0,
            word1: bridgeFrame.damage_word1,
            word2: bridgeFrame.damage_word2,
            word3: bridgeFrame.damage_word3
        )
    }

    init(
        cells: UnsafeBufferPointer<SeyalPreparedCell>,
        generation: UInt64,
        rows: Int,
        columns: Int,
        cursorRow: Int = 0,
        cursorColumn: Int = 0,
        cursorVisible: Bool = false,
        alternateScreen: Bool = false,
        fullRebuild: Bool = true,
        damage: DamageMask = DamageMask(),
        graphemeUtf8: Data = Data()
    ) {
        self.cells = Array(cells)
        self.graphemeUtf8 = graphemeUtf8
        self.generation = generation
        self.rows = rows
        self.columns = columns
        self.cursorRow = cursorRow
        self.cursorColumn = cursorColumn
        self.cursorVisible = cursorVisible
        self.alternateScreen = alternateScreen
        self.fullRebuild = fullRebuild
        self.damage = damage
    }

    init(
        cells: [SeyalPreparedCell],
        generation: UInt64,
        rows: Int,
        columns: Int,
        cursorRow: Int = 0,
        cursorColumn: Int = 0,
        cursorVisible: Bool = false,
        alternateScreen: Bool = false,
        fullRebuild: Bool = true,
        damage: DamageMask = DamageMask(),
        graphemeUtf8: Data = Data()
    ) {
        self.cells = cells
        self.graphemeUtf8 = graphemeUtf8
        self.generation = generation
        self.rows = rows
        self.columns = columns
        self.cursorRow = cursorRow
        self.cursorColumn = cursorColumn
        self.cursorVisible = cursorVisible
        self.alternateScreen = alternateScreen
        self.fullRebuild = fullRebuild
        self.damage = damage
    }
}

struct HistoryRenderRegion {
    let buffer: MTLBuffer
    let instanceCount: Int
    let clip: CGRect
}

enum MetalTerminalRendererError: Error {
    case unavailableCommandQueue
    case unavailableLibrary
    case unavailableShader
    case unavailablePipeline
    case unavailableSampler
    case unavailableBuffer
    case invalidFrame
    case invalidInstanceLayout
    case gpuCommandCompletionFailuresExhausted
    case presentationSubmissionFailuresExhausted
    case preparationFailuresExhausted
    case glyphAtlas(GlyphAtlasError)
}

enum RendererUpdateResult: Equatable {
    case updated
    case deferred
}

struct DeferredHistoryPrepare {
    let historyRange: NativeHistoryRange
    let region: NativeTranscriptRegion
    let backingScale: CGFloat
}

struct GPUCompletionRetryState: Equatable {
    static let maximumAutomaticRetries = 4

    private(set) var retriesUsed = 0
    private(set) var exhausted = false

    mutating func recordSuccess() {
        retriesUsed = 0
        exhausted = false
    }

    /// Returns true only when another automatic GPU submission is permitted.
    /// The initial submission is not counted here, so four retries means a
    /// persistent failure series can produce at most five failed completions.
    mutating func recordFailureAndClaimRetry() -> Bool {
        guard !exhausted else { return false }
        guard retriesUsed < Self.maximumAutomaticRetries else {
            exhausted = true
            return false
        }
        retriesUsed += 1
        return true
    }

    mutating func resetForExplicitRecovery() {
        recordSuccess()
    }
}

struct MetalRendererStats: Equatable {
    var submittedFrames: UInt64 = 0
    var completedFrames: UInt64 = 0
    var commandCompletionFailures: UInt64 = 0
    var commandCompletionFailureExhaustions: UInt64 = 0
    var coalescedFrames: UInt64 = 0
    var fullRebuilds: UInt64 = 0
    var rebuiltRows: UInt64 = 0
    var rebuiltCells: UInt64 = 0
    var instanceBufferAllocations: UInt64 = 0
    var instanceBytes: UInt64 = 0
}

struct MetalSubmissionTiming {
    let preparedToCommitNanoseconds: UInt64
    let commitToCompletionNanoseconds: UInt64
}

