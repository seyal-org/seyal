//! ApplicationRoot action dispatch. Kept beside the façade so PT5 pane-tree
//! verbs can land without growing `mod.rs` past the cohesion ratchet.

use super::*;
use crate::chrome::ChromeAction;
use crate::composer::ComposerAction;

impl ApplicationRoot {
    pub fn apply(&mut self, action: AppAction) -> Result<(), AppError> {
        if self.frozen
            && !matches!(
                action,
                AppAction::AckEffect
                    | AppAction::AckRecoveryEffect
                    | AppAction::CancelRecovery
                    | AppAction::DisconnectReconstruction
                    | AppAction::Quit
            )
        {
            return self.fail(AppError::Frozen);
        }
        let result = match action {
            AppAction::Focus { fence } => self.focus(fence),
            AppAction::Bind { fence, evidence } => self.bind(fence, evidence),
            AppAction::Refresh {
                fence,
                alternate_screen,
            } => self.refresh(fence, alternate_screen),
            AppAction::SubmitInput { fence, text } => self.submit_input(fence, &text),
            AppAction::Quit => self.quit(),
            AppAction::AckEffect => self.ack_effect(),
            AppAction::BeginRecovery { now } => self.begin_recovery(now),
            AppAction::CompleteRecovery {
                generation,
                outcome,
                now,
                launch,
            } => self.complete_recovery(generation, outcome, now, launch),
            AppAction::FireScheduledRecovery { generation, now } => {
                self.fire_scheduled_recovery(generation, now)
            }
            AppAction::AckRecoveryEffect => self.ack_recovery_effect(),
            AppAction::CancelRecovery => self.cancel_recovery(),
            AppAction::AdvanceRecoveryStage { stage } => self.advance_recovery_stage(stage),
            AppAction::BeginReconstructionAttempt => self.begin_reconstruction_attempt(),
            AppAction::CommitReconstruction {
                runtime,
                execution,
                attachment,
                controller_authority_committed,
                authoritative_snapshot_committed,
            } => self.commit_reconstruction(
                runtime,
                execution,
                attachment,
                controller_authority_committed,
                authoritative_snapshot_committed,
            ),
            AppAction::DisconnectReconstruction => self.disconnect_reconstruction(),
            AppAction::SetComposerDraft {
                fence,
                text,
                composer_epoch,
            } => self.set_composer_draft(fence, text, composer_epoch),
            AppAction::SubmitComposer {
                fence,
                composer_epoch,
            } => self.submit_composer(fence, composer_epoch),
            AppAction::ApplyComposerResult {
                fence,
                request_id,
                accepted,
            } => self.apply_composer_result(fence, request_id, accepted),
            AppAction::ApplyRuntimeBlocks { fence, records } => {
                self.apply_runtime_blocks(fence, records)
            }
            AppAction::ApplyRuntimeComposerStatus {
                fence,
                eligibility,
                revision,
            } => self.apply_runtime_composer_status(fence, eligibility, revision),
            AppAction::SelectRestingPresentation { fence, raw } => {
                self.select_resting_presentation(fence, raw)
            }
            AppAction::OpenComposerHistory { fence } => {
                self.composer_history(fence, ComposerAction::OpenHistory { pane: fence.pane })
            }
            AppAction::SetComposerHistoryFilter { fence, query } => self.composer_history(
                fence,
                ComposerAction::SetHistoryFilter {
                    pane: fence.pane,
                    query,
                },
            ),
            AppAction::MoveComposerHistorySelection { fence, delta } => self.composer_history(
                fence,
                ComposerAction::MoveHistorySelection {
                    pane: fence.pane,
                    delta,
                },
            ),
            AppAction::SelectComposerHistory {
                fence,
                composer_epoch,
            } => self.composer_history(
                fence,
                ComposerAction::SelectHistory {
                    pane: fence.pane,
                    epoch: composer_epoch,
                },
            ),
            AppAction::CloseComposerHistory { fence } => {
                self.composer_history(fence, ComposerAction::CloseHistory { pane: fence.pane })
            }
            AppAction::SetLeftPanel { mode } => self.set_left_panel(mode),
            AppAction::SetInspectorMode { mode } => self.set_inspector_mode(mode),
            AppAction::SelectAgent { fence, id } => self.select_agent(fence, id),
            AppAction::OpenAttention { fence, id } => self.open_attention(fence, id),
            AppAction::ReplaceChrome {
                fence,
                agents,
                attention,
            } => self.replace_chrome(fence, agents, attention),
            AppAction::SelectBlock { fence, id } => self.select_block(fence, id),
            AppAction::ClearBlockSelection { fence } => {
                self.require_fence(fence)?;
                let shell = self.shell.snapshot();
                self.chrome
                    .apply(ChromeAction::ClearBlockSelection, &shell)
                    .map(|_| ())
                    .map_err(chrome_error)
            }
            AppAction::SelectWorkspace { id } => self.select_workspace(id),
            AppAction::SelectTab { id } => self.select_tab(id),
            AppAction::CreateTab => {
                // R6.4.1: menu/key-equivalent New Tab cannot bypass the palette modal.
                if self.palette.is_open() {
                    self.require_workspace_command_for_menu(
                        crate::keybinding::WorkspaceCommandId::TabCreate,
                    )?;
                }
                self.create_tab()
            }
            AppAction::CloseTab { .. }
            | AppAction::CloseWindow { .. }
            | AppAction::ClosePane { .. } => self.apply_close(action),
            AppAction::TerminateExecution { fence } => self.terminate_execution(fence),
            AppAction::Adopt { fence, evidence } => self.adopt(fence, evidence),
            #[cfg(test)]
            AppAction::SyncLiveUnpresented { entries } => self.sync_live_unpresented(entries),
            #[cfg(test)]
            AppAction::RecordUnpresented {
                execution,
                workspace,
            } => self.record_unpresented(execution, workspace),
            AppAction::TerminateUnpresented { execution } => self.terminate_unpresented(execution),
            AppAction::SplitFocused { axis } => self.split_focused(axis),
            AppAction::FocusPane { id } => self.focus_pane(id),
            AppAction::ZoomPane { id } => self.zoom_pane(id),
            AppAction::Unzoom => self.unzoom(),
            AppAction::SwapPanes { a, b } => self.swap_panes(a, b),
            AppAction::MovePaneBeside {
                pane,
                neighbor,
                side,
            } => self.move_pane_beside(pane, neighbor, side),
            AppAction::FocusDirection { direction } => self.focus_direction(direction),
            AppAction::EqualizeFocused => self.equalize_focused(),
            AppAction::EqualizeTab => self.equalize_tab(),
            AppAction::MoveSplitDivider { pane, position } => {
                self.move_split_divider(pane, position)
            }
            AppAction::SetShellVisibility {
                left,
                inspector,
                tab_strip,
            } => self.set_shell_visibility(left, inspector, tab_strip),
            AppAction::OpenPalette { fence } => {
                if self.palette.is_open() {
                    self.require_workspace_command_for_menu(
                        crate::keybinding::WorkspaceCommandId::CommandPaletteOpen,
                    )?;
                }
                self.open_palette(fence)
            }
            AppAction::SetPaletteQuery { fence, query } => self.set_palette_query(fence, query),
            AppAction::MovePaletteSelection { fence, delta } => {
                self.move_palette_selection(fence, delta)
            }
            AppAction::RunPalette { fence, address } => self.run_palette(fence, address),
            AppAction::Navigate { fence, address } => {
                self.require_fence(fence)?;
                self.navigate_address(address)
            }
            AppAction::HistoryBack { fence, observed } => {
                self.require_fence(fence)?;
                self.history_back(observed)
            }
            AppAction::HistoryForward { fence, observed } => {
                self.require_fence(fence)?;
                self.history_forward(observed)
            }
            AppAction::ClosePalette { fence } => self.close_palette(fence),
            AppAction::OpenGoto { fence, scope } => self.open_goto(fence, scope),
            AppAction::SetGotoScope { fence, scope } => self.set_goto_scope(fence, scope),
            AppAction::CycleGotoScope { fence } => self.cycle_goto_scope(fence),
            AppAction::SetGotoQuery { fence, query } => self.set_goto_query(fence, query),
            AppAction::MoveGotoSelection { fence, delta } => self.move_goto_selection(fence, delta),
            AppAction::RunGoto { fence, address } => self.run_goto(fence, address),
            AppAction::CloseGoto { fence } => self.close_goto(fence),
            AppAction::SelectWindow { id } => self.select_window(id),
            AppAction::CycleWindow { direction } => self.cycle_window(direction),
            AppAction::CreateWindow => self.create_window(),
            AppAction::ReportWindowEvent {
                window,
                event,
                occluded,
            } => self.report_window_event(window, event, occluded),
        };
        match result {
            Ok(()) => {
                self.last_error = None;
                self.snapshot_generation = self.snapshot_generation.saturating_add(1);
                #[cfg(target_os = "macos")]
                if !self.sync_attachment_delivery() {
                    self.last_error = Some(AppError::DeliveryControlFailed);
                }
                Ok(())
            }
            Err(error) => self.fail(error),
        }
    }
}
