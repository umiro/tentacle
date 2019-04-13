use crate::{
    builder::{CodecFn, NameFn, SelectVersionFn, ServiceHandleFn, SessionHandleFn},
    traits::{Codec, ServiceProtocol, SessionProtocol},
    yamux::config::Config as YamuxConfig,
    ProtocolId, SessionId,
};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

pub(crate) struct ServiceConfig {
    pub timeout: Duration,
    pub yamux_config: YamuxConfig,
    pub max_frame_length: usize,
    /// event output or callback output
    pub event: HashSet<ProtocolId>,
    /// Whether to allow the handle to be reopen
    pub reopen: bool,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        ServiceConfig {
            timeout: Duration::from_secs(10),
            yamux_config: YamuxConfig::default(),
            max_frame_length: 1024 * 1024 * 8,
            event: HashSet::default(),
            reopen: false,
        }
    }
}

/// When dial, specify which protocol want to open
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum DialProtocol {
    /// Try open all protocol
    All,
    /// Try open one protocol
    Single(ProtocolId),
    /// Try open some protocol
    Multi(Vec<ProtocolId>),
}

/// When sending a message, select the specified session
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum TargetSession {
    /// Try broadcast
    All,
    /// Try send to only one
    Single(SessionId),
    /// Try send to some session
    Multi(Vec<SessionId>),
}

/// Define the minimum data required for a custom protocol
pub struct ProtocolMeta {
    pub(crate) inner: Arc<Meta>,
    pub(crate) service_handle: ServiceHandleFn,
    pub(crate) session_handle: SessionHandleFn,
}

impl ProtocolMeta {
    /// Protocol id
    #[inline]
    pub fn id(&self) -> ProtocolId {
        self.inner.id
    }

    /// Protocol name, default is "/p2p/protocol_id"
    #[inline]
    pub fn name(&self) -> String {
        (self.inner.name)(self.inner.id)
    }

    /// Protocol supported version
    #[inline]
    pub fn support_versions(&self) -> Vec<String> {
        self.inner.support_versions.clone()
    }

    /// The codec used by the custom protocol, such as `LengthDelimitedCodec` by tokio
    #[inline]
    pub fn codec(&self) -> Box<dyn Codec + Send + 'static> {
        (self.inner.codec)()
    }

    /// A service level callback handle for a protocol.
    ///
    /// ---
    ///
    /// #### Behavior
    ///
    /// This function is called when the protocol is first opened in the service
    /// and remains in memory until the entire service is closed.
    #[inline]
    pub fn service_handle(&mut self) -> ProtocolHandle<Box<dyn ServiceProtocol + Send + 'static>> {
        (self.service_handle)()
    }

    /// A session level callback handle for a protocol.
    ///
    /// ---
    ///
    /// #### Behavior
    ///
    /// When a session is opened, whenever the protocol of the session is opened,
    /// the function will be called again to generate the corresponding exclusive handle.
    ///
    /// Correspondingly, whenever the protocol is closed, the corresponding exclusive handle is cleared.
    #[inline]
    pub fn session_handle(&mut self) -> ProtocolHandle<Box<dyn SessionProtocol + Send + 'static>> {
        (self.session_handle)()
    }
}

pub(crate) struct Meta {
    pub(crate) id: ProtocolId,
    pub(crate) name: NameFn,
    pub(crate) support_versions: Vec<String>,
    pub(crate) codec: CodecFn,
    pub(crate) select_version: SelectVersionFn,
}

/// Protocol handle
pub enum ProtocolHandle<T: Sized> {
    /// No operation
    Neither,
    /// Event output
    Event,
    /// Both event and callback
    Both(T),
    /// Callback handle
    Callback(T),
}

impl<T> ProtocolHandle<T> {
    /// Returns true if the enum is a callback value.
    #[inline]
    pub fn is_callback(&self) -> bool {
        if let ProtocolHandle::Callback(_) = self {
            true
        } else {
            false
        }
    }

    /// Returns true if the enum is a empty value.
    #[inline]
    pub fn is_neither(&self) -> bool {
        if let ProtocolHandle::Neither = self {
            true
        } else {
            false
        }
    }

    /// Returns true if the enum is a event value.
    #[inline]
    pub fn is_event(&self) -> bool {
        if let ProtocolHandle::Event = self {
            true
        } else {
            false
        }
    }

    /// Returns true if the enum is a both value.
    #[inline]
    pub fn is_both(&self) -> bool {
        if let ProtocolHandle::Both(_) = self {
            true
        } else {
            false
        }
    }

    /// Returns true if the enum is a both value.
    #[inline]
    pub fn has_event(&self) -> bool {
        self.is_event() || self.is_both()
    }
}

/// Service state
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum State {
    /// Calculate the number of connection requests that need to be sent externally
    Running(usize),
    Forever,
    PreShutdown,
}

impl State {
    /// new
    pub fn new(forever: bool) -> Self {
        if forever {
            State::Forever
        } else {
            State::Running(0)
        }
    }

    /// Can it be shutdown?
    #[inline]
    pub fn is_shutdown(&self) -> bool {
        match self {
            State::Running(num) if num == &0 => true,
            State::PreShutdown => true,
            State::Running(_) | State::Forever => false,
        }
    }

    /// Convert to pre shutdown state
    #[inline]
    pub fn pre_shutdown(&mut self) {
        *self = State::PreShutdown
    }

    /// Add one task count
    #[inline]
    pub fn increase(&mut self) {
        match self {
            State::Running(num) => *num += 1,
            State::PreShutdown | State::Forever => (),
        }
    }

    /// Reduce one task count
    #[inline]
    pub fn decrease(&mut self) {
        match self {
            State::Running(num) => *num -= 1,
            State::PreShutdown | State::Forever => (),
        }
    }
}

#[cfg(test)]
mod test {
    use super::State;

    #[test]
    fn test_state_no_forever() {
        let mut state = State::new(false);
        state.increase();
        state.increase();
        assert_eq!(state, State::Running(2));
        state.decrease();
        state.decrease();
        assert_eq!(state, State::Running(0));
        state.increase();
        state.increase();
        state.increase();
        state.increase();
        state.pre_shutdown();
        assert_eq!(state, State::PreShutdown);
    }

    #[test]
    fn test_state_forever() {
        let mut state = State::new(true);
        state.increase();
        state.increase();
        assert_eq!(state, State::Forever);
        state.decrease();
        state.decrease();
        assert_eq!(state, State::Forever);
        state.increase();
        state.increase();
        state.increase();
        state.increase();
        state.pre_shutdown();
        assert_eq!(state, State::PreShutdown);
    }
}
