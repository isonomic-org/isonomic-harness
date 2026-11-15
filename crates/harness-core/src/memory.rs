//! The `memory` event: one per change a session makes to the memory store.
//! It holds a reference, never the text.

use crate::block::MemoryRef;
use crate::ids::Seq;
use crate::kinds::{MemoryOp, MemoryScope};
use serde::{Deserialize, Serialize};

/// A session created, revised or deleted a memory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEvent {
    /// Created, revised or deleted.
    pub op: MemoryOp,
    /// The revision written; for a deletion, the revision that was current.
    #[serde(rename = "ref")]
    pub reference: MemoryRef,
    /// User, app or agent.
    pub scope: MemoryScope,
    /// The `user` event in this session whose words are the evidence. Absent
    /// for deletions and for agent notes, which are saved unreviewed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_seq: Option<Seq>,
}
