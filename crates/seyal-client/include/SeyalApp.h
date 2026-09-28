#ifndef SEYAL_APP_H
#define SEYAL_APP_H

#include <stddef.h>
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
     * Workspace/Tab/Pane composition mutations (#922). CREATE_TAB/CLOSE_TAB/
     * SPLIT_FOCUSED/CLOSE_PANE route through the same Rust ShellState as
     * SELECT_WORKSPACE/SELECT_TAB/FOCUS_PANE; they fail closed (do not
     * mutate) rather than silently no-op. CLOSE_TAB/CLOSE_PANE:
     * target_execution_lo/hi = TabId/PaneId. SPLIT_FOCUSED: reserved = 0
     * (Right) or 1 (Down). Error codes: 28 = TabCreationUnavailable,
     * 29 = PaneSplitUnavailable, 31 = CannotCloseLastTab,
     * 32 = CannotCloseLastPane, 33 = CannotCloseBoundPane (the Pane is
     * bound to an execution; disposition is not yet available).
     */
    SEYAL_APP_ACTION_CREATE_TAB = 23,
    SEYAL_APP_ACTION_CLOSE_TAB = 24,
    SEYAL_APP_ACTION_SPLIT_FOCUSED = 25,
    SEYAL_APP_ACTION_CLOSE_PANE = 26,
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
     * Global keyboard-first command palette (#932 / SPEC-022 N2).
     * RUN_PALETTE: when the selected row carries a ResourceAddress, payload =
     *   address_version(u16 LE) + address_kind(u16 LE) + address_bytes[len].
     *   Verb/chrome rows send payload_len = 0; Rust runs the frozen command.
     * Navigation never re-resolves by ordinal. Error codes 26-27, 34-43.
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
    SEYAL_APP_ACTION_APPLY_COMPOSER_STATUS = 52,
    /** Cancel the active recovery episode (generation bump → Disconnected). */
    SEYAL_APP_ACTION_CANCEL_RECOVERY = 53,
    /**
     * Advance presentation stage after connect.
     * reserved = SEYAL_APP_RECOVERY_RESTORING (5) or SEYAL_APP_RECOVERY_USABLE (6).
     */
    SEYAL_APP_ACTION_ADVANCE_RECOVERY_STAGE = 54,
    /** Begin a continuity-identity commit attempt. */
    SEYAL_APP_ACTION_BEGIN_RECONSTRUCTION = 55,
    /**
     * Commit Runtime/execution continuity and a fresh attachment (Rust-owned).
     * fence_execution_* = Runtime pin; target_execution_* = execution pin;
     * target_attachment_* = attachment pin. `reserved` is ignored.
     * Controller authority and authoritative-snapshot commitment are derived
     * from the live CLIENTS entry whose identities match those pins. A missing,
     * non-controller, or snapshot-less client fails closed. SeyalAppAction
     * layout is unchanged.
     */
    SEYAL_APP_ACTION_COMMIT_RECONSTRUCTION = 56,
    /** Mark reconstruction disconnected after the host drops the live client. */
    SEYAL_APP_ACTION_DISCONNECT_RECONSTRUCTION = 57,
    /*
     * W4a window actions (ADR-018 §2.2). SELECT_WINDOW / CREATE_WINDOW:
     * target_execution_lo/hi = WindowId / WorkspaceId. CYCLE_WINDOW: reserved
     * = 0 next, 1 previous. REPORT_WINDOW_EVENT: target_execution = WindowId,
     * reserved = event kind (0 became-key … 9 screen/scale, 10 activation-failed).
     * Error 34 = UnknownWindow.
     */
    SEYAL_APP_ACTION_SELECT_WINDOW = 58,
    SEYAL_APP_ACTION_CYCLE_WINDOW = 59,
    SEYAL_APP_ACTION_CREATE_WINDOW = 60,
    SEYAL_APP_ACTION_REPORT_WINDOW_EVENT = 61,
    /**
     * Atomic Navigate(address) (SPEC-022 §4). Payload is required:
     * address_version(u16 LE) + address_kind(u16 LE) + address_bytes[len].
     * Rejected navigate leaves focus unchanged. Navigation errors are 35-44.
     */
    SEYAL_APP_ACTION_NAVIGATE = 62,
    /*
     * Navigation-only goto / quick-switcher (SPEC-022 §7 / N4).
     * reserved = SeyalAppGotoScope. Projects through seyal_app_palette with
     * SEYAL_APP_PALETTE_GOTO; SetPaletteQuery/Move/Run/Close route to goto
     * while open. Goto errors are 45-47.
     */
    SEYAL_APP_ACTION_OPEN_GOTO = 63,
    SEYAL_APP_ACTION_SET_GOTO_SCOPE = 64,
    /*
     * Focus-history Back/Forward (SPEC-022 §6 / N3). Payload is FocusSeq as
     * little-endian u64. History errors are 48 (stale cursor) and 49
     * (unavailable).
     */
    SEYAL_APP_ACTION_HISTORY_BACK = 65,
    SEYAL_APP_ACTION_HISTORY_FORWARD = 66
};

/* SEYAL_APP_ACTION_OPEN_GOTO / SET_GOTO_SCOPE reserved values. */
enum SeyalAppGotoScope {
    SEYAL_APP_GOTO_WORKSPACES = 0,
    SEYAL_APP_GOTO_TABS = 1,
    SEYAL_APP_GOTO_PANES = 2,
    SEYAL_APP_GOTO_SESSIONS = 3
};

#define SEYAL_APP_WINDOW_EVENT_BECAME_KEY 0u
#define SEYAL_APP_WINDOW_EVENT_RESIGNED_KEY 1u
#define SEYAL_APP_WINDOW_EVENT_BECAME_MAIN 2u
#define SEYAL_APP_WINDOW_EVENT_RESIGNED_MAIN 3u
#define SEYAL_APP_WINDOW_EVENT_OCCLUSION_CHANGED 4u
#define SEYAL_APP_WINDOW_EVENT_MINIATURIZED 5u
#define SEYAL_APP_WINDOW_EVENT_DEMINIATURIZED 6u
#define SEYAL_APP_WINDOW_EVENT_ENTERED_FULLSCREEN 7u
#define SEYAL_APP_WINDOW_EVENT_EXITED_FULLSCREEN 8u
#define SEYAL_APP_WINDOW_EVENT_SCREEN_OR_SCALE_CHANGED 9u
/** Host could not realize WindowActivation (SPEC-022 §5 / N5). */
#define SEYAL_APP_WINDOW_EVENT_ACTIVATION_FAILED 10u

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
    /*
     * SEYAL_APP_CHROME_* visibility bits. All three bits set (left, inspector,
     * tab strip visible) is the Core Terminal default; SET_SHELL_CHROME may
     * still recede any of them.
     */
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

/*
 * SeyalAppShell.flags: whether CREATE_TAB/SPLIT_FOCUSED would currently be
 * accepted. Hosts must omit the "+"/split control when the bit is unset
 * rather than show one that always fails closed (mirrors the command
 * palette's own omission of "New Tab"/"Split"; see build_commands).
 */
#define SEYAL_APP_SHELL_ALLOWS_TAB_CREATION 1u
#define SEYAL_APP_SHELL_ALLOWS_PANE_SPLITTING 2u
/*
 * Whether CLOSE_TAB of the active Tab / CLOSE_PANE of the focused Pane would
 * currently be accepted (Rust rejects closing the last Tab/Pane). Hosts omit
 * the close control when the bit is unset instead of re-deriving the rule.
 */
#define SEYAL_APP_SHELL_ALLOWS_TAB_CLOSE 4u
#define SEYAL_APP_SHELL_ALLOWS_PANE_CLOSE 8u

/*
 * Multi-window shell header (#1108 / ADR-018 §2.1). window_count / effect_count
 * index seyal_app_window / seyal_app_native_effect. containment_generation is
 * the monotonic shell fence; shell_last_error is a bounded non-secret code.
 * Active-window tab/pane counts remain for the product-active Window projection.
 */
typedef struct SeyalAppShell {
    uint16_t version;
    uint16_t size;
    uint16_t workspace_count;
    uint16_t tab_count;
    uint16_t pane_count;
    uint16_t flags;
    uint16_t window_count;
    uint16_t effect_count;
    uint32_t shell_last_error;
    uint64_t active_workspace_lo;
    uint64_t active_workspace_hi;
    uint64_t active_tab_lo;
    uint64_t active_tab_hi;
    uint64_t focused_pane_lo;
    uint64_t focused_pane_hi;
    uint64_t containment_generation;
    uint64_t active_window_lo;
    uint64_t active_window_hi;
    uint64_t last_active_workspace_lo;
    uint64_t last_active_workspace_hi;
} SeyalAppShell;

/* SeyalAppWindow.flags */
#define SEYAL_APP_WINDOW_PRODUCT_ACTIVE 1u
#define SEYAL_APP_WINDOW_ATTENTION 2u

typedef struct SeyalAppWindow {
    uint16_t version;
    uint16_t size;
    uint16_t tab_count;
    uint16_t flags;
    uint64_t window_lo;
    uint64_t window_hi;
    uint64_t workspace_lo;
    uint64_t workspace_hi;
    uint64_t active_tab_lo;
    uint64_t active_tab_hi;
    const uint8_t *title;
    uint32_t title_len;
    uint32_t reserved;
} SeyalAppWindow;

/* SeyalAppTab.flags / layout */
#define SEYAL_APP_TAB_ACTIVE 1u
#define SEYAL_APP_TAB_ATTENTION 2u
#define SEYAL_APP_TAB_LAYOUT_SINGLE 0u
#define SEYAL_APP_TAB_LAYOUT_SPLIT_RIGHT 1u
#define SEYAL_APP_TAB_LAYOUT_SPLIT_DOWN 2u

typedef struct SeyalAppTab {
    uint16_t version;
    uint16_t size;
    uint16_t pane_count;
    uint16_t tree_node_count;
    uint16_t flags;
    uint16_t layout;
    uint64_t tab_lo;
    uint64_t tab_hi;
    uint64_t focused_pane_lo;
    uint64_t focused_pane_hi;
    const uint8_t *title;
    uint32_t title_len;
    uint32_t reserved;
} SeyalAppTab;

/* SeyalAppPaneLeaf.flags / presentation_tier (ADR-018 §5) */
#define SEYAL_APP_PANE_FOCUSED 1u
#define SEYAL_APP_PANE_HAS_EXECUTION 2u
#define SEYAL_APP_TIER_FOCUSED 0u
#define SEYAL_APP_TIER_VISIBLE 1u
#define SEYAL_APP_TIER_HIDDEN 2u
#define SEYAL_APP_TIER_UNPRESENTED 3u

typedef struct SeyalAppPaneLeaf {
    uint16_t version;
    uint16_t size;
    uint16_t flags;
    uint16_t presentation_tier;
    uint64_t pane_lo;
    uint64_t pane_hi;
    uint64_t execution_lo;
    uint64_t execution_hi;
    const uint8_t *title;
    uint32_t title_len;
    uint32_t reserved;
} SeyalAppPaneLeaf;

/* SeyalAppPaneTreeNode.kind: 0 leaf, 1 split-right, 2 split-down.
 * first/second are relative indices within the Tab's tree_node_count. */
typedef struct SeyalAppPaneTreeNode {
    uint16_t version;
    uint16_t size;
    uint16_t kind;
    uint16_t reserved;
    uint64_t pane_lo;
    uint64_t pane_hi;
    uint32_t first;
    uint32_t second;
} SeyalAppPaneTreeNode;

/*
 * ADR-018 §2.4 native effects in commit order.
 * kind: 1 BoundedDetachThenTerminate (window_lo = relative deadline_ms),
 *       2 RealizeWindow, 3 DestroyWindowRealization, 4 OrderFrontMakeKey,
 *       5 QuitCleanupComplete.
 */
typedef struct SeyalAppNativeEffect {
    uint16_t version;
    uint16_t size;
    uint16_t kind;
    uint16_t reserved;
    uint64_t window_lo;
    uint64_t window_hi;
} SeyalAppNativeEffect;

#define SEYAL_APP_EFFECT_BOUNDED_DETACH_THEN_TERMINATE 1u
#define SEYAL_APP_EFFECT_REALIZE_WINDOW 2u
#define SEYAL_APP_EFFECT_DESTROY_WINDOW_REALIZATION 3u
#define SEYAL_APP_EFFECT_ORDER_FRONT_MAKE_KEY 4u
#define SEYAL_APP_EFFECT_QUIT_CLEANUP_COMPLETE 5u

/* seyal_app_record_compatible kind values. */
#define SEYAL_APP_RECORD_SHELL 0u
#define SEYAL_APP_RECORD_WINDOW 1u
#define SEYAL_APP_RECORD_TAB 2u
#define SEYAL_APP_RECORD_PANE_LEAF 3u
#define SEYAL_APP_RECORD_TREE_NODE 4u
#define SEYAL_APP_RECORD_NATIVE_EFFECT 5u

/*
 * Pane regions (#923): one per leaf of the active Tab's PaneTree, index
 * 0..<SeyalAppShell.pane_count in the same order as SEYAL_APP_ROW_PANE rows.
 * x/y/width/height are unit fractions of the Tab's center area, origin
 * top-left; hosts position regions and never derive geometry. LIVE marks the
 * single region that hosts the live terminal/Metal/composer surface; no region
 * is LIVE while the focused Pane is not the execution's Pane. Out-of-range
 * indices return size == 0.
 */
#define SEYAL_APP_PANE_REGION_FOCUSED 1u
#define SEYAL_APP_PANE_REGION_LIVE 2u

typedef struct SeyalAppPaneRegion {
    uint16_t version;
    uint16_t size;
    uint16_t flags;
    uint16_t reserved;
    uint64_t pane_lo;
    uint64_t pane_hi;
    float x;
    float y;
    float width;
    float height;
} SeyalAppPaneRegion;

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
    /*
     * Optional ResourceAddress (SPEC-022 / N2). address_len == 0 means none.
     * Palette navigation rows set these; other row kinds leave them zero.
     * address_bytes holds up to 48 payload bytes (Pane = three UUIDs).
     */
    uint16_t address_version;
    uint16_t address_kind;
    uint16_t address_len;
    uint16_t address_pad;
    uint8_t address_bytes[48];
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
/** Overlay is projecting the navigation-only goto surface (N4). */
#define SEYAL_APP_PALETTE_GOTO 2u
/** Goto enumeration exceeded the bound; results are truncated (SPEC-022 R7.6). */
#define SEYAL_APP_PALETTE_TRUNCATED 4u

uint64_t seyal_app_create(void);
int32_t seyal_app_destroy(uint64_t handle);
uint8_t seyal_app_option_as_alt(uint64_t handle);
int32_t seyal_app_apply(uint64_t handle, const SeyalAppAction *action);
SeyalAppSnapshot seyal_app_snapshot(uint64_t handle);
SeyalAppComposer seyal_app_composer(uint64_t handle);
SeyalAppChrome seyal_app_chrome(uint64_t handle);
SeyalAppShell seyal_app_shell(uint64_t handle);
SeyalAppRow seyal_app_shell_row(uint64_t handle, uint16_t kind, uint32_t index);
SeyalAppWindow seyal_app_window(uint64_t handle, uint32_t index);
SeyalAppTab seyal_app_tab(uint64_t handle, uint32_t window_index, uint32_t tab_index);
SeyalAppPaneLeaf seyal_app_pane_leaf(
    uint64_t handle,
    uint32_t window_index,
    uint32_t tab_index,
    uint32_t pane_index);
SeyalAppPaneTreeNode seyal_app_tab_tree_node(
    uint64_t handle,
    uint32_t window_index,
    uint32_t tab_index,
    uint32_t node_index);
SeyalAppNativeEffect seyal_app_native_effect(uint64_t handle, uint32_t index);
/**
 * Fail-closed version/size check for W3 records.
 * Returns 0 when compatible; -1 unknown kind; -2 version; -3 size.
 */
int32_t seyal_app_record_compatible(uint16_t version, uint16_t size, uint16_t kind);
SeyalAppPaneRegion seyal_app_pane_region(uint64_t handle, uint32_t index);
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
uint64_t seyal_app_recovery_param(uint64_t handle);
SeyalAppAccessibility seyal_app_accessibility(uint64_t handle);
SeyalAppTheme seyal_app_theme(uint16_t appearance);

/*
 * Resolved visual snapshot from Rust cold TOML/theme authority (#993 / ADR-015).
 * `platform_appearance`: 0 = dark, 1 = light (host system appearance input).
 * Returned `appearance` is the Rust-resolved value after applying preference.
 * Font family pointers and warnings are borrowed until the next seyal_app_visual*.
 * flags: bit0 reduced material/transparency, bit1 full-default fallback,
 *        bit2 warnings present.
 * utility_material: 0 opaque, 1 tonal, 2 frosted.
 * preference: 0 system, 1 light, 2 dark.
 */
typedef struct SeyalAppVisual {
    uint16_t version;
    uint16_t size;
    uint16_t appearance;
    uint16_t preference;
    uint32_t canvas;
    uint32_t text;
    uint32_t accent;
    uint32_t container;
    double ui_font_size;
    double terminal_font_size;
    double window_padding;
    double terminal_padding;
    double utility_opacity;
    uint32_t flags;
    uint16_t utility_material;
    uint16_t warning_count;
    const uint8_t *ui_font_family;
    uint32_t ui_font_family_len;
    const uint8_t *terminal_font_family;
    uint32_t terminal_font_family_len;
} SeyalAppVisual;

typedef struct SeyalAppVisualWarning {
    const uint8_t *text;
    uint32_t text_len;
    uint32_t reserved;
} SeyalAppVisualWarning;

SeyalAppVisual seyal_app_visual(uint16_t platform_appearance);
SeyalAppVisualWarning seyal_app_visual_warning(uint32_t index);
/* Test/native harness only: reload cold UI config from path (len 0 = default). */
int32_t seyal_app_test_reload_ui_configuration(const uint8_t *path, size_t path_len);

int32_t seyal_app_last_error(uint64_t handle);

#ifdef __cplusplus
}
#endif

#endif
