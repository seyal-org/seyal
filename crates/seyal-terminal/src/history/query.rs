//! Anchor resolution, source-unit views, search, and selection.

use super::store::HistoryStore;
use super::types::{
    HistoryAnchor, HistoryAnchorResolution, HistoryBreakAfter, HistoryMatch, HistoryRangeError,
    HistoryUnitView, HISTORY_SELECTION_UNIT_CAP,
};
use crate::LineId;
use unicode_segmentation::UnicodeSegmentation;

impl HistoryStore {
    pub(crate) fn resolve_anchor(&self, anchor: HistoryAnchor) -> HistoryAnchorResolution {
        let line = self.entries().find(|line| {
            line.line_id() == anchor.line_id
                && anchor.unit_offset >= line.start_offset()
                && anchor.unit_offset.saturating_sub(line.start_offset())
                    < line.units().count() as u32
        });
        let Some(line) = line else {
            return if self.line_was_evicted(anchor.line_id) {
                HistoryAnchorResolution::Unavailable
            } else {
                HistoryAnchorResolution::Invalid
            };
        };
        let Some(relative) = anchor.unit_offset.checked_sub(line.start_offset()) else {
            return HistoryAnchorResolution::Invalid;
        };
        let Ok(relative) = usize::try_from(relative) else {
            return HistoryAnchorResolution::Invalid;
        };
        let Some(unit) = line.units().nth(relative) else {
            return HistoryAnchorResolution::Invalid;
        };
        let Ok(text) = String::from_utf8(unit.utf8().to_vec()) else {
            return HistoryAnchorResolution::Unavailable;
        };
        HistoryAnchorResolution::Resolved {
            text,
            width: unit.width(),
            style: unit.style(),
        }
    }

    pub(crate) fn source_units(
        &self,
        start: LineId,
        end: LineId,
        max_units: usize,
    ) -> Vec<HistoryUnitView> {
        if max_units == 0 || end < start {
            return Vec::new();
        }
        let mut units = Vec::new();
        for line in self.entries() {
            if line.line_id() < start {
                continue;
            }
            if line.line_id() > end {
                break;
            }
            for (index, unit) in line.units().enumerate() {
                units.push(HistoryUnitView {
                    anchor: HistoryAnchor {
                        line_id: line.line_id(),
                        unit_offset: line.start_offset().saturating_add(index as u32),
                    },
                    text: String::from_utf8_lossy(unit.utf8()).into_owned(),
                    width: unit.width(),
                    style: unit.style(),
                    break_after: line.break_after(),
                });
                if units.len() >= max_units {
                    return units;
                }
            }
        }
        units
    }

    pub(crate) fn search(&self, needle: &str, max_matches: usize) -> Vec<HistoryMatch> {
        if needle.is_empty() || max_matches == 0 {
            return Vec::new();
        }
        let needle_units = needle.graphemes(true).count();
        let mut window: Vec<HistoryUnitView> = Vec::with_capacity(needle_units.max(1));
        let mut matches = Vec::new();
        for line in self.entries() {
            for (index, unit) in line.units().enumerate() {
                window.push(HistoryUnitView {
                    anchor: HistoryAnchor {
                        line_id: line.line_id(),
                        unit_offset: line.start_offset().saturating_add(index as u32),
                    },
                    text: String::from_utf8_lossy(unit.utf8()).into_owned(),
                    width: unit.width(),
                    style: unit.style(),
                    break_after: line.break_after(),
                });
                while window.len() > needle_units {
                    window.remove(0);
                }
                let text = window
                    .iter()
                    .map(|unit| unit.text.as_str())
                    .collect::<String>();
                if text == needle {
                    matches.push(HistoryMatch {
                        start: window[0].anchor,
                        end: window[window.len() - 1].anchor,
                    });
                    if matches.len() >= max_matches {
                        return matches;
                    }
                }
            }
            if line.break_after() == HistoryBreakAfter::HardBreak {
                window.clear();
            }
        }
        matches
    }

    pub(crate) fn selection(
        &self,
        start: HistoryAnchor,
        end: HistoryAnchor,
    ) -> Result<Vec<HistoryUnitView>, HistoryRangeError> {
        if start > end {
            return Err(HistoryRangeError::Unrepresentable);
        }
        if self.range_intersects_evicted(start.line_id, end.line_id) {
            return Err(HistoryRangeError::Stale);
        }
        let mut selected = Vec::new();
        for line in self.entries() {
            if line.line_id() < start.line_id {
                continue;
            }
            if line.line_id() > end.line_id {
                break;
            }
            for (index, unit) in line.units().enumerate() {
                let view = HistoryUnitView {
                    anchor: HistoryAnchor {
                        line_id: line.line_id(),
                        unit_offset: line.start_offset().saturating_add(index as u32),
                    },
                    text: String::from_utf8_lossy(unit.utf8()).into_owned(),
                    width: unit.width(),
                    style: unit.style(),
                    break_after: line.break_after(),
                };
                if view.anchor >= start && view.anchor <= end {
                    if selected.len() >= HISTORY_SELECTION_UNIT_CAP {
                        return Err(HistoryRangeError::Unrepresentable);
                    }
                    selected.push(view);
                }
            }
        }
        if selected.is_empty() {
            return Err(HistoryRangeError::Stale);
        }
        Ok(selected)
    }

}
