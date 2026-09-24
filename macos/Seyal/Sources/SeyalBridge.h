#ifndef SEYAL_BRIDGE_H
#define SEYAL_BRIDGE_H

#include <stdint.h>
#include "SeyalApp.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Layout contract: sizes/offsets of the structs below must match the Rust
 * `#[repr(C)]` types in crates/seyal-client/src/ffi.rs (and PreparedCell /
 * HistoryCell). Dual Rust+Swift size/offset tests lock the ABI.
 *
 * Panic policy: seyal-client is built with panic=abort. Bridge entry points
 * must never unwind into Swift; a Rust panic terminates the process.
 *
 * Borrow policy: pointer fields returned by seyal_bridge_frame / history row /
 * block record APIs borrow Rust storage only until the next mutating bridge
 * call (poll/prepare/disconnect/history consume). Swift copies synchronously
 * (NativePreparedFrame owns cells at construction).
 */

typedef struct SeyalPreparedCell {
    uint32_t scalar;
    uint32_t foreground;
    uint32_t background;
    uint16_t flags;
    uint16_t reserved;
} SeyalPreparedCell;

typedef struct SeyalPreparedFrame {
    const SeyalPreparedCell *cells;
    uint32_t cell_count;
    uint64_t generation;
    uint16_t rows;
    uint16_t columns;
    uint16_t cursor_row;
    uint16_t cursor_column;
    uint8_t cursor_visible;
    uint8_t alternate_screen;
    uint8_t full_rebuild;
    uint8_t reserved0;
    uint16_t rebuilt_row_count;
    uint16_t reserved1;
    uint64_t damage_word0;
    uint64_t damage_word1;
    uint64_t damage_word2;
    uint64_t damage_word3;
    const uint8_t *grapheme_utf8;
    uint32_t grapheme_utf8_len;
    uint32_t reserved2;
} SeyalPreparedFrame;

typedef struct SeyalExecutionBlockMetadata {
    uint64_t block_id_low;
    uint64_t block_id_high;
    uint64_t revision;
    uint64_t start_line_id;
    uint8_t state;
    uint8_t reserved[7];
} SeyalExecutionBlockMetadata;

typedef struct SeyalBlockRecord {
    uint64_t id;
    uint64_t start_line;
    uint64_t end_line;
    uint8_t state;
    uint8_t reserved[3];
    int32_t exit_status;
    const uint8_t *command;
    uint32_t command_len;
} SeyalBlockRecord;

typedef struct SeyalHistoryRow {
    uint64_t line_id;
    const struct SeyalHistoryCell *cells;
    uint32_t cell_count;
} SeyalHistoryRow;

typedef struct SeyalHistoryCell {
    uint32_t scalar;
    uint32_t foreground;
    uint32_t background;
    /* bit0 bold, bit1 underline, bit2 inverse, bit3 continuation,
       bits 4-5 terminal width, bit7 sidecar offset in reserved */
    uint16_t flags;
    uint16_t reserved;
} SeyalHistoryCell;

typedef struct SeyalHistoryRange {
    uint64_t start_line;
    uint64_t end_line;
    uint64_t block_id;
    uint64_t request_id;
    uint64_t revision;
    uint32_t row_count;
    uint32_t reserved;
} SeyalHistoryRange;

typedef struct SeyalHistorySidecar {
    const uint8_t *bytes;
    uint32_t len;
    uint32_t reserved;
} SeyalHistorySidecar;

typedef struct SeyalComposerResult {
    uint64_t request_id;
    uint64_t block_id;
    uint8_t code;
    uint8_t reserved[7];
} SeyalComposerResult;

/* Runtime-published composer eligibility (#978): eligibility uses
 * SeyalAppComposerEligibility; revision 0 means nothing published yet. */
typedef struct SeyalComposerStatus {
    uint64_t revision;
    uint8_t eligibility;
    uint8_t reserved[7];
} SeyalComposerStatus;

typedef struct SeyalRecoveryResult {
    uint8_t stage;
    uint8_t failure_class;
    uint8_t retryable;
    uint8_t connection_origin;
    uint64_t handle;
    uint64_t runtime_id_low;
    uint64_t runtime_id_high;
    uint64_t execution_id_low;
    uint64_t execution_id_high;
    uint64_t attachment_id_low;
    uint64_t attachment_id_high;
} SeyalRecoveryResult;

/// Read-only Pass 9 merge-acceptance diagnostic. Never called from PTY/VT/render
/// hot paths; sampled only at quiescent lifecycle points by the soak harness.
typedef struct SeyalPass9DiagSnapshot {
    uint8_t connected;
    uint8_t reserved0[7];
    int32_t socket_fd;
    uint32_t live_handles;
    uint32_t pending_handles;
    uint64_t active_handle;
    uint64_t runtime_id_low;
    uint64_t runtime_id_high;
    uint64_t execution_id_low;
    uint64_t execution_id_high;
    uint64_t attachment_id_low;
    uint64_t attachment_id_high;
} SeyalPass9DiagSnapshot;

enum SeyalTerminalKeyKind {
    SEYAL_KEY_ENTER = 1,
    SEYAL_KEY_TAB = 2,
    SEYAL_KEY_BACKSPACE = 3,
    SEYAL_KEY_ESCAPE = 4,
    SEYAL_KEY_ARROW_UP = 5,
    SEYAL_KEY_ARROW_DOWN = 6,
    SEYAL_KEY_ARROW_RIGHT = 7,
    SEYAL_KEY_ARROW_LEFT = 8,
    SEYAL_KEY_CONTROL_ASCII = 9,
};

int32_t seyal_bridge_connect_first(void);
/// Optional isolated Runtime directory for this process. Must be an absolute
/// path. Production Seyal.app never calls this unless `--runtime-dir` or a
/// test host selected the directory; environment variables are ignored.
int32_t seyal_bridge_set_runtime_dir(const char *path);
uint64_t seyal_bridge_open_first(void);
uint64_t seyal_bridge_open_first_until(uint64_t budget_micros);
uint64_t seyal_bridge_open_first_observer_until(uint64_t budget_micros);
uint64_t seyal_bridge_open_execution(uint64_t execution_low, uint64_t execution_high);
uint64_t seyal_bridge_open_execution_until(
    uint64_t execution_low,
    uint64_t execution_high,
    uint64_t budget_micros
);
int32_t seyal_bridge_adopt_handle(uint64_t handle);
int32_t seyal_bridge_select(uint64_t handle);
void seyal_bridge_disconnect_handle(uint64_t handle);
int32_t seyal_bridge_socket_fd(void);
uint64_t seyal_bridge_execution_id_low(void);
uint64_t seyal_bridge_execution_id_high(void);
uint64_t seyal_bridge_runtime_id_low(void);
uint64_t seyal_bridge_runtime_id_high(void);
uint64_t seyal_bridge_attachment_id_low(void);
uint64_t seyal_bridge_attachment_id_high(void);
SeyalExecutionBlockMetadata seyal_bridge_execution_block_metadata(void);
int32_t seyal_bridge_poll(void);
/// Ensure the initial PreparedSurface exists after attach snapshot commit.
/// Returns 0 on success, negative on failure. Idempotent.
int32_t seyal_bridge_ensure_prepared(void);
int32_t seyal_bridge_wants_write(void);
int32_t seyal_bridge_flush_writable(void);
int32_t seyal_bridge_submit_utf8(const uint8_t *bytes, uint32_t len);
int32_t seyal_bridge_submit_paste(const uint8_t *bytes, uint32_t len);
int32_t seyal_bridge_submit_host_selection(
    uint8_t action,
    uint8_t kind,
    uint16_t start_col,
    uint16_t start_row,
    uint16_t end_col,
    uint16_t end_row
);
int32_t seyal_bridge_submit_host_search(const uint8_t *bytes, uint32_t len, uint8_t forward);
typedef struct SeyalCopiedText {
    const uint8_t *utf8;
    uint32_t len;
    uint32_t reserved;
} SeyalCopiedText;
SeyalCopiedText seyal_bridge_copied_text(void);
int32_t seyal_bridge_copied_text_consume(void);
int32_t seyal_bridge_submit_composer(const uint8_t *bytes, uint32_t len);
int32_t seyal_bridge_request_history_range(
    uint64_t block_id,
    uint64_t start_line,
    uint64_t end_line,
    uint16_t max_lines,
    uint32_t max_cells,
    uint32_t start_unit
);
uint64_t seyal_bridge_next_history_request_id(void);
SeyalHistoryRange seyal_bridge_history_range_peek_for(uint64_t block_id, uint64_t request_id);
SeyalHistoryRow seyal_bridge_history_range_row_for(uint64_t block_id, uint64_t request_id, uint32_t index);
SeyalHistorySidecar seyal_bridge_history_range_sidecar_for(uint64_t block_id, uint64_t request_id);
/* Plain UTF-8 text of a held history response for Block Copy (#1010); same
 * borrowed-bytes shape as the sidecar, valid until the next call. */
SeyalHistorySidecar seyal_bridge_history_range_text_for(uint64_t block_id, uint64_t request_id);
uint8_t seyal_bridge_history_range_consume(uint64_t block_id, uint64_t request_id);
SeyalComposerResult seyal_bridge_composer_result(void);
SeyalComposerStatus seyal_bridge_composer_status(void);
SeyalRecoveryResult seyal_bridge_last_recovery_result(void);
SeyalPass9DiagSnapshot seyal_bridge_pass9_diag_snapshot(void);
int32_t seyal_bridge_submit_key(uint16_t kind, uint32_t scalar);
uint8_t seyal_bridge_supports_key_v2(void);
int32_t seyal_bridge_submit_key_v2(uint16_t kind, uint16_t modifiers, uint32_t value, uint8_t event, uint32_t shifted_ascii, uint32_t action_id);
uint8_t seyal_bridge_mouse_cell(
    double pixel_x,
    double pixel_y_from_top,
    double viewport_width,
    double viewport_height,
    double horizontal_insets,
    double vertical_insets,
    double cell_width,
    double cell_height,
    uint16_t *col,
    uint16_t *row
);
int32_t seyal_bridge_submit_mouse(
    uint8_t kind,
    uint8_t button,
    uint16_t modifiers,
    uint16_t col,
    uint16_t row,
    uint32_t action_id
);
int32_t seyal_bridge_propose_geometry(
    double viewport_width,
    double viewport_height,
    double horizontal_insets,
    double vertical_insets,
    double cell_width,
    double cell_height,
    uint8_t meaningful_layout_epoch
);
int32_t seyal_bridge_retry_resize(void);
int32_t seyal_bridge_input_failure(void);
int32_t seyal_bridge_resize_failure(void);
SeyalPreparedFrame seyal_bridge_frame(void);
uint64_t seyal_bridge_block_timeline_revision(void);
uint64_t seyal_bridge_next_composer_request_id(void);
uint32_t seyal_bridge_block_count(void);
SeyalBlockRecord seyal_bridge_block_record(uint32_t index);
void seyal_bridge_disconnect(void);

#ifdef __cplusplus
}
#endif

#endif
