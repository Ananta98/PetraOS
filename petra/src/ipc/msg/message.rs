// SPDX-License-Identifier: GPL-2.0

//! Individual message representation for System V Message Queues.

use alloc::vec::Vec;

/// A message buffered inside a System V Message Queue.
#[derive(Clone, Debug)]
pub struct Message {
    /// Message type identifier (must be positive).
    pub mtype: i64,
    /// Message text payload.
    pub mtext: Vec<u8>,
}

impl Message {
    pub fn new(mtype: i64, mtext: Vec<u8>) -> Self {
        Self { mtype, mtext }
    }

    pub fn size(&self) -> usize {
        self.mtext.len()
    }
}
