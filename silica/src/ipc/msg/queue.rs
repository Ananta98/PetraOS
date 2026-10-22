// SPDX-License-Identifier: GPL-2.0

//! Message queue state and synchronization.

use alloc::collections::VecDeque;
use ostd::sync::WaitQueue;

use super::message::Message;
use crate::uapi::msg::MsqIdDs;

/// System V Message Queue structure.
pub struct MsgQueue {
    /// Associated `msqid_ds` descriptor.
    pub ds: MsqIdDs,
    /// FIFO queue of messages.
    pub messages: VecDeque<Message>,
    /// WaitQueue for tasks waiting to send messages when queue is full.
    pub send_wq: WaitQueue,
    /// WaitQueue for tasks waiting to receive messages matching criteria.
    pub recv_wq: WaitQueue,
    /// Flag indicating whether the queue has been marked for deletion (`IPC_RMID`).
    pub is_deleted: bool,
}

impl MsgQueue {
    pub fn new(ds: MsqIdDs) -> Self {
        Self {
            ds,
            messages: VecDeque::new(),
            send_wq: WaitQueue::new(),
            recv_wq: WaitQueue::new(),
            is_deleted: false,
        }
    }
}
