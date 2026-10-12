use super::AppError;
use crate::chrome::ChromeError;
use crate::composer::ComposerError;
use crate::palette::PaletteError;
use crate::shell::ShellError;

pub(super) fn chrome_error(error: ChromeError) -> AppError {
    match error {
        ChromeError::UnknownAgent => AppError::UnknownAgent,
        ChromeError::UnknownAttention => AppError::UnknownAttention,
        ChromeError::UnknownWorkspace => AppError::UnknownChromeWorkspace,
        ChromeError::UnknownTab => AppError::UnknownChromeTab,
        ChromeError::UnknownBlock => AppError::UnknownBlock,
    }
}

pub(super) fn close_tab_error(error: ShellError) -> AppError {
    match error {
        ShellError::CannotCloseLastTab => AppError::CannotCloseLastTab,
        _ => AppError::UnknownChromeTab,
    }
}

pub(super) fn close_pane_error(error: ShellError) -> AppError {
    match error {
        ShellError::CannotCloseLastPane => AppError::CannotCloseLastPane,
        ShellError::CannotCloseBoundPane => AppError::CannotCloseBoundPane,
        _ => AppError::UnknownPane,
    }
}

pub(super) fn palette_error(error: PaletteError) -> AppError {
    match error {
        PaletteError::NotOpen => AppError::PaletteNotOpen,
        PaletteError::NoSelection => AppError::PaletteNoSelection,
    }
}

pub(super) fn composer_error(error: ComposerError) -> AppError {
    match error {
        ComposerError::UnknownPane => AppError::UnknownPane,
        ComposerError::EmptyDraft | ComposerError::SubmitDisabled => {
            AppError::ComposerSubmitDisabled
        }
        ComposerError::StaleRequest => AppError::StaleComposerRequest,
        ComposerError::StaleEpoch => AppError::StaleComposerEpoch,
        ComposerError::HistoryUnavailable => AppError::ComposerHistoryUnavailable,
        ComposerError::HistoryClosed => AppError::ComposerHistoryClosed,
        ComposerError::HistoryNoSelection => AppError::ComposerHistoryNoSelection,
    }
}
