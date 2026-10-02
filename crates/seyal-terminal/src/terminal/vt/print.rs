//! Grapheme print / append path for TerminalCore.

use super::TerminalCore;
use crate::{
    active_grapheme::{
        append_payload, build_lead_cell, edge_decision_late_widen, edge_decision_new_unit,
        try_append_scalar, ActiveGrapheme, EdgeDecision,
    },
    damage::Mutation,
    width::{grapheme_terminal_width, AmbiguousWidthPolicy},
    TerminalError,
};

impl TerminalCore {
    pub(super) fn print_current(&mut self, character: char) -> Result<Mutation, TerminalError> {
        self.print_scalar(character)
    }

    fn print_scalar(&mut self, character: char) -> Result<Mutation, TerminalError> {
        let unicode_core = self.modes.unicode_core;
        let wraparound = self.modes.wraparound;
        let ambiguous = self.ambiguous_width;
        let style = self.current().pen();

        // Append to active grapheme when eligible.
        if let Some(active) = self.active_grapheme.as_ref() {
            if try_append_scalar(active, character, unicode_core)
                || (!unicode_core
                    && grapheme_terminal_width(&character.to_string(), ambiguous) == 0)
            {
                return self.append_to_active(character, wraparound, ambiguous);
            }
            // Boundary: active unit stays committed; start a new one.
            self.active_grapheme = None;
        }

        // Legacy combining onto previous cell without active anchor.
        if !unicode_core {
            let width = grapheme_terminal_width(&character.to_string(), ambiguous);
            if width == 0 {
                return Ok(Mutation::none());
            }
        }

        let mut text = String::new();
        text.push(character);
        let width = if unicode_core {
            grapheme_terminal_width(&text, ambiguous)
        } else {
            grapheme_terminal_width(&text, ambiguous).max(1)
        };

        if width == 0 {
            // Isolated combining in Unicode-core with no active base: ignore.
            return Ok(Mutation::none());
        }

        let cursor = self.current().cursor(true);
        match edge_decision_new_unit(cursor.col, self.current().cols(), width, wraparound) {
            EdgeDecision::IgnoreUnit => {
                // SPEC-011 §8.4: ignore atomically; leave active unset.
                return Ok(Mutation::none());
            }
            EdgeDecision::RejectExtension | EdgeDecision::Place => {}
        }

        let lead = build_lead_cell(&text, width, style, false, &mut self.grapheme_store, None);
        let (mutation, soft_wrapped, lead_col, lead_row) = {
            let line_ids = &mut self.line_ids;
            let store = &mut self.grapheme_store;
            let screen = if self.modes.alternate_screen {
                self.alternate.as_mut().unwrap_or(&mut self.primary)
            } else {
                &mut self.primary
            };
            screen.place_new_unit(lead, wraparound, line_ids, store)?
        };
        let _ = soft_wrapped;

        self.active_grapheme = Some(ActiveGrapheme {
            col: lead_col,
            row: lead_row,
            utf8: text,
            width,
            style,
            store_id: self
                .current()
                .cell(lead_col, lead_row)
                .map(|c| c.store_id)
                .unwrap_or(crate::grapheme_store::INLINE_STORE_ID),
            overflow: false,
        });
        Ok(mutation)
    }

    fn append_to_active(
        &mut self,
        character: char,
        wraparound: bool,
        ambiguous: AmbiguousWidthPolicy,
    ) -> Result<Mutation, TerminalError> {
        let Some(mut active) = self.active_grapheme.take() else {
            return Ok(Mutation::none());
        };
        let previous_width = active.width;
        let result = append_payload(&mut active, character, &mut self.grapheme_store, ambiguous);

        if result.width_changed && result.width > previous_width {
            match edge_decision_late_widen(
                active.col,
                self.current().cols(),
                result.width,
                wraparound,
            ) {
                EdgeDecision::RejectExtension => {
                    // SPEC-011 §8.4: reject only the width-changing extension.
                    active.utf8.pop();
                    self.active_grapheme = Some(active);
                    return Ok(Mutation::none());
                }
                EdgeDecision::IgnoreUnit => {
                    active.utf8.pop();
                    self.active_grapheme = Some(active);
                    return Ok(Mutation::none());
                }
                EdgeDecision::Place => {
                    if active.col + 1 >= self.current().cols() && wraparound {
                        // Late widen that must soft-wrap: clear old, re-place on next row.
                        let style = active.style;
                        let text = active.utf8.clone();
                        let overflow = active.overflow;
                        let old_store = active.store_id;
                        let clear_mut = {
                            let store = &mut self.grapheme_store;
                            let screen = if self.modes.alternate_screen {
                                self.alternate.as_mut().unwrap_or(&mut self.primary)
                            } else {
                                &mut self.primary
                            };
                            screen.clear_unit_at(active.col, active.row, store)
                        };
                        // Soft-wrap lineage: mark previous row wrap by setting pending and LF.
                        {
                            let screen = if self.modes.alternate_screen {
                                self.alternate.as_mut().unwrap_or(&mut self.primary)
                            } else {
                                &mut self.primary
                            };
                            screen.set_pending_wrap(true);
                        }
                        let lead = build_lead_cell(
                            &text,
                            result.width,
                            style,
                            overflow,
                            &mut self.grapheme_store,
                            Some(old_store),
                        );
                        let (place_mut, _, lead_col, lead_row) = {
                            let line_ids = &mut self.line_ids;
                            let store = &mut self.grapheme_store;
                            let screen = if self.modes.alternate_screen {
                                self.alternate.as_mut().unwrap_or(&mut self.primary)
                            } else {
                                &mut self.primary
                            };
                            screen.place_new_unit(lead, wraparound, line_ids, store)?
                        };
                        active.col = lead_col;
                        active.row = lead_row;
                        active.width = result.width;
                        active.store_id = self
                            .current()
                            .cell(lead_col, lead_row)
                            .map(|c| c.store_id)
                            .unwrap_or(crate::grapheme_store::INLINE_STORE_ID);
                        self.active_grapheme = Some(active);
                        return Ok(clear_mut.merge(place_mut));
                    }
                }
            }
        }

        active.width = result.width;
        let release = if active.store_id != crate::grapheme_store::INLINE_STORE_ID {
            Some(active.store_id)
        } else {
            None
        };
        let lead = build_lead_cell(
            &active.utf8,
            active.width.max(1),
            active.style,
            active.overflow,
            &mut self.grapheme_store,
            release,
        );
        active.store_id = lead.store_id;
        active.overflow = lead.overflow;
        let mutation = {
            let store = &mut self.grapheme_store;
            let screen = if self.modes.alternate_screen {
                self.alternate.as_mut().unwrap_or(&mut self.primary)
            } else {
                &mut self.primary
            };
            screen.replace_active_lead(active.col, active.row, lead, previous_width, store)
        };
        // Adjust cursor after late widen occupying an extra cell.
        if result.width > previous_width && result.width >= 2 {
            let screen = if self.modes.alternate_screen {
                self.alternate.as_mut().unwrap_or(&mut self.primary)
            } else {
                &mut self.primary
            };
            let cursor = screen.cursor(true);
            if !screen.pending_wrap() && cursor.col == active.col + 1 {
                // Was width-1 with cursor after lead; now need cursor after continuation.
                let _ = screen; // cursor advance handled below
            }
        }
        if result.width > previous_width {
            let cols = self.current().cols();
            let screen = if self.modes.alternate_screen {
                self.alternate.as_mut().unwrap_or(&mut self.primary)
            } else {
                &mut self.primary
            };
            let next = active.col.saturating_add(u16::from(result.width));
            if next >= cols {
                // Move cursor to last col with pending wrap.
                let _ = screen.set_col(cols.saturating_sub(1));
                screen.set_pending_wrap(true);
            } else {
                let _ = screen.set_col(next);
            }
        }
        self.active_grapheme = Some(active);
        Ok(mutation)
    }
}
