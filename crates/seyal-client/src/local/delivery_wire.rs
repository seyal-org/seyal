//! SPEC-004 §19 SuspendDelivery / ResumeDelivery helpers for LocalDisplayClient.

use seyal_runtime::local_ipc::framing::{
    encode_frame, MessageType, ResumeDelivery, SuspendDelivery, CAP_ATTACHMENT_DELIVERY_CONTROL,
};

use super::{input_resize::OutboundKind, ClientError, LocalDisplayClient};

impl LocalDisplayClient {
    pub fn delivery_control_negotiated(&self) -> bool {
        self.delivery_control_negotiated
    }

    pub fn delivery_suspended(&self) -> bool {
        self.delivery_suspended
    }

    /// Queue type 40/41. Idempotent. No-op when the capability was not negotiated.
    pub fn set_delivery_suspended(&mut self, suspend: bool) -> Result<(), ClientError> {
        if !self.delivery_control_negotiated || self.delivery_suspended == suspend {
            return Ok(());
        }
        let payload = if suspend {
            SuspendDelivery {
                attachment_id: self.attachment_id,
            }
            .encode()
        } else {
            ResumeDelivery {
                attachment_id: self.attachment_id,
            }
            .encode()
        };
        let kind = if suspend {
            MessageType::SuspendDelivery
        } else {
            MessageType::ResumeDelivery
        };
        let frame = encode_frame(kind, &payload);
        self.admit_frame(frame, OutboundKind::DeliveryControl)?;
        self.delivery_suspended = suspend;
        let _ = self.flush_control_write();
        Ok(())
    }
}

pub(crate) fn delivery_control_negotiated(server_capabilities: u32) -> bool {
    server_capabilities & CAP_ATTACHMENT_DELIVERY_CONTROL != 0
}
