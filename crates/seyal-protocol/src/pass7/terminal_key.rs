//! Pass 7 semantic terminal-key wire payloads (v1 and v2).

use crate::{framing::FramingError, AttachmentId};

use super::{attachment_id_from, exact_len};

pub const CAP_SEMANTIC_TERMINAL_KEY: u32 = 1 << 2;
pub const CAP_EXTENDED_TERMINAL_KEY: u32 = 1 << 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum TerminalKeyKind {
    Enter = 1,
    Tab = 2,
    Backspace = 3,
    Escape = 4,
    ArrowUp = 5,
    ArrowDown = 6,
    ArrowRight = 7,
    ArrowLeft = 8,
    ControlAscii = 9,
}

impl TerminalKeyKind {
    fn from_u16(value: u16) -> Result<Self, FramingError> {
        match value {
            1 => Ok(Self::Enter),
            2 => Ok(Self::Tab),
            3 => Ok(Self::Backspace),
            4 => Ok(Self::Escape),
            5 => Ok(Self::ArrowUp),
            6 => Ok(Self::ArrowDown),
            7 => Ok(Self::ArrowRight),
            8 => Ok(Self::ArrowLeft),
            9 => Ok(Self::ControlAscii),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalKeyModifiers(u16);

impl TerminalKeyModifiers {
    pub const NONE: Self = Self(0);
    pub const CONTROL: Self = Self(1 << 0);

    pub const fn bits(self) -> u16 {
        self.0
    }

    fn from_bits(bits: u16) -> Result<Self, FramingError> {
        match bits {
            0 => Ok(Self::NONE),
            1 => Ok(Self::CONTROL),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

fn valid_control_ascii_scalar(scalar: u32) -> bool {
    matches!(scalar, 0x20 | 0x3f | 0x40 | 0x41..=0x5f)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalKey {
    pub attachment_id: AttachmentId,
    pub kind: TerminalKeyKind,
    pub modifiers: TerminalKeyModifiers,
    pub scalar: u32,
}

impl TerminalKey {
    pub const WIRE_LEN: usize = 24;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&(self.kind as u16).to_le_bytes());
        out.extend_from_slice(&self.modifiers.bits().to_le_bytes());
        out.extend_from_slice(&self.scalar.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        let value = Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            kind: TerminalKeyKind::from_u16(u16::from_le_bytes(bytes[16..18].try_into().unwrap()))?,
            modifiers: TerminalKeyModifiers::from_bits(u16::from_le_bytes(
                bytes[18..20].try_into().unwrap(),
            ))?,
            scalar: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), FramingError> {
        match self.kind {
            TerminalKeyKind::ControlAscii => {
                if self.modifiers != TerminalKeyModifiers::CONTROL
                    || !valid_control_ascii_scalar(self.scalar)
                {
                    return Err(FramingError::MalformedPayload);
                }
            }
            _ => {
                if self.modifiers != TerminalKeyModifiers::NONE || self.scalar != 0 {
                    return Err(FramingError::MalformedPayload);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum TerminalKeyV2Kind {
    Enter = 1,
    Tab = 2,
    Backspace = 3,
    Escape = 4,
    ArrowUp = 5,
    ArrowDown = 6,
    ArrowRight = 7,
    ArrowLeft = 8,
    Home = 9,
    End = 10,
    Insert = 11,
    Delete = 12,
    PageUp = 13,
    PageDown = 14,
    Function = 15,
    Keypad = 16,
    Ascii = 17,
}

impl TerminalKeyV2Kind {
    fn from_u16(value: u16) -> Result<Self, FramingError> {
        Ok(match value {
            1 => Self::Enter,
            2 => Self::Tab,
            3 => Self::Backspace,
            4 => Self::Escape,
            5 => Self::ArrowUp,
            6 => Self::ArrowDown,
            7 => Self::ArrowRight,
            8 => Self::ArrowLeft,
            9 => Self::Home,
            10 => Self::End,
            11 => Self::Insert,
            12 => Self::Delete,
            13 => Self::PageUp,
            14 => Self::PageDown,
            15 => Self::Function,
            16 => Self::Keypad,
            17 => Self::Ascii,
            _ => return Err(FramingError::MalformedPayload),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalKeyV2Modifiers(u16);

impl TerminalKeyV2Modifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const ALT: Self = Self(1 << 1);
    pub const CONTROL: Self = Self(1 << 2);
    pub const ALT_SHIFT: Self = Self((1 << 0) | (1 << 1));
    pub const ALT_CONTROL: Self = Self((1 << 1) | (1 << 2));

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub fn from_bits_for_ffi(bits: u16) -> Option<Self> {
        if bits & !0b111 != 0 {
            None
        } else {
            Some(Self(bits))
        }
    }

    fn from_bits(bits: u16) -> Result<Self, FramingError> {
        if bits & !0b111 != 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self(bits))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TerminalKeyV2Event {
    Press = 1,
    Repeat = 2,
    Release = 3,
}

impl TerminalKeyV2Event {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            1 => Ok(Self::Press),
            2 => Ok(Self::Repeat),
            3 => Ok(Self::Release),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalKeyV2 {
    pub attachment_id: AttachmentId,
    pub kind: TerminalKeyV2Kind,
    pub modifiers: TerminalKeyV2Modifiers,
    pub value: u32,
    pub event: TerminalKeyV2Event,
    pub shifted_ascii: u32,
    pub action_id: u32,
}

impl TerminalKeyV2 {
    pub const WIRE_LEN: usize = 40;
    pub const VERSION: u16 = 2;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&(self.kind as u16).to_le_bytes());
        out.extend_from_slice(&self.modifiers.bits().to_le_bytes());
        out.extend_from_slice(&self.value.to_le_bytes());
        out.push(self.event as u8);
        out.push(0);
        out.extend_from_slice(&Self::VERSION.to_le_bytes());
        out.extend_from_slice(&self.shifted_ascii.to_le_bytes());
        out.extend_from_slice(&self.action_id.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[25] != 0
            || u16::from_le_bytes(bytes[26..28].try_into().unwrap()) != Self::VERSION
            || u32::from_le_bytes(bytes[36..40].try_into().unwrap()) != 0
        {
            return Err(FramingError::MalformedPayload);
        }
        let value = Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            kind: TerminalKeyV2Kind::from_u16(u16::from_le_bytes(
                bytes[16..18].try_into().unwrap(),
            ))?,
            modifiers: TerminalKeyV2Modifiers::from_bits(u16::from_le_bytes(
                bytes[18..20].try_into().unwrap(),
            ))?,
            value: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            event: TerminalKeyV2Event::from_u8(bytes[24])?,
            shifted_ascii: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            action_id: u32::from_le_bytes(bytes[32..36].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), FramingError> {
        if self.action_id == 0 {
            return Err(FramingError::MalformedPayload);
        }
        let modifiers = self.modifiers.bits();
        match self.kind {
            TerminalKeyV2Kind::Function => {
                if !(1..=12).contains(&self.value) || self.shifted_ascii != 0 {
                    return Err(FramingError::MalformedPayload);
                }
            }
            TerminalKeyV2Kind::Keypad => {
                if !(self.value <= 16) || self.shifted_ascii != 0 {
                    return Err(FramingError::MalformedPayload);
                }
            }
            TerminalKeyV2Kind::Ascii => {
                if !(0x20..=0x7e).contains(&self.value)
                    || (b'A' as u32..=b'Z' as u32).contains(&self.value)
                    || modifiers
                        & (TerminalKeyV2Modifiers::ALT.bits()
                            | TerminalKeyV2Modifiers::CONTROL.bits())
                        == 0
                {
                    return Err(FramingError::MalformedPayload);
                }
                if modifiers & TerminalKeyV2Modifiers::SHIFT.bits() != 0 {
                    if !(0x20..=0x7e).contains(&self.shifted_ascii) {
                        return Err(FramingError::MalformedPayload);
                    }
                } else if self.shifted_ascii != 0 {
                    return Err(FramingError::MalformedPayload);
                }
            }
            _ => {
                if self.value != 0 || self.shifted_ascii != 0 {
                    return Err(FramingError::MalformedPayload);
                }
            }
        }
        Ok(())
    }
}
