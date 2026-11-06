//! ADR-028 leave-offer ABI projection (thin host confirm UI).

use std::mem::size_of;

use crate::app::{LeaveOfferKind, APP_ABI_VERSION};

use super::{AppHandle, APPS};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppLeaveOffer {
    pub version: u16,
    pub size: u16,
    pub kind: u16,
    pub path_len: u16,
    pub path: *const u8,
    pub jump_workspace_lo: u64,
    pub jump_workspace_hi: u64,
}

impl AppHandle {
    pub(super) fn encode_leave_offer(&mut self) -> SeyalAppLeaveOffer {
        let empty = SeyalAppLeaveOffer {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppLeaveOffer>() as u16,
            kind: 0,
            path_len: 0,
            path: std::ptr::null(),
            jump_workspace_lo: 0,
            jump_workspace_hi: 0,
        };
        let Some(offer) = self.root.leave_offer().cloned() else {
            self.leave_offer_path.clear();
            return empty;
        };
        self.leave_offer_path = offer.path.into_bytes();
        let (kind, jump_lo, jump_hi) = match offer.kind {
            LeaveOfferKind::Create => (1u16, 0u64, 0u64),
            LeaveOfferKind::Jump { workspace } => {
                let bytes = workspace.to_bytes();
                let lo = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
                let hi = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
                (2, lo, hi)
            }
        };
        let path_len = u16::try_from(self.leave_offer_path.len()).unwrap_or(0);
        SeyalAppLeaveOffer {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppLeaveOffer>() as u16,
            kind,
            path_len,
            path: self.leave_offer_path.as_ptr(),
            jump_workspace_lo: jump_lo,
            jump_workspace_hi: jump_hi,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_leave_offer(handle: u64) -> SeyalAppLeaveOffer {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppLeaveOffer {
                version: APP_ABI_VERSION,
                size: size_of::<SeyalAppLeaveOffer>() as u16,
                kind: 0,
                path_len: 0,
                path: std::ptr::null(),
                jump_workspace_lo: 0,
                jump_workspace_hi: 0,
            };
        };
        state.encode_leave_offer()
    })
}
