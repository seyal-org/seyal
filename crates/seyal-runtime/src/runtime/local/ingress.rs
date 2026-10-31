use seyal_exec::ModeState;

use crate::{
    key_v2_encode::encode_terminal_key_v2,
    local_ipc::{
        attachment::AttachmentError,
        framing::{
            self, ComposerCommandRef, ComposerResult, ComposerResultCode, ErrorCode, HostSearch,
            HostSelectionAction, MessageType, TerminalKey as WireTerminalKey, TerminalKeyKind,
            TerminalKeyV2, TerminalKeyV2Modifiers, TerminalMouse, TerminalMouseKind,
            CAP_COMMAND_BLOCKS, CAP_EXTENDED_TERMINAL_KEY, MAX_INPUT_BYTES,
        },
    },
    RuntimeError,
};

use seyal_exec::{
    apply_host_mouse_gesture, encode_mouse_report, mouse_takes_host_override, CopyModeMotion,
    MouseEventKind, MouseReport, PasteError, VisualPos,
};

fn direct_input_error(error: &RuntimeError) -> ErrorCode {
    if matches!(error, RuntimeError::InputHeld) {
        ErrorCode::InvalidState
    } else {
        ErrorCode::Backpressure
    }
}

use super::super::shell_integration::ComposerAdmission;
use super::super::Runtime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CopyModeKeyAction {
    Exit,
    Yank,
    ToggleAnchor,
    Motion(CopyModeMotion),
    Swallow,
}

fn copy_mode_key_action(kind: TerminalKeyKind) -> CopyModeKeyAction {
    match kind {
        TerminalKeyKind::Escape => CopyModeKeyAction::Exit,
        TerminalKeyKind::Enter => CopyModeKeyAction::Yank,
        TerminalKeyKind::Tab => CopyModeKeyAction::ToggleAnchor,
        TerminalKeyKind::ArrowLeft => CopyModeKeyAction::Motion(CopyModeMotion::Left),
        TerminalKeyKind::ArrowRight => CopyModeKeyAction::Motion(CopyModeMotion::Right),
        TerminalKeyKind::ArrowUp => CopyModeKeyAction::Motion(CopyModeMotion::Up),
        TerminalKeyKind::ArrowDown => CopyModeKeyAction::Motion(CopyModeMotion::Down),
        TerminalKeyKind::Backspace | TerminalKeyKind::ControlAscii => CopyModeKeyAction::Swallow,
    }
}

fn encode_terminal_key(key: WireTerminalKey, modes: ModeState) -> Vec<u8> {
    match key.kind {
        TerminalKeyKind::Enter => vec![0x0d],
        TerminalKeyKind::Tab => vec![0x09],
        TerminalKeyKind::Backspace => vec![0x7f],
        TerminalKeyKind::Escape => vec![0x1b],
        TerminalKeyKind::ArrowUp => {
            if modes.application_cursor {
                b"\x1bOA".to_vec()
            } else {
                b"\x1b[A".to_vec()
            }
        }
        TerminalKeyKind::ArrowDown => {
            if modes.application_cursor {
                b"\x1bOB".to_vec()
            } else {
                b"\x1b[B".to_vec()
            }
        }
        TerminalKeyKind::ArrowRight => {
            if modes.application_cursor {
                b"\x1bOC".to_vec()
            } else {
                b"\x1b[C".to_vec()
            }
        }
        TerminalKeyKind::ArrowLeft => {
            if modes.application_cursor {
                b"\x1bOD".to_vec()
            } else {
                b"\x1b[D".to_vec()
            }
        }
        TerminalKeyKind::ControlAscii => {
            let scalar = key.scalar as u8;
            vec![match scalar {
                b' ' | b'@' => 0x00,
                b'A'..=b'Z' => scalar - b'@',
                b'[' => 0x1b,
                b'\\' => 0x1c,
                b']' => 0x1d,
                b'^' => 0x1e,
                b'_' => 0x1f,
                b'?' => 0x7f,
                _ => unreachable!("wire validation limits ControlAscii"),
            }]
        }
    }
}

impl Runtime {
    pub(super) fn fatal_terminal_key_v2(&mut self, token: u64) {
        self.send_error(
            token,
            ErrorCode::MalformedPayload,
            MessageType::TerminalKeyV2 as u16,
        );
        if let Some(state) = self.local_ipc.as_mut()
            && let Some(meta) = state.connections.get_mut(&token)
        {
            meta.close_after_flush = true;
        }
        let _ = self.sync_local_writable(token);
    }

    pub(super) fn handle_input(&mut self, token: u64, payload: &[u8]) {
        let Ok(input) = framing::InputRef::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::Input as u16,
            );
            return;
        };
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, input.attachment_id)
        }) {
            Some(Ok(id)) => id,
            Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_error(
                    token,
                    ErrorCode::PermissionDenied,
                    MessageType::Input as u16,
                );
                return;
            }
            _ => {
                self.send_error(token, ErrorCode::StaleIdentity, MessageType::Input as u16);
                return;
            }
        };
        match self.input_ingress(execution_id) {
            Ok(ingress) => {
                if let Err(error) = ingress.try_submit(input.bytes.to_vec()) {
                    self.send_error(token, direct_input_error(&error), MessageType::Input as u16);
                }
            }
            Err(_) => self.send_error(
                token,
                ErrorCode::InvalidExecution,
                MessageType::Input as u16,
            ),
        }
    }

    pub(super) fn handle_paste(&mut self, token: u64, payload: &[u8]) {
        let Ok(input) = framing::InputRef::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::Paste as u16,
            );
            return;
        };
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, input.attachment_id)
        }) {
            Some(Ok(id)) => id,
            Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_error(
                    token,
                    ErrorCode::PermissionDenied,
                    MessageType::Paste as u16,
                );
                return;
            }
            _ => {
                self.send_error(token, ErrorCode::StaleIdentity, MessageType::Paste as u16);
                return;
            }
        };
        let Some(entry) = self.entries.get(&execution_id) else {
            self.send_error(
                token,
                ErrorCode::InvalidExecution,
                MessageType::Paste as u16,
            );
            return;
        };
        let wrapped = match entry.execution.terminal().encode_host_paste(input.bytes) {
            Ok(bytes) => bytes,
            Err(PasteError::Empty) => {
                self.send_error(
                    token,
                    ErrorCode::MalformedPayload,
                    MessageType::Paste as u16,
                );
                return;
            }
            Err(PasteError::TooLarge) => {
                self.send_error(
                    token,
                    ErrorCode::CapacityExceeded,
                    MessageType::Paste as u16,
                );
                return;
            }
        };
        match self.input_ingress(execution_id) {
            Ok(ingress) => {
                if let Err(error) = ingress.try_submit(wrapped) {
                    self.send_error(token, direct_input_error(&error), MessageType::Paste as u16);
                }
            }
            Err(_) => self.send_error(
                token,
                ErrorCode::InvalidExecution,
                MessageType::Paste as u16,
            ),
        }
    }

    pub(super) fn handle_terminal_key(&mut self, token: u64, payload: &[u8]) {
        let Ok(key) = WireTerminalKey::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::TerminalKey as u16,
            );
            return;
        };
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, key.attachment_id)
        }) {
            Some(Ok(id)) => id,
            Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_error(
                    token,
                    ErrorCode::PermissionDenied,
                    MessageType::TerminalKey as u16,
                );
                return;
            }
            _ => {
                self.send_error(
                    token,
                    ErrorCode::StaleIdentity,
                    MessageType::TerminalKey as u16,
                );
                return;
            }
        };
        if self.intercept_copy_mode_key(token, execution_id, key.kind, key.attachment_id) {
            return;
        }
        let modes = self
            .entries
            .get(&execution_id)
            .map(|entry| entry.execution.terminal().modes());
        let Some(modes) = modes else {
            self.send_error(
                token,
                ErrorCode::InvalidExecution,
                MessageType::TerminalKey as u16,
            );
            return;
        };
        let bytes = encode_terminal_key(key, modes);
        match self.input_ingress(execution_id) {
            Ok(ingress) => {
                if let Err(error) = ingress.try_submit(bytes) {
                    self.send_error(
                        token,
                        direct_input_error(&error),
                        MessageType::TerminalKey as u16,
                    );
                }
            }
            Err(_) => self.send_error(
                token,
                ErrorCode::InvalidExecution,
                MessageType::TerminalKey as u16,
            ),
        }
    }

    pub(super) fn handle_host_selection(&mut self, token: u64, payload: &[u8]) {
        let Ok(command) = framing::HostSelection::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::HostSelection as u16,
            );
            return;
        };
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, command.attachment_id)
        }) {
            Some(Ok(id)) => id,
            Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_error(
                    token,
                    ErrorCode::PermissionDenied,
                    MessageType::HostSelection as u16,
                );
                return;
            }
            _ => {
                self.send_error(
                    token,
                    ErrorCode::StaleIdentity,
                    MessageType::HostSelection as u16,
                );
                return;
            }
        };
        let mut copied = None;
        let mut malformed_kind = false;
        let mut yank_failed = false;
        {
            let Some(entry) = self.entries.get_mut(&execution_id) else {
                self.send_error(
                    token,
                    ErrorCode::InvalidExecution,
                    MessageType::HostSelection as u16,
                );
                return;
            };
            match command.action {
                HostSelectionAction::EnterCopyMode => entry.execution.enter_copy_mode(),
                HostSelectionAction::ExitCopyMode => entry.execution.exit_copy_mode(),
                HostSelectionAction::ToggleAnchor => entry.execution.copy_mode_toggle_anchor(),
                HostSelectionAction::ToggleKind => entry.execution.copy_mode_toggle_kind(),
                HostSelectionAction::Clear => entry.execution.clear_selection(),
                HostSelectionAction::SetVisual => {
                    let start = seyal_exec::VisualPos {
                        col: command.start_col,
                        row: command.start_row,
                    };
                    let end = seyal_exec::VisualPos {
                        col: command.end_col,
                        row: command.end_row,
                    };
                    match command.kind {
                        0 => entry.execution.set_linear_selection(start, end),
                        1 => entry.execution.set_rectangular_selection(start, end),
                        _ => malformed_kind = true,
                    }
                }
                HostSelectionAction::Yank => match entry.execution.yank_selection() {
                    Ok(text) => copied = Some(text),
                    Err(_) => yank_failed = true,
                },
            }
        }
        if malformed_kind {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::HostSelection as u16,
            );
        }
        if yank_failed {
            self.send_error(
                token,
                ErrorCode::InvalidState,
                MessageType::HostSelection as u16,
            );
        }
        if let Some(text) = copied {
            self.emit_copied_text(token, command.attachment_id, &text);
        }
    }

    pub(super) fn handle_terminal_mouse(&mut self, token: u64, payload: &[u8]) {
        let Ok(event) = TerminalMouse::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::TerminalMouse as u16,
            );
            return;
        };
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, event.attachment_id)
        }) {
            Some(Ok(id)) => id,
            Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_error(
                    token,
                    ErrorCode::PermissionDenied,
                    MessageType::TerminalMouse as u16,
                );
                return;
            }
            _ => {
                self.send_error(
                    token,
                    ErrorCode::StaleIdentity,
                    MessageType::TerminalMouse as u16,
                );
                return;
            }
        };
        let monotonic = self.local_ipc.as_mut().is_some_and(|state| {
            let Some(meta) = state.connections.get_mut(&token) else {
                return false;
            };
            if event.action_id <= meta.last_terminal_mouse_action_id {
                false
            } else {
                meta.last_terminal_mouse_action_id = event.action_id;
                true
            }
        });
        if !monotonic {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::TerminalMouse as u16,
            );
            return;
        }
        let encoded = {
            let Some(entry) = self.entries.get_mut(&execution_id) else {
                self.send_error(
                    token,
                    ErrorCode::InvalidExecution,
                    MessageType::TerminalMouse as u16,
                );
                return;
            };
            let cols = entry.execution.terminal().cols();
            let rows = entry.execution.terminal().rows();
            if event.col >= cols || event.row >= rows {
                self.send_error(
                    token,
                    ErrorCode::MalformedPayload,
                    MessageType::TerminalMouse as u16,
                );
                return;
            }
            let cell = VisualPos {
                col: event.col,
                row: event.row,
            };
            let reporting = entry.execution.terminal().modes().mouse_reporting;
            let shift = event.modifiers.bits() & TerminalKeyV2Modifiers::SHIFT.bits() != 0;
            let kind = match event.kind {
                TerminalMouseKind::Press => MouseEventKind::Press,
                TerminalMouseKind::Release => MouseEventKind::Release,
                TerminalMouseKind::Move => MouseEventKind::Move,
                TerminalMouseKind::Wheel => MouseEventKind::Wheel,
            };
            if mouse_takes_host_override(reporting, shift, entry.mouse_host_anchor, kind) {
                if let Some((start, end)) =
                    apply_host_mouse_gesture(&mut entry.mouse_host_anchor, kind, cell)
                {
                    entry.execution.set_linear_selection(start, end);
                }
                return;
            }
            let report_button = if event.kind == TerminalMouseKind::Move && entry.mouse_buttons == 0
            {
                3
            } else {
                event.button
            };
            encode_mouse_report(
                MouseReport {
                    kind,
                    button: report_button,
                    shift: false,
                    alt: event.modifiers.bits() & TerminalKeyV2Modifiers::ALT.bits() != 0,
                    control: event.modifiers.bits() & TerminalKeyV2Modifiers::CONTROL.bits() != 0,
                    col: event.col,
                    row: event.row,
                },
                entry.execution.terminal().modes(),
            )
        };
        let Some(bytes) = encoded else {
            return;
        };
        match self.input_ingress(execution_id) {
            Ok(ingress) => {
                if let Err(error) = ingress.try_submit(bytes) {
                    self.send_error(
                        token,
                        direct_input_error(&error),
                        MessageType::TerminalMouse as u16,
                    );
                    return;
                }
            }
            Err(_) => {
                self.send_error(
                    token,
                    ErrorCode::InvalidExecution,
                    MessageType::TerminalMouse as u16,
                );
                return;
            }
        }
        if let Some(entry) = self.entries.get_mut(&execution_id) {
            match event.kind {
                TerminalMouseKind::Press if event.button <= 2 => {
                    entry.mouse_buttons |= 1 << event.button;
                }
                TerminalMouseKind::Release if event.button <= 2 => {
                    entry.mouse_buttons &= !(1 << event.button);
                }
                _ => {}
            }
        }
    }

    pub(super) fn handle_host_search(&mut self, token: u64, payload: &[u8]) {
        let Ok(search) = HostSearch::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::HostSearch as u16,
            );
            return;
        };
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, search.attachment_id)
        }) {
            Some(Ok(id)) => id,
            Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_error(
                    token,
                    ErrorCode::PermissionDenied,
                    MessageType::HostSearch as u16,
                );
                return;
            }
            _ => {
                self.send_error(
                    token,
                    ErrorCode::StaleIdentity,
                    MessageType::HostSearch as u16,
                );
                return;
            }
        };
        let missed = {
            let Some(entry) = self.entries.get_mut(&execution_id) else {
                self.send_error(
                    token,
                    ErrorCode::InvalidExecution,
                    MessageType::HostSearch as u16,
                );
                return;
            };
            entry
                .execution
                .search_and_select(search.needle, search.forward)
                .is_none()
                && !search.needle.is_empty()
        };
        if missed {
            self.send_error(
                token,
                ErrorCode::InvalidState,
                MessageType::HostSearch as u16,
            );
        }
    }

    fn emit_copied_text(&mut self, token: u64, attachment_id: crate::AttachmentId, text: &str) {
        if text.len() > MAX_INPUT_BYTES as usize {
            self.send_error(
                token,
                ErrorCode::CapacityExceeded,
                MessageType::CopiedText as u16,
            );
            return;
        }
        let payload = framing::InputRef {
            attachment_id,
            bytes: text.as_bytes(),
        }
        .encode();
        let _ = self.send_mandatory_frame(
            token,
            framing::encode_frame(MessageType::CopiedText, &payload),
        );
    }

    fn intercept_copy_mode_key(
        &mut self,
        token: u64,
        execution_id: crate::ExecutionId,
        kind: TerminalKeyKind,
        attachment_id: crate::AttachmentId,
    ) -> bool {
        let copied = {
            let Some(entry) = self.entries.get_mut(&execution_id) else {
                return false;
            };
            if !entry.execution.copy_mode_active() {
                return false;
            }
            match copy_mode_key_action(kind) {
                CopyModeKeyAction::Exit => {
                    entry.execution.exit_copy_mode();
                    None
                }
                CopyModeKeyAction::Yank => entry.execution.yank_selection().ok(),
                CopyModeKeyAction::ToggleAnchor => {
                    entry.execution.copy_mode_toggle_anchor();
                    None
                }
                CopyModeKeyAction::Motion(motion) => {
                    entry.execution.copy_mode_motion(motion);
                    None
                }
                CopyModeKeyAction::Swallow => None,
            }
        };
        if let Some(text) = copied {
            self.emit_copied_text(token, attachment_id, &text);
        }
        true
    }

    pub(super) fn handle_terminal_key_v2(&mut self, token: u64, payload: &[u8]) {
        let Ok(key) = TerminalKeyV2::decode(payload) else {
            self.fatal_terminal_key_v2(token);
            return;
        };
        let supports = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.connections.get(&token))
            .is_some_and(|meta| meta.client_capabilities & CAP_EXTENDED_TERMINAL_KEY != 0);
        if !supports {
            self.fatal_terminal_key_v2(token);
            return;
        }
        let monotonic = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.connections.get(&token))
            .is_some_and(|meta| key.action_id > meta.last_terminal_key_action_id);
        if !monotonic {
            self.fatal_terminal_key_v2(token);
            return;
        }
        if let Some(state) = self.local_ipc.as_mut()
            && let Some(meta) = state.connections.get_mut(&token)
        {
            meta.last_terminal_key_action_id = key.action_id;
        }
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, key.attachment_id)
        }) {
            Some(Ok(id)) => id,
            Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_error_detail(
                    token,
                    ErrorCode::PermissionDenied,
                    MessageType::TerminalKeyV2 as u16,
                    key.action_id,
                );
                return;
            }
            _ => {
                self.send_error_detail(
                    token,
                    ErrorCode::StaleIdentity,
                    MessageType::TerminalKeyV2 as u16,
                    key.action_id,
                );
                return;
            }
        };
        let Some(modes) = self
            .entries
            .get(&execution_id)
            .map(|entry| entry.execution.terminal().modes())
        else {
            self.send_error_detail(
                token,
                ErrorCode::InvalidExecution,
                MessageType::TerminalKeyV2 as u16,
                key.action_id,
            );
            return;
        };
        let Ok(bytes) = encode_terminal_key_v2(key, modes) else {
            self.send_error_detail(
                token,
                ErrorCode::MalformedPayload,
                MessageType::TerminalKeyV2 as u16,
                key.action_id,
            );
            return;
        };
        if !bytes.is_empty() {
            match self.input_ingress(execution_id) {
                Ok(ingress) => {
                    if let Err(error) = ingress.try_submit(bytes) {
                        self.send_error_detail(
                            token,
                            direct_input_error(&error),
                            MessageType::TerminalKeyV2 as u16,
                            key.action_id,
                        );
                    }
                }
                Err(_) => self.send_error_detail(
                    token,
                    ErrorCode::InvalidExecution,
                    MessageType::TerminalKeyV2 as u16,
                    key.action_id,
                ),
            }
        }
    }

    pub(super) fn handle_composer_command(&mut self, token: u64, payload: &[u8]) {
        let session_bootstrap = self.local_ipc.as_ref().is_some_and(|state| {
            state
                .connections
                .get(&token)
                .is_some_and(|meta| meta.client_capabilities & framing::CAP_SESSION_BOOTSTRAP != 0)
        });
        let (request, target_generation) = if session_bootstrap {
            let Ok((request, generation)) = ComposerCommandRef::decode_fenced(payload) else {
                self.send_error(
                    token,
                    ErrorCode::MalformedPayload,
                    MessageType::ComposerCommand as u16,
                );
                return;
            };
            (request, Some(generation))
        } else {
            let Ok(request) = ComposerCommandRef::decode(payload) else {
                self.send_error(
                    token,
                    ErrorCode::MalformedPayload,
                    MessageType::ComposerCommand as u16,
                );
                return;
            };
            (request, None)
        };
        let supports_blocks = self
            .local_ipc
            .as_ref()
            .and_then(|state| {
                state
                    .connections
                    .get(&token)
                    .map(|meta| meta.client_capabilities & CAP_COMMAND_BLOCKS != 0)
            })
            .unwrap_or(false);
        if !supports_blocks {
            self.send_error(
                token,
                ErrorCode::PermissionDenied,
                MessageType::ComposerCommand as u16,
            );
            return;
        }
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, request.attachment_id)
        }) {
            Some(Ok(id)) => id,
            Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_error(
                    token,
                    ErrorCode::PermissionDenied,
                    MessageType::ComposerCommand as u16,
                );
                return;
            }
            _ => {
                self.send_error(
                    token,
                    ErrorCode::StaleIdentity,
                    MessageType::ComposerCommand as u16,
                );
                return;
            }
        };
        let admission = if let Some(generation) = target_generation {
            self.submit_composer_command_fenced(
                execution_id,
                request.command.to_owned(),
                Some(generation),
            )
        } else {
            self.submit_composer_command(execution_id, request.command.to_owned())
        };
        match admission {
            Ok(ComposerAdmission::Accepted(block_id)) => {
                self.publish_block_timeline(execution_id);
                let result = ComposerResult {
                    attachment_id: request.attachment_id,
                    code: ComposerResultCode::Accepted,
                    block_id: block_id.raw(),
                    request_id: request.request_id,
                };
                let _ = self.send_mandatory_frame(
                    token,
                    framing::encode_frame(MessageType::ComposerResult, &result.encode()),
                );
            }
            Ok(ComposerAdmission::Unsupported) => {
                let result = ComposerResult {
                    attachment_id: request.attachment_id,
                    code: ComposerResultCode::Unsupported,
                    block_id: 0,
                    request_id: request.request_id,
                };
                let _ = self.send_mandatory_frame(
                    token,
                    framing::encode_frame(MessageType::ComposerResult, &result.encode()),
                );
            }
            Ok(ComposerAdmission::Invalid | ComposerAdmission::StaleTarget) => {
                let result = ComposerResult {
                    attachment_id: request.attachment_id,
                    code: ComposerResultCode::Invalid,
                    block_id: 0,
                    request_id: request.request_id,
                };
                let _ = self.send_mandatory_frame(
                    token,
                    framing::encode_frame(MessageType::ComposerResult, &result.encode()),
                );
            }
            Ok(ComposerAdmission::Relayed) => {
                let result = ComposerResult {
                    attachment_id: request.attachment_id,
                    code: ComposerResultCode::Accepted,
                    block_id: 0,
                    request_id: request.request_id,
                };
                let _ = self.send_mandatory_frame(
                    token,
                    framing::encode_frame(MessageType::ComposerResult, &result.encode()),
                );
            }
            Ok(ComposerAdmission::Busy) => {
                let result = ComposerResult {
                    attachment_id: request.attachment_id,
                    code: ComposerResultCode::Busy,
                    block_id: 0,
                    request_id: request.request_id,
                };
                let _ = self.send_mandatory_frame(
                    token,
                    framing::encode_frame(MessageType::ComposerResult, &result.encode()),
                );
            }
            Err(RuntimeError::InputBackpressure | RuntimeError::ControlQueueFull) => {
                let result = ComposerResult {
                    attachment_id: request.attachment_id,
                    code: ComposerResultCode::Backpressure,
                    block_id: 0,
                    request_id: request.request_id,
                };
                let _ = self.send_mandatory_frame(
                    token,
                    framing::encode_frame(MessageType::ComposerResult, &result.encode()),
                );
            }
            Err(RuntimeError::ExecutionNotRunning) => {
                let result = ComposerResult {
                    attachment_id: request.attachment_id,
                    code: ComposerResultCode::Invalid,
                    block_id: 0,
                    request_id: request.request_id,
                };
                let _ = self.send_mandatory_frame(
                    token,
                    framing::encode_frame(MessageType::ComposerResult, &result.encode()),
                );
            }
            Err(_) => {
                let result = ComposerResult {
                    attachment_id: request.attachment_id,
                    code: ComposerResultCode::Invalid,
                    block_id: 0,
                    request_id: request.request_id,
                };
                let _ = self.send_mandatory_frame(
                    token,
                    framing::encode_frame(MessageType::ComposerResult, &result.encode()),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_mode_keys_are_classified_without_pty_fallthrough() {
        assert_eq!(
            copy_mode_key_action(TerminalKeyKind::Escape),
            CopyModeKeyAction::Exit
        );
        assert_eq!(
            copy_mode_key_action(TerminalKeyKind::Enter),
            CopyModeKeyAction::Yank
        );
        assert_eq!(
            copy_mode_key_action(TerminalKeyKind::Tab),
            CopyModeKeyAction::ToggleAnchor
        );
        assert_eq!(
            copy_mode_key_action(TerminalKeyKind::ArrowLeft),
            CopyModeKeyAction::Motion(CopyModeMotion::Left)
        );
        assert_eq!(
            copy_mode_key_action(TerminalKeyKind::ArrowRight),
            CopyModeKeyAction::Motion(CopyModeMotion::Right)
        );
        assert_eq!(
            copy_mode_key_action(TerminalKeyKind::ArrowUp),
            CopyModeKeyAction::Motion(CopyModeMotion::Up)
        );
        assert_eq!(
            copy_mode_key_action(TerminalKeyKind::ArrowDown),
            CopyModeKeyAction::Motion(CopyModeMotion::Down)
        );
        assert_eq!(
            copy_mode_key_action(TerminalKeyKind::Backspace),
            CopyModeKeyAction::Swallow
        );
        assert_eq!(
            copy_mode_key_action(TerminalKeyKind::ControlAscii),
            CopyModeKeyAction::Swallow
        );
    }

    #[test]
    fn m001_arrows_follow_canonical_cursor_mode() {
        let key = WireTerminalKey {
            attachment_id: crate::AttachmentId::from_bytes([0; 16]),
            kind: TerminalKeyKind::ArrowUp,
            modifiers: seyal_protocol::framing::TerminalKeyModifiers::NONE,
            scalar: 0,
        };
        assert_eq!(encode_terminal_key(key, ModeState::default()), b"\x1b[A");
        let modes = ModeState {
            application_cursor: true,
            ..ModeState::default()
        };
        assert_eq!(encode_terminal_key(key, modes), b"\x1bOA");
    }
}
