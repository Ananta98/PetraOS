// SPDX-License-Identifier: GPL-2.0

//! Netlink (`AF_NETLINK`) Route Socket Subsystem.
//!
//! Provides the in-kernel Netlink route socket (`NETLINK_ROUTE`) handler
//! answering interface enumeration (`RTM_GETLINK`) and IP address queries (`RTM_GETADDR`),
//! required by musl libc's `getifaddrs(3)` and network configuration utilities.

use alloc::{collections::VecDeque, vec::Vec};
use ostd::sync::WaitQueue;
use spin::Mutex;

pub const AF_NETLINK: i32 = 16;
pub const NETLINK_ROUTE: i32 = 0;

pub const NLMSG_NOOP: u16 = 1;
pub const NLMSG_ERROR: u16 = 2;
pub const NLMSG_DONE: u16 = 3;
pub const NLMSG_OVERRUN: u16 = 4;

pub const RTM_NEWLINK: u16 = 16;
pub const RTM_GETLINK: u16 = 18;
pub const RTM_NEWADDR: u16 = 20;
pub const RTM_GETADDR: u16 = 22;
pub const RTM_GETROUTE: u16 = 26;

pub const NLM_F_REQUEST: u16 = 0x01;
pub const NLM_F_MULTI: u16 = 0x02;
pub const NLM_F_ACK: u16 = 0x04;
pub const NLM_F_DUMP: u16 = 0x0300;

/// State for an open Netlink socket.
pub struct NetlinkSocketState {
    pub protocol: i32,
    pub pid: Mutex<u32>,
    pub groups: Mutex<u32>,
    pub rx_buf: Mutex<VecDeque<u8>>,
    pub wq: WaitQueue,
}

impl NetlinkSocketState {
    pub fn new(protocol: i32) -> Self {
        Self {
            protocol,
            pid: Mutex::new(0),
            groups: Mutex::new(0),
            rx_buf: Mutex::new(VecDeque::new()),
            wq: WaitQueue::new(),
        }
    }

    /// Processes an incoming Netlink message from user space and generates replies.
    pub fn handle_request(&self, data: &[u8]) {
        if data.len() < 16 {
            return;
        }

        let nlmsg_len = u32::from_ne_bytes(data[0..4].try_into().unwrap()) as usize;
        let nlmsg_type = u16::from_ne_bytes(data[4..6].try_into().unwrap());
        let _nlmsg_flags = u16::from_ne_bytes(data[6..8].try_into().unwrap());
        let nlmsg_seq = u32::from_ne_bytes(data[8..12].try_into().unwrap());
        let nlmsg_pid = u32::from_ne_bytes(data[12..16].try_into().unwrap());

        if nlmsg_len < 16 || nlmsg_len > data.len() {
            return;
        }

        let mut reply = Vec::new();

        match nlmsg_type {
            RTM_GETLINK => {
                // 1. Loopback interface "lo" (index 1)
                let start =
                    push_nlmsghdr(&mut reply, RTM_NEWLINK, NLM_F_MULTI, nlmsg_seq, nlmsg_pid);
                // ifinfomsg (16 bytes): family=0, pad=0, type=ARPHRD_LOOPBACK(772), index=1, flags=0x49, change=0
                reply.push(0); // ifi_family = AF_UNSPEC
                reply.push(0); // __ifi_pad
                reply.extend_from_slice(&772u16.to_ne_bytes()); // ARPHRD_LOOPBACK
                reply.extend_from_slice(&1i32.to_ne_bytes()); // ifi_index = 1
                reply.extend_from_slice(&(0x1 | 0x8 | 0x40u32).to_ne_bytes()); // IFF_UP | IFF_LOOPBACK | IFF_RUNNING
                reply.extend_from_slice(&0u32.to_ne_bytes()); // ifi_change
                // Attributes
                push_rtattr(&mut reply, 3, b"lo\0"); // IFLA_IFNAME
                push_rtattr(&mut reply, 4, &65536u32.to_ne_bytes()); // IFLA_MTU
                finalize_nlmsg(&mut reply, start);

                // 2. Ethernet interface "eth0" (index 2)
                let start =
                    push_nlmsghdr(&mut reply, RTM_NEWLINK, NLM_F_MULTI, nlmsg_seq, nlmsg_pid);
                reply.push(0); // ifi_family = AF_UNSPEC
                reply.push(0); // __ifi_pad
                reply.extend_from_slice(&1u16.to_ne_bytes()); // ARPHRD_ETHER
                reply.extend_from_slice(&2i32.to_ne_bytes()); // ifi_index = 2
                reply.extend_from_slice(&(0x1 | 0x2 | 0x40 | 0x1000u32).to_ne_bytes()); // IFF_UP | IFF_BROADCAST | IFF_RUNNING | IFF_MULTICAST
                reply.extend_from_slice(&0u32.to_ne_bytes()); // ifi_change
                push_rtattr(&mut reply, 3, b"eth0\0"); // IFLA_IFNAME
                push_rtattr(&mut reply, 4, &1500u32.to_ne_bytes()); // IFLA_MTU
                push_rtattr(&mut reply, 1, &[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]); // IFLA_ADDRESS
                finalize_nlmsg(&mut reply, start);

                // 3. NLMSG_DONE terminator
                let start =
                    push_nlmsghdr(&mut reply, NLMSG_DONE, NLM_F_MULTI, nlmsg_seq, nlmsg_pid);
                reply.extend_from_slice(&0i32.to_ne_bytes());
                finalize_nlmsg(&mut reply, start);
            }
            RTM_GETADDR => {
                // 1. Loopback IPv4: 127.0.0.1/8
                let start =
                    push_nlmsghdr(&mut reply, RTM_NEWADDR, NLM_F_MULTI, nlmsg_seq, nlmsg_pid);
                // ifaddrmsg (8 bytes): family=AF_INET(2), prefixlen=8, flags=IFA_F_PERMANENT(0x80), scope=254(HOST), index=1
                reply.push(2); // ifa_family = AF_INET
                reply.push(8); // ifa_prefixlen
                reply.push(0x80); // ifa_flags = IFA_F_PERMANENT
                reply.push(254); // ifa_scope = RT_SCOPE_HOST
                reply.extend_from_slice(&1i32.to_ne_bytes()); // ifa_index = 1
                push_rtattr(&mut reply, 1, &[127, 0, 0, 1]); // IFA_ADDRESS
                push_rtattr(&mut reply, 2, &[127, 0, 0, 1]); // IFA_LOCAL
                push_rtattr(&mut reply, 3, b"lo\0"); // IFA_LABEL
                finalize_nlmsg(&mut reply, start);

                // 2. eth0 IPv4: 10.0.2.15/24
                let start =
                    push_nlmsghdr(&mut reply, RTM_NEWADDR, NLM_F_MULTI, nlmsg_seq, nlmsg_pid);
                reply.push(2); // ifa_family = AF_INET
                reply.push(24); // ifa_prefixlen
                reply.push(0x80); // ifa_flags = IFA_F_PERMANENT
                reply.push(0); // ifa_scope = RT_SCOPE_UNIVERSE
                reply.extend_from_slice(&2i32.to_ne_bytes()); // ifa_index = 2
                push_rtattr(&mut reply, 1, &[10, 0, 2, 15]); // IFA_ADDRESS
                push_rtattr(&mut reply, 2, &[10, 0, 2, 15]); // IFA_LOCAL
                push_rtattr(&mut reply, 3, b"eth0\0"); // IFA_LABEL
                finalize_nlmsg(&mut reply, start);

                // 3. NLMSG_DONE terminator
                let start =
                    push_nlmsghdr(&mut reply, NLMSG_DONE, NLM_F_MULTI, nlmsg_seq, nlmsg_pid);
                reply.extend_from_slice(&0i32.to_ne_bytes());
                finalize_nlmsg(&mut reply, start);
            }
            _ => {
                // Default: terminate with NLMSG_DONE
                let start =
                    push_nlmsghdr(&mut reply, NLMSG_DONE, NLM_F_MULTI, nlmsg_seq, nlmsg_pid);
                reply.extend_from_slice(&0i32.to_ne_bytes());
                finalize_nlmsg(&mut reply, start);
            }
        }

        let mut rx = self.rx_buf.lock();
        rx.extend(reply);
        drop(rx);

        self.wq.wake_all();
        crate::fs::poll::notify();
    }
}

impl core::fmt::Debug for NetlinkSocketState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NetlinkSocketState")
            .field("protocol", &self.protocol)
            .finish()
    }
}

fn push_nlmsghdr(buf: &mut Vec<u8>, msg_type: u16, flags: u16, seq: u32, pid: u32) -> usize {
    let start = buf.len();
    buf.extend_from_slice(&0u32.to_ne_bytes()); // placeholder for length
    buf.extend_from_slice(&msg_type.to_ne_bytes());
    buf.extend_from_slice(&flags.to_ne_bytes());
    buf.extend_from_slice(&seq.to_ne_bytes());
    buf.extend_from_slice(&pid.to_ne_bytes());
    start
}

fn finalize_nlmsg(buf: &mut Vec<u8>, start: usize) {
    let len = (buf.len() - start) as u32;
    buf[start..start + 4].copy_from_slice(&len.to_ne_bytes());
    // Pad to 4-byte alignment
    let pad = (4 - (buf.len() % 4)) % 4;
    buf.resize(buf.len() + pad, 0);
}

fn push_rtattr(buf: &mut Vec<u8>, rta_type: u16, data: &[u8]) {
    let rta_len = (4 + data.len()) as u16;
    buf.extend_from_slice(&rta_len.to_ne_bytes());
    buf.extend_from_slice(&rta_type.to_ne_bytes());
    buf.extend_from_slice(data);
    let pad = (4 - (buf.len() % 4)) % 4;
    buf.resize(buf.len() + pad, 0);
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::net::socket::{KernelSocket, NETLINK_ROUTE};
    use ostd::prelude::ktest;

    #[ktest]
    fn netlink_route_getlink_query_and_response() {
        let sock = KernelSocket::new_netlink(NETLINK_ROUTE).expect("netlink socket");

        // Build RTM_GETLINK request (header length 16)
        let mut req = Vec::new();
        let start = push_nlmsghdr(&mut req, RTM_GETLINK, NLM_F_REQUEST | NLM_F_DUMP, 42, 100);
        finalize_nlmsg(&mut req, start);

        sock.send(&req, 0).expect("netlink send");

        let mut rx_buf = [0u8; 2048];
        let n = sock.recv(&mut rx_buf, 0).expect("netlink recv");
        assert!(n >= 16);

        // Verify there is at least one NLMSG in the buffer and ends with NLMSG_DONE
        let mut offset = 0;
        let mut found_link = false;
        let mut found_done = false;

        while offset + 16 <= n {
            let msg_len =
                u32::from_ne_bytes(rx_buf[offset..offset + 4].try_into().unwrap()) as usize;
            let msg_type = u16::from_ne_bytes(rx_buf[offset + 4..offset + 6].try_into().unwrap());
            if msg_type == RTM_NEWLINK {
                found_link = true;
            } else if msg_type == NLMSG_DONE {
                found_done = true;
            }
            if msg_len == 0 || offset + msg_len > n {
                break;
            }
            let pad = (4 - (msg_len % 4)) % 4;
            offset += msg_len + pad;
        }

        assert!(found_link, "RTM_NEWLINK message should be returned");
        assert!(found_done, "NLMSG_DONE message should terminate the dump");
    }

    #[ktest]
    fn netlink_route_getaddr_query_and_response() {
        let sock = KernelSocket::new_netlink(NETLINK_ROUTE).expect("netlink socket");

        // Build RTM_GETADDR request
        let mut req = Vec::new();
        let start = push_nlmsghdr(&mut req, RTM_GETADDR, NLM_F_REQUEST | NLM_F_DUMP, 43, 101);
        finalize_nlmsg(&mut req, start);

        sock.send(&req, 0).expect("netlink send");

        let mut rx_buf = [0u8; 2048];
        let n = sock.recv(&mut rx_buf, 0).expect("netlink recv");
        assert!(n >= 16);

        let mut offset = 0;
        let mut found_addr = false;
        let mut found_done = false;

        while offset + 16 <= n {
            let msg_len =
                u32::from_ne_bytes(rx_buf[offset..offset + 4].try_into().unwrap()) as usize;
            let msg_type = u16::from_ne_bytes(rx_buf[offset + 4..offset + 6].try_into().unwrap());
            if msg_type == RTM_NEWADDR {
                found_addr = true;
            } else if msg_type == NLMSG_DONE {
                found_done = true;
            }
            if msg_len == 0 || offset + msg_len > n {
                break;
            }
            let pad = (4 - (msg_len % 4)) % 4;
            offset += msg_len + pad;
        }

        assert!(found_addr, "RTM_NEWADDR message should be returned");
        assert!(found_done, "NLMSG_DONE message should terminate the dump");
    }
}
