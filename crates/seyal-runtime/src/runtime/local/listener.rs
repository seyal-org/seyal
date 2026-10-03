use std::time::Instant;

use seyal_exec::{ReactorEventKind, RegistrationToken};

use crate::{local_ipc::connection::ServerEvent, RuntimeError};

#[cfg(feature = "test-fault-injection")]
use crate::test_fault::{self, FaultPoint};

use super::super::Runtime;
use super::connection::ConnectionMeta;
use super::{ACCEPT_BACKOFF_INITIAL, ACCEPT_BACKOFF_MAX};

impl Runtime {
    pub(in crate::runtime) fn local_ipc_deadline(&self) -> Option<Instant> {
        self.local_ipc
            .as_ref()
            .and_then(|state| state.listener_backoff_deadline)
    }

    pub(in crate::runtime) fn service_local_deadline(
        &mut self,
        now: Instant,
    ) -> Result<(), RuntimeError> {
        let token = self.local_ipc.as_ref().and_then(|state| {
            state
                .listener_backoff_deadline
                .filter(|deadline| *deadline <= now)
                .map(|_| state.listener_reactor_token)
        });
        let Some(token) = token else {
            return Ok(());
        };
        self.reactor.set_readable(token, true)?;
        if let Some(state) = self.local_ipc.as_mut() {
            state.listener_backoff_deadline = None;
        }
        Ok(())
    }

    pub(super) fn backoff_local_listener(&mut self) -> Result<(), RuntimeError> {
        let (token, delay) = match self.local_ipc.as_ref() {
            Some(state) => (state.listener_reactor_token, state.listener_backoff_delay),
            None => return Ok(()),
        };
        self.reactor.set_readable(token, false)?;
        if let Some(state) = self.local_ipc.as_mut() {
            state.listener_backoff_deadline = Some(Instant::now() + delay);
            state.listener_backoff_delay = delay.saturating_mul(2).min(ACCEPT_BACKOFF_MAX);
        }
        Ok(())
    }

    pub(super) fn reset_local_listener_backoff(&mut self) {
        if let Some(state) = self.local_ipc.as_mut() {
            state.listener_backoff_deadline = None;
            state.listener_backoff_delay = ACCEPT_BACKOFF_INITIAL;
        }
    }

    pub(in crate::runtime) fn service_local_reactor_event(
        &mut self,
        reactor_token: RegistrationToken,
        kind: ReactorEventKind,
        hangup: bool,
    ) -> Result<(), RuntimeError> {
        if self
            .local_ipc
            .as_ref()
            .is_some_and(|state| state.listener_reactor_token == reactor_token)
        {
            self.accept_local_connections()?;
            return Ok(());
        }
        let connection_token = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.reactor_connections.get(&reactor_token).copied());
        let Some(connection_token) = connection_token else {
            return Ok(());
        };
        let events = {
            let Some(state) = self.local_ipc.as_mut() else {
                return Ok(());
            };
            match kind {
                ReactorEventKind::AuxiliaryReadable => {
                    state.server.service_read(connection_token, hangup)
                }
                ReactorEventKind::AuxiliaryWritable => state.server.service_write(connection_token),
                _ => Vec::new(),
            }
        };
        self.handle_local_server_events(events);
        if self.local_connection_exists(connection_token) {
            let close_after_flush = self.local_ipc.as_ref().is_some_and(|state| {
                state
                    .connections
                    .get(&connection_token)
                    .is_some_and(|meta| meta.close_after_flush)
                    && !state.server.wants_write(connection_token)
            });
            if close_after_flush {
                self.close_local_connection(connection_token);
                return Ok(());
            }
            self.sync_local_writable(connection_token);
        }
        Ok(())
    }

    pub(super) fn accept_local_connections(&mut self) -> Result<(), RuntimeError> {
        let events = {
            let Some(state) = self.local_ipc.as_mut() else {
                return Ok(());
            };
            #[cfg(feature = "test-fault-injection")]
            if test_fault::take(FaultPoint::AcceptResourcePressure) {
                Vec::new()
            } else {
                state.server.accept_ready()?
            }
            #[cfg(not(feature = "test-fault-injection"))]
            {
                state.server.accept_ready()?
            }
        };

        if events.is_empty() {
            self.backoff_local_listener()?;
            return Ok(());
        }
        self.reset_local_listener_backoff();

        for event in events {
            match event {
                ServerEvent::Connected { token } => {
                    let fd = self
                        .local_ipc
                        .as_ref()
                        .and_then(|state| state.server.connection_fd(token));
                    let Some(fd) = fd else {
                        continue;
                    };
                    #[cfg(feature = "test-fault-injection")]
                    if test_fault::take(FaultPoint::ConnectionReactorRegistration) {
                        if let Some(state) = self.local_ipc.as_mut() {
                            state.server.close(token);
                        }
                        continue;
                    }
                    match self.reactor.register_auxiliary(fd) {
                        Ok(reactor_token) => {
                            if let Some(state) = self.local_ipc.as_mut() {
                                state.connections.insert(
                                    token,
                                    ConnectionMeta {
                                        attachment: None,
                                        reactor_token,
                                        last_resize_request_id: 0,
                                        last_provisioning_request_id: 0,
                                        outstanding_creates: 0,
                                        client_capabilities: 0,
                                        last_terminal_key_action_id: 0,
                                        last_terminal_mouse_action_id: 0,
                                        close_after_flush: false,
                                    },
                                );
                                state.reactor_connections.insert(reactor_token, token);
                            }
                        }
                        Err(_) => {
                            if let Some(state) = self.local_ipc.as_mut() {
                                state.server.close(token);
                            }
                        }
                    }
                }
                ServerEvent::PeerRejected => {}
                other => self.handle_local_server_events(vec![other]),
            }
        }
        Ok(())
    }

    pub(super) fn handle_local_server_events(&mut self, events: Vec<ServerEvent>) {
        for event in events {
            match event {
                ServerEvent::Connected { .. } | ServerEvent::PeerRejected => {}
                ServerEvent::FramingError { token } | ServerEvent::Disconnected { token } => {
                    self.close_local_connection(token);
                }
                ServerEvent::Frame {
                    token,
                    message_type,
                    payload,
                } => {
                    let closing = self.local_ipc.as_ref().is_some_and(|state| {
                        state
                            .connections
                            .get(&token)
                            .is_some_and(|meta| meta.close_after_flush)
                    });
                    if !closing {
                        self.dispatch_local_ipc_frame(token, message_type, &payload);
                    }
                }
            }
        }
    }
}
