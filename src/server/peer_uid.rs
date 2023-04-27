use serde::Deserialize;
use serde::Serialize;

/// Unique ID for a client connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default, derive_more::Deref, derive_more::From)]
pub struct PeerUid(pub u64);

impl PeerUid {
    pub(crate) fn increment(&mut self) {
        self.0 += 1;
    }
}
