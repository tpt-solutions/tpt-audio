//! Archon backend — **research-gated**.
//!
//! Ported from the old `platform-archon` crate. The Archon audio server API
//! (and the zero-copy `tpt-archon-bridge` IPC) is not yet published, so this
//! backend cannot be implemented. What is preserved from the research crate:
//!
//! - the [`Capability`] request/grant model (`AUDIO_CAPTURE` / `AUDIO_RENDER`),
//! - the "everything live returns `Unsupported`" contract with diagnostics.
//!
//! When the upstream transport lands, `AudioBackend` impl here becomes the
//! Archon device/stream surface.

/// Capability tokens understood by the Archon microkernel audio server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Permission for an app to capture system audio (microphone/loopback).
    AudioCapture,
    /// Permission to render audio to a system output.
    AudioRender,
}

/// Outcome of a capability request made to the Archon security broker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapabilityGrant {
    /// The user granted the capability.
    Granted(Capability),
    /// The user denied the capability.
    Denied(Capability),
    /// The broker has not yet responded (awaiting user decision).
    Pending(Capability),
}

/// Tracks capability grants for the research-gated backend.
#[derive(Default)]
pub struct ArchonCapabilityBroker {
    grants: std::collections::HashMap<Capability, CapabilityGrant>,
}

impl ArchonCapabilityBroker {
    /// Requests a capability. Without the real broker transport this always
    /// resolves to [`CapabilityGrant::Pending`].
    pub fn request(&mut self, cap: Capability) -> CapabilityGrant {
        let grant = CapabilityGrant::Pending(cap);
        self.grants.insert(cap, grant.clone());
        grant
    }

    /// Whether the capability has been granted.
    pub fn is_granted(&self, cap: Capability) -> bool {
        matches!(self.grants.get(&cap), Some(CapabilityGrant::Granted(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_pending_until_a_broker_grants() {
        let mut broker = ArchonCapabilityBroker::default();
        let grant = broker.request(Capability::AudioCapture);
        assert_eq!(grant, CapabilityGrant::Pending(Capability::AudioCapture));
        assert!(!broker.is_granted(Capability::AudioCapture));

        broker.grants.insert(
            Capability::AudioCapture,
            CapabilityGrant::Granted(Capability::AudioCapture),
        );
        assert!(broker.is_granted(Capability::AudioCapture));
    }
}
