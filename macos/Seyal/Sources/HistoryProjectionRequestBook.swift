import Foundation

struct HistoryProjectionRequestKey: Hashable {
    let displayHandle: UInt64
    let blockID: UInt64
    let startLine: UInt64
    let endLine: UInt64
}

/// Bounded HISTORY request dedup for plan-owned sealed clips and component
/// tests. Production Flow chrome hosts do not own fetches (#1484 Finding D);
/// `MetalSurfaceView.applyBlockViewportPlan` uses this so identical clips are
/// not re-requested every display frame (#1484 100k).
struct HistoryProjectionRequestBook {
    private var inFlight = Set<HistoryProjectionRequestKey>()
    private var settled = Set<HistoryProjectionRequestKey>()

    mutating func begin(_ key: HistoryProjectionRequestKey) -> Bool {
        guard !inFlight.contains(key), !settled.contains(key) else { return false }
        inFlight.insert(key)
        return true
    }

    /// True when any in-flight key shares this Controller + Block.
    func hasInFlight(displayHandle: UInt64, blockID: UInt64) -> Bool {
        inFlight.contains { $0.displayHandle == displayHandle && $0.blockID == blockID }
    }

    /// True when a settled key shares this Controller + Block (and nothing is
    /// still in flight for it).
    func hasSettled(displayHandle: UInt64, blockID: UInt64) -> Bool {
        !hasInFlight(displayHandle: displayHandle, blockID: blockID)
            && settled.contains { $0.displayHandle == displayHandle && $0.blockID == blockID }
    }

    mutating func complete(_ key: HistoryProjectionRequestKey) {
        let matching = Set(inFlight.filter {
            $0.displayHandle == key.displayHandle && $0.blockID == key.blockID
        })
        inFlight.subtract(matching)
        settled.formUnion(matching)
        settled.insert(key)
    }

    mutating func receive(
        _ key: HistoryProjectionRequestKey,
        status: UInt32,
        deliveredRows: Int? = nil,
        minimumRows: Int? = nil
    ) {
        switch status {
        case 0:
            // NATIVE books key endLine to sealed/content row count. A short
            // Complete must stay retryable so sealed growth can reopen (#1484).
            if let deliveredRows, let minimumRows, minimumRows > 0, deliveredRows < minimumRows {
                retry(key)
                return
            }
            complete(key)
        case 1:
            // Truncated history is still being continued by RustDisplayBridge.
            break
        default:
            retry(key)
        }
    }

    mutating func retry(_ key: HistoryProjectionRequestKey) {
        inFlight = Set(inFlight.filter {
            $0.displayHandle != key.displayHandle || $0.blockID != key.blockID
        })
        settled = Set(settled.filter {
            $0.displayHandle != key.displayHandle || $0.blockID != key.blockID
        })
    }

    mutating func retain(blockIDs: Set<UInt64>, forDisplayHandle handle: UInt64) {
        inFlight = inFlight.filter {
            $0.displayHandle != handle || blockIDs.contains($0.blockID)
        }
        settled = settled.filter {
            $0.displayHandle != handle || blockIDs.contains($0.blockID)
        }
    }

    mutating func discard(forDisplayHandle handle: UInt64) {
        inFlight = inFlight.filter { $0.displayHandle != handle }
        settled = settled.filter { $0.displayHandle != handle }
    }
}
