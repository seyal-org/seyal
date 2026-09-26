import Foundation
import Metal
import QuartzCore

extension MetalTerminalRenderer {
    static func gpuCompletionFailureRecoverySelfTest() -> Bool {
        // This is the exact state machine used by asynchronous production
        // command-completion handling. It intentionally contains no terminal,
        // client-cache or attachment authority, so exhausting it cannot mutate
        // canonical state.
        var recovery = GPUCompletionRetryState()
        var automaticRetries = 0

        // Initial failed submission + four automatic retries. The fifth failed
        // completion exhausts the series and must not claim a fifth retry.
        for _ in 0...GPUCompletionRetryState.maximumAutomaticRetries {
            if recovery.recordFailureAndClaimRetry() {
                automaticRetries += 1
            }
        }
        guard automaticRetries == GPUCompletionRetryState.maximumAutomaticRetries,
              recovery.exhausted,
              !recovery.recordFailureAndClaimRetry()
        else {
            return false
        }

        // Ordinary repeated failure calls cannot restart an exhausted series.
        for _ in 0..<1_000 where recovery.recordFailureAndClaimRetry() {
            return false
        }

        // A lifecycle-driven explicit recovery can restart a finite series.
        recovery.resetForExplicitRecovery()
        guard !recovery.exhausted,
              recovery.retriesUsed == 0,
              recovery.recordFailureAndClaimRetry()
        else {
            return false
        }

        // A genuine successful GPU completion also clears the consecutive
        // failure accounting.
        recovery.recordSuccess()
        return !recovery.exhausted && recovery.retriesUsed == 0
    }

}
