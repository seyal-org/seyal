#ifndef SEYAL_APP_H
#define SEYAL_APP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Versioned, size-tagged one-Pane application-root ABI (ADR-015 / #906).
 *
 * Pointer-bearing fields are borrowed until the next mutating bridge call
 * (apply, snapshot buffer replace, accessibility, destroy). The host must
 * copy synchronously and may retain only its derived copy. A stale generation
 * must not authorize actions.
 *
 * Product snapshots and Candidate-D prepared frames are separate transfers.
 */

#define SEYAL_APP_ABI_VERSION 1u

enum SeyalAppActionKind {
    SEYAL_APP_ACTION_FOCUS = 0,
    SEYAL_APP_ACTION_BIND = 1,
    SEYAL_APP_ACTION_REFRESH = 2,
    SEYAL_APP_ACTION_SUBMIT_INPUT = 3,
    SEYAL_APP_ACTION_QUIT = 4,
    SEYAL_APP_ACTION_ACK_EFFECT = 5,
    SEYAL_APP_ACTION_BEGIN_RECOVERY = 6,
    SEYAL_APP_ACTION_COMPLETE_RECOVERY = 7,
    SEYAL_APP_ACTION_FIRE_RECOVERY = 8,
    SEYAL_APP_ACTION_ACK_RECOVERY = 9,
    SEYAL_APP_ACTION_SET_COMPOSER_DRAFT = 10,
    SEYAL_APP_ACTION_SUBMIT_COMPOSER = 11,
    SEYAL_APP_ACTION_APPLY_COMPOSER_RESULT = 12,
    SEYAL_APP_ACTION_APPLY_RUNTIME_BLOCKS = 13,
    SEYAL_APP_ACTION_SET_LEFT_PANEL = 14,
    SEYAL_APP_ACTION_SET_INSPECTOR = 15,
    SEYAL_APP_ACTION_SELECT_AGENT = 16,
    SEYAL_APP_ACTION_OPEN_ATTENTION = 17,
    SEYAL_APP_ACTION_REPLACE_CHROME = 18,
    SEYAL_APP_ACTION_SELECT_WORKSPACE = 19,
    SEYAL_APP_ACTION_SELECT_TAB = 20,
    SEYAL_APP_ACTION_FOCUS_PANE = 21,
    SEYAL_APP_ACTION_SET_SHELL_CHROME = 22,
    /*
     * Composer history recall (#933).
     * SET_COMPOSER_HISTORY_FILTER: payload = UTF-8 query.
     * MOVE_COMPOSER_HISTORY_SELECTION: reserved = signed row delta (int32).
     * SELECT_COMPOSER_HISTORY: target_pty_generation = composer epoch.
     */
    SEYAL_APP_ACTION_OPEN_COMPOSER_HISTORY = 40,
    SEYAL_APP_ACTION_SET_COMPOSER_HISTORY_FILTER = 41,
    SEYAL_APP_ACTION_MOVE_COMPOSER_HISTORY_SELECTION = 42,
    SEYAL_APP_ACTION_SELECT_COMPOSER_HISTORY = 43,
    SEYAL_APP_ACTION_CLOSE_COMPOSER_HISTORY = 44,
    /*
     * Block details inspector (#935).
     * SELECT_BLOCK: target_execution_lo/hi = BlockId (from seyal_app_block_row).
     * Selecting binds the inspector to that Block, switches inspector mode to
     * SEYAL_APP_INSPECTOR_BLOCK and reveals the inspector. Fails closed for a
     * Block not in the focused Pane's list. Error code 30 = UnknownBlock.
     */
    SEYAL_APP_ACTION_SELECT_BLOCK = 45,
    SEYAL_APP_ACTION_CLEAR_BLOCK_SELECTION = 46,
    /*
     * Global keyboard-first command palette (#932). The command list is never
     * sent by the host. Error codes 26-29.
     */
    SEYAL_APP_ACTION_OPEN_PALETTE = 47,
    SEYAL_APP_ACTION_SET_PALETTE_QUERY = 48,
    SEYAL_APP_ACTION_MOVE_PALETTE_SELECTION = 49,
    SEYAL_APP_ACTION_RUN_PALETTE = 50,
    SEYAL_APP_ACTION_CLOSE_PALETTE = 51,
    /*
     * Relay the attached client's Runtime-published composer eligibility
     * (ADR-009 invariant 7; #978). reserved = SeyalAppComposerEligibility,
     * target_execution_lo = Runtime revision (from seyal_bridge_composer_status).
     * The host copies the value; it never decides eligibility. Rust ignores a
     * revision older than the one it holds. reserved = NONE clears the fact
     * (transport lost) and the composer reads busy until Runtime republishes.
     */
    SEYAL_APP_ACTION_APPLY_COMPOSER_STATUS = 52
};

/* SEYAL_APP_ACTION_APPLY_COMPOSER_STATUS reserved values. */
enum SeyalAppComposerEligibility {
    SEYAL_APP_COMPOSER_ELIGIBILITY_NONE = 0,
    SEYAL_APP_COMPOSER_ELIGIBILITY_AVAILABLE = 1,
    SEYAL_APP_COMPOSER_ELIGIBILITY_BUSY = 2,
    SEYAL_APP_COMPOSER_ELIGIBILITY_UNSUPPORTED = 3
};

/* SEYAL_APP_ACTION_SET_INSPECTOR reserved values and SeyalAppChrome.inspector_mode. */
enum SeyalAppInspectorMode {
    SEYAL_APP_INSPECTOR_CONTEXT = 0,
    SEYAL_APP_INSPECTOR_WORKSPACE = 1,
    SEYAL_APP_INSPECTOR_TAB = 2,
    SEYAL_APP_INSPECTOR_PANE = 3,
    SEYAL_APP_INSPECTOR_BLOCK = 4
};

enum SeyalAppEligibility {
    SEYAL_APP_ELIGIBILITY_UNBOUND = 0,
    SEYAL_APP_ELIGIBILITY_FLOW = 1,
    SEYAL_APP_ELIGIBILITY_RAW = 2,
    SEYAL_APP_ELIGIBILITY_TUI = 3
};

/*
 * Recovery actions keep the 120-byte SeyalAppAction record.
 * BEGIN/FIRE: target_pty_generation = host clock milliseconds.
 * COMPLETE: target_execution_lo = episode generation,
 *           reserved = outcome | (launch << 8),
 *           target_attachment_lo = opened handle,
 *           target_pty_generation = host clock milliseconds.
 */
enum SeyalAppRecoveryStage {
    SEYAL_APP_RECOVERY_DISCONNECTED = 0,
    SEYAL_APP_RECOVERY_DISCOVERING = 1,
    SEYAL_APP_RECOVERY_STARTING = 2,
    SEYAL_APP_RECOVERY_WAITING_CONTROLLER = 3,
    SEYAL_APP_RECOVERY_RECONSTRUCTING = 4,
    SEYAL_APP_RECOVERY_RESTORING = 5,
    SEYAL_APP_RECOVERY_USABLE = 6,
    SEYAL_APP_RECOVERY_EXHAUSTED = 7,
    SEYAL_APP_RECOVERY_BLOCKED = 8
};

enum SeyalAppRecoveryOutcome {
    SEYAL_APP_RECOVERY_CONNECTED = 0,
    SEYAL_APP_RECOVERY_OPENED_ADOPTED = 1,
    SEYAL_APP_RECOVERY_OPENED_REJECTED = 2,
    SEYAL_APP_RECOVERY_ENDPOINT_MISSING = 3,
    SEYAL_APP_RECOVERY_RETRYABLE = 4,
    SEYAL_APP_RECOVERY_CONTROLLER_BUSY = 5,
    SEYAL_APP_RECOVERY_BLOCKED_OUTCOME = 6
};

enum SeyalAppRecoveryLaunch {
    SEYAL_APP_RECOVERY_LAUNCH_NONE = 0,
    SEYAL_APP_RECOVERY_LAUNCH_STARTED = 1,
    SEYAL_APP_RECOVERY_LAUNCH_HELPER_MISSING = 2
};

enum SeyalAppRecoveryEffect {
    SEYAL_APP_RECOVERY_EFFECT_NONE = 0,
    SEYAL_APP_RECOVERY_EFFECT_PERFORM_ATTEMPT = 1,
    SEYAL_APP_RECOVERY_EFFECT_SCHEDULE = 2,
    SEYAL_APP_RECOVERY_EFFECT_LAUNCH_HELPER = 3,
    SEYAL_APP_RECOVERY_EFFECT_DISPOSE_HANDLE = 4
};

enum SeyalAppAxRole {
    SEYAL_APP_AX_APPLICATION = 0,
    SEYAL_APP_AX_PANE = 1,
    SEYAL_APP_AX_COMPOSER = 2,
    SEYAL_APP_AX_TERMINAL = 3
};

typedef struct SeyalAppAction {
    uint16_t version;
    uint16_t size;
    uint16_t kind;
    uint16_t flags;
    uint64_t fence_pane_lo;
    uint64_t fence_pane_hi;
    uint64_t fence_execution_lo;
    uint64_t fence_execution_hi;
    uint64_t fence_attachment_lo;
    uint64_t fence_attachment_hi;
    uint64_t fence_epoch;
    uint64_t target_execution_lo;
    uint64_t target_execution_hi;
    uint64_t target_attachment_lo;
    uint64_t target_attachment_hi;
    uint64_t target_pty_generation;
    const uint8_t *payload;
    uint32_t payload_len;
    uint32_t reserved;
} SeyalAppAction;

#define SEYAL_APP_FLAG_HAS_EXECUTION 1u
#define SEYAL_APP_FLAG_HAS_ATTACHMENT 2u
#define SEYAL_APP_FLAG_CONTROLLER 4u
#define SEYAL_APP_FLAG_ALTERNATE_SCREEN 8u
#define SEYAL_APP_FLAG_TARGET_CONTROLLER 16u

/*
 * Snapshot flags (SeyalAppSnapshot.flags). Distinct from SeyalAppAction.flags.
 * Hosts must translate SNAP_* into FLAG_* when filling an identity fence.
 */
#define SEYAL_APP_SNAP_COMPOSER 1u
#define SEYAL_APP_SNAP_CONTROLLER 2u
#define SEYAL_APP_SNAP_FROZEN 4u
#define SEYAL_APP_SNAP_HAS_EXECUTION 8u
#define SEYAL_APP_SNAP_HAS_ATTACHMENT 16u

typedef struct SeyalAppSnapshot {
    uint16_t version;
    uint16_t size;
    uint16_t eligibility;
    uint16_t flags;
    uint64_t generation;
    uint64_t pane_lo;
    uint64_t pane_hi;
    uint64_t execution_lo;
    uint64_t execution_hi;
    uint64_t attachment_lo;
    uint64_t attachment_hi;
    uint64_t epoch;
    uint32_t last_error;
    uint32_t pending_effect;
    const uint8_t *output_utf8;
    uint32_t output_utf8_len;
    uint32_t reserved;
    uint16_t recovery_stage;
    uint16_t recovery_attempts;
    uint32_t recovery_effect;
    uint64_t recovery_generation;
} SeyalAppSnapshot;

typedef struct SeyalAppAxNode {
    uint64_t id;
    uint64_t parent;
    uint8_t role;
    uint8_t enabled;
    uint8_t selected;
    uint8_t focused;
    uint32_t actions;
    const uint8_t *label;
    uint32_t label_len;
    uint32_t reserved0;
    const uint8_t *value;
    uint32_t value_len;
    uint32_t reserved1;
    const uint8_t *help;
    uint32_t help_len;
    uint32_t reserved2;
} SeyalAppAxNode;

enum SeyalAppComposerMode {
    SEYAL_APP_COMPOSER_HIDDEN = 0,
    SEYAL_APP_COMPOSER_AVAILABLE = 1,
    SEYAL_APP_COMPOSER_BUSY = 2
};

#define SEYAL_APP_COMPOSER_CAN_SUBMIT 1u
#define SEYAL_APP_COMPOSER_DIRECT_TERMINAL 2u

#define SEYAL_APP_COPY_COMPOSER_PLACEHOLDER 0u
#define SEYAL_APP_COPY_COMPOSER_EXECUTE 1u
#define SEYAL_APP_COPY_BLOCK_PROMPT 2u
#define SEYAL_APP_COPY_COMPOSER_HISTORY 3u
#define SEYAL_APP_COPY_COMPOSER_HISTORY_PLACEHOLDER 4u

/*
 * seyal_app_block_row flags: state in the low three bits, plus
 * SEYAL_APP_BLOCK_SELECTED when that Block is bound to the inspector (#935).
 * Hosts must mask with SEYAL_APP_BLOCK_STATE_MASK before comparing states.
 */
#define SEYAL_APP_BLOCK_STATE_RUNNING 1u
#define SEYAL_APP_BLOCK_STATE_COMPLETED 2u
#define SEYAL_APP_BLOCK_STATE_FAILED 3u
/* Completed without an observed exit status; never success or failure. */
#define SEYAL_APP_BLOCK_STATE_UNKNOWN 4u
#define SEYAL_APP_BLOCK_STATE_MASK 7u
#define SEYAL_APP_BLOCK_SELECTED 8u

typedef struct SeyalAppComposer {
    uint16_t version;
    uint16_t size;
    uint16_t mode;
    uint16_t flags;
    uint64_t epoch;
    uint64_t request_id;
    const uint8_t *draft_utf8;
    uint32_t draft_utf8_len;
    uint32_t block_count;
} SeyalAppComposer;

/*
 * Pane-local composer history overlay (#933). `query_utf8` is borrowed until
 * the next mutating bridge call. Rows come from seyal_app_history_row; the
 * selected row carries SEYAL_APP_ROW_SELECTED. Hosts show the overlay only
 * while SEYAL_APP_HISTORY_OPEN is set and enable the recall affordance only
 * while SEYAL_APP_HISTORY_HAS_ENTRIES is set.
 */
typedef struct SeyalAppComposerHistory {
    uint16_t version;
    uint16_t size;
    uint16_t flags;
    uint16_t selected;
    uint32_t entry_count;
    uint32_t row_count;
    const uint8_t *query_utf8;
    uint32_t query_utf8_len;
    uint32_t reserved;
} SeyalAppComposerHistory;

#define SEYAL_APP_HISTORY_OPEN 1u
#define SEYAL_APP_HISTORY_HAS_ENTRIES 2u

typedef struct SeyalAppChrome {
    uint16_t version;
    uint16_t size;
    uint16_t left_panel;
    uint16_t inspector_mode;
    uint32_t agent_count;
    uint32_t attention_count;
    uint32_t inspector_row_count;
    /* SEYAL_APP_CHROME_* visibility bits. Zero is M001 first-UI receded chrome. */
    uint32_t reserved;
} SeyalAppChrome;

#define SEYAL_APP_CHROME_LEFT_VISIBLE 1u
#define SEYAL_APP_CHROME_INSPECTOR_VISIBLE 2u
#define SEYAL_APP_CHROME_TAB_STRIP_VISIBLE 4u

typedef struct SeyalAppAccessibility {
    uint16_t version;
    uint16_t size;
    uint32_t node_count;
    const SeyalAppAxNode *nodes;
    uint32_t reserved;
} SeyalAppAccessibility;

typedef struct SeyalAppTheme {
    uint32_t canvas;
    uint32_t text;
    uint32_t accent;
    uint16_t appearance;
    uint16_t reserved;
} SeyalAppTheme;

typedef struct SeyalAppShell {
    uint16_t version;
    uint16_t size;
    uint16_t workspace_count;
    uint16_t tab_count;
    uint16_t pane_count;
    uint16_t flags;
    uint32_t reserved;
    uint64_t active_workspace_lo;
    uint64_t active_workspace_hi;
    uint64_t active_tab_lo;
    uint64_t active_tab_hi;
    uint64_t focused_pane_lo;
    uint64_t focused_pane_hi;
} SeyalAppShell;

#define SEYAL_APP_ROW_WORKSPACE 0u
#define SEYAL_APP_ROW_TAB 1u
#define SEYAL_APP_ROW_PANE 2u
#define SEYAL_APP_ROW_INSPECTOR 0u
#define SEYAL_APP_ROW_AGENT 1u
#define SEYAL_APP_ROW_ATTENTION 2u
#define SEYAL_APP_ROW_SELECTED 1u

typedef struct SeyalAppRow {
    uint16_t kind;
    uint16_t flags;
    uint32_t reserved;
    uint64_t id_lo;
    uint64_t id_hi;
    const uint8_t *title;
    uint32_t title_len;
    uint32_t reserved1;
    const uint8_t *detail;
    uint32_t detail_len;
    uint32_t reserved2;
} SeyalAppRow;

/*
 * Global command palette overlay (#932). `query_utf8` is borrowed until the
 * next mutating bridge call. Rows come from seyal_app_palette_row; `title`
 * is the command label, `detail` its category (e.g. "Navigation", "View").
 */
typedef struct SeyalAppPalette {
    uint16_t version;
    uint16_t size;
    uint16_t flags;
    uint16_t selected;
    uint32_t row_count;
    const uint8_t *query_utf8;
    uint32_t query_utf8_len;
    uint32_t reserved;
} SeyalAppPalette;

#define SEYAL_APP_PALETTE_OPEN 1u

uint64_t seyal_app_create(void);
int32_t seyal_app_destroy(uint64_t handle);
uint8_t seyal_app_option_as_alt(uint64_t handle);
int32_t seyal_app_apply(uint64_t handle, const SeyalAppAction *action);
SeyalAppSnapshot seyal_app_snapshot(uint64_t handle);
SeyalAppComposer seyal_app_composer(uint64_t handle);
SeyalAppChrome seyal_app_chrome(uint64_t handle);
SeyalAppShell seyal_app_shell(uint64_t handle);
SeyalAppRow seyal_app_shell_row(uint64_t handle, uint16_t kind, uint32_t index);
SeyalAppRow seyal_app_chrome_row(uint64_t handle, uint16_t kind, uint32_t index);
SeyalAppRow seyal_app_block_row(uint64_t handle, uint32_t index);
SeyalAppRow seyal_app_copy(uint64_t handle, uint16_t kind);
SeyalAppComposerHistory seyal_app_composer_history(uint64_t handle);
SeyalAppRow seyal_app_history_row(uint64_t handle, uint32_t index);
SeyalAppPalette seyal_app_palette(uint64_t handle);
SeyalAppRow seyal_app_palette_row(uint64_t handle, uint32_t index);

typedef struct SeyalAppBlockSpan {
    uint64_t start_line;
    uint64_t end_line;
} SeyalAppBlockSpan;

SeyalAppBlockSpan seyal_app_block_span(uint64_t handle, uint32_t index);

/* #865 Flow output projection. Hosts must not invent start+511 ranges.
 * PRIMARY_CLIP: reserved0 = first prepared-frame row; reserved1 = row count.
 * HISTORY: start_line/end_line inclusive; reserved0/reserved1 unused (0). */
#define SEYAL_APP_BLOCK_PROJECTION_FAIL_CLOSED 0u
#define SEYAL_APP_BLOCK_PROJECTION_HISTORY 1u
#define SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP 2u

typedef struct SeyalAppBlockProjection {
    uint16_t kind;
    uint16_t reserved0;
    uint32_t reserved1;
    uint64_t start_line;
    uint64_t end_line;
} SeyalAppBlockProjection;

SeyalAppBlockProjection seyal_app_block_projection(uint64_t handle, uint32_t index);
uint64_t seyal_app_recovery_param(uint64_t handle);
SeyalAppAccessibility seyal_app_accessibility(uint64_t handle);
SeyalAppTheme seyal_app_theme(uint16_t appearance);
int32_t seyal_app_last_error(uint64_t handle);

#ifdef __cplusplus
}
#endif

#endif
