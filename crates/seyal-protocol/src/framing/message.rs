//! SPEC-004 message-type tags, borrowed message enum, and payload dispatch.

use super::envelope::{FrameHeader, FramingError, HEADER_LEN, MAX_FRAME_PAYLOAD};
use super::payload::{
    Attach, Attached, ClientHello, Detach, Detached, ErrorMessage, ExecutionList, HostSearch,
    HostSelection, InputRef, LifecycleMessage, Resize, Resync, ServerHello, TerminalMouse,
};
use super::{
    BlockTimeline, ComposerCommandRef, ComposerResult, ComposerStatus, HistoryRangeRequest,
    HistoryRangeSnapshot, ResizeRequest, ResizeResult, TerminalKey, TerminalKeyV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    ClientHello = 1,
    ServerHello = 2,
    ListExecutions = 3,
    ExecutionList = 4,
    Attach = 5,
    Attached = 6,
    Detach = 7,
    Detached = 8,
    Input = 9,
    Resize = 10,
    Resync = 11,
    DisplaySnapshot = 12,
    DisplayDelta = 13,
    Lifecycle = 14,
    Error = 15,
    Goodbye = 16,
    TerminalKey = 17,
    ResizeRequest = 18,
    ResizeResult = 19,
    ComposerCommand = 20,
    BlockTimeline = 21,
    ComposerResult = 22,
    ComposerStatus = 23,
    HistoryRangeRequest = 24,
    HistoryRangeSnapshot = 25,
    DisplaySnapshotV2 = 27,
    DisplayDeltaV2 = 28,
    TerminalKeyV2 = 29,
    /// Host clipboard paste. Same payload layout as `Input`; Runtime wraps
    /// the bytes using canonical bracketed-paste mode before PTY admission.
    /// Type 26 is Pass 8 block-state metadata (not a control MessageType).
    Paste = 30,
    /// Host selection/copy-mode commands. Never written to the PTY.
    HostSelection = 31,
    /// Runtime→client yanked/copied UTF-8. Same payload layout as `Input`.
    CopiedText = 32,
    /// Host history search. Never written to the PTY.
    HostSearch = 33,
    /// Native mouse event. Runtime encodes SGR/X10 from canonical modes.
    TerminalMouse = 34,
}
impl MessageType {
    pub fn from_u16(value: u16) -> Option<Self> {
        Some(match value {
            1 => Self::ClientHello,
            2 => Self::ServerHello,
            3 => Self::ListExecutions,
            4 => Self::ExecutionList,
            5 => Self::Attach,
            6 => Self::Attached,
            7 => Self::Detach,
            8 => Self::Detached,
            9 => Self::Input,
            10 => Self::Resize,
            11 => Self::Resync,
            12 => Self::DisplaySnapshot,
            13 => Self::DisplayDelta,
            14 => Self::Lifecycle,
            15 => Self::Error,
            16 => Self::Goodbye,
            17 => Self::TerminalKey,
            18 => Self::ResizeRequest,
            19 => Self::ResizeResult,
            20 => Self::ComposerCommand,
            21 => Self::BlockTimeline,
            22 => Self::ComposerResult,
            23 => Self::ComposerStatus,
            24 => Self::HistoryRangeRequest,
            25 => Self::HistoryRangeSnapshot,
            27 => Self::DisplaySnapshotV2,
            28 => Self::DisplayDeltaV2,
            29 => Self::TerminalKeyV2,
            30 => Self::Paste,
            31 => Self::HostSelection,
            32 => Self::CopiedText,
            33 => Self::HostSearch,
            34 => Self::TerminalMouse,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message<'a> {
    ClientHello(ClientHello),
    ServerHello(ServerHello),
    ListExecutions,
    ExecutionList(ExecutionList),
    Attach(Attach),
    Attached(Attached),
    Detach(Detach),
    Detached(Detached),
    Input(InputRef<'a>),
    Resize(Resize),
    Resync(Resync),
    DisplaySnapshot(&'a [u8]),
    DisplayDelta(&'a [u8]),
    DisplaySnapshotV2(&'a [u8]),
    DisplayDeltaV2(&'a [u8]),
    Lifecycle(LifecycleMessage),
    Error(ErrorMessage),
    Goodbye,
    TerminalKey(TerminalKey),
    ResizeRequest(ResizeRequest),
    ResizeResult(ResizeResult),
    ComposerCommand(ComposerCommandRef<'a>),
    BlockTimeline(BlockTimeline),
    ComposerResult(ComposerResult),
    ComposerStatus(ComposerStatus),
    HistoryRangeRequest(HistoryRangeRequest),
    HistoryRangeSnapshot(HistoryRangeSnapshot),
    TerminalKeyV2(TerminalKeyV2),
    Paste(InputRef<'a>),
    HostSelection(HostSelection),
    CopiedText(InputRef<'a>),
    HostSearch(HostSearch<'a>),
    TerminalMouse(TerminalMouse),
}

pub fn decode_message<'a>(
    header: &FrameHeader,
    payload: &'a [u8],
) -> Result<Message<'a>, FramingError> {
    if payload.len() != header.payload_len as usize {
        return Err(FramingError::ExactLengthMismatch);
    }
    let kind =
        MessageType::from_u16(header.message_type).ok_or(FramingError::UnknownMessageType)?;
    Ok(match kind {
        MessageType::ClientHello => Message::ClientHello(ClientHello::decode(payload)?),
        MessageType::ServerHello => Message::ServerHello(ServerHello::decode(payload)?),
        MessageType::ListExecutions => {
            if !payload.is_empty() {
                return Err(FramingError::ExactLengthMismatch);
            }
            Message::ListExecutions
        }
        MessageType::ExecutionList => Message::ExecutionList(ExecutionList::decode(payload)?),
        MessageType::Attach => Message::Attach(Attach::decode(payload)?),
        MessageType::Attached => Message::Attached(Attached::decode(payload)?),
        MessageType::Detach => Message::Detach(Detach::decode(payload)?),
        MessageType::Detached => Message::Detached(Detached::decode(payload)?),
        MessageType::Input => Message::Input(InputRef::decode(payload)?),
        MessageType::Resize => Message::Resize(Resize::decode(payload)?),
        MessageType::Resync => Message::Resync(Resync::decode(payload)?),
        MessageType::DisplaySnapshot => Message::DisplaySnapshot(payload),
        MessageType::DisplayDelta => Message::DisplayDelta(payload),
        MessageType::DisplaySnapshotV2 => Message::DisplaySnapshotV2(payload),
        MessageType::DisplayDeltaV2 => Message::DisplayDeltaV2(payload),
        MessageType::Lifecycle => Message::Lifecycle(LifecycleMessage::decode(payload)?),
        MessageType::Error => Message::Error(ErrorMessage::decode(payload)?),
        MessageType::Goodbye => {
            if !payload.is_empty() {
                return Err(FramingError::ExactLengthMismatch);
            }
            Message::Goodbye
        }
        MessageType::TerminalKey => Message::TerminalKey(TerminalKey::decode(payload)?),
        MessageType::ResizeRequest => Message::ResizeRequest(ResizeRequest::decode(payload)?),
        MessageType::ResizeResult => Message::ResizeResult(ResizeResult::decode(payload)?),
        MessageType::ComposerCommand => {
            Message::ComposerCommand(ComposerCommandRef::decode(payload)?)
        }
        MessageType::BlockTimeline => Message::BlockTimeline(BlockTimeline::decode(payload)?),
        MessageType::ComposerResult => Message::ComposerResult(ComposerResult::decode(payload)?),
        MessageType::ComposerStatus => Message::ComposerStatus(ComposerStatus::decode(payload)?),
        MessageType::HistoryRangeRequest => {
            Message::HistoryRangeRequest(HistoryRangeRequest::decode(payload)?)
        }
        MessageType::HistoryRangeSnapshot => {
            Message::HistoryRangeSnapshot(HistoryRangeSnapshot::decode(payload)?)
        }
        MessageType::TerminalKeyV2 => Message::TerminalKeyV2(TerminalKeyV2::decode(payload)?),
        MessageType::Paste => Message::Paste(InputRef::decode(payload)?),
        MessageType::HostSelection => Message::HostSelection(HostSelection::decode(payload)?),
        MessageType::CopiedText => Message::CopiedText(InputRef::decode(payload)?),
        MessageType::HostSearch => Message::HostSearch(HostSearch::decode(payload)?),
        MessageType::TerminalMouse => Message::TerminalMouse(TerminalMouse::decode(payload)?),
    })
}

pub fn encode_frame(message_type: MessageType, payload: &[u8]) -> Vec<u8> {
    debug_assert!(payload.len() <= MAX_FRAME_PAYLOAD as usize);
    let header = FrameHeader::new(message_type as u16, payload.len() as u32);
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&header.encode());
    out.extend_from_slice(payload);
    out
}
