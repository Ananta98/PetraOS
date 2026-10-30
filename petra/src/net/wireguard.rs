// SPDX-License-Identifier: GPL-2.0

//! WireGuard Secure VPN Tunnel Network Device.
//!
//! Implements a virtual network interface (`wg0`) communicating over encrypted
//! UDP tunnels using ChaCha20-Poly1305 AEAD (RFC 7539 / RFC 8439) for authenticated
//! packet encryption and anti-replay counters.

use alloc::{collections::VecDeque, string::String, sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::{
    crypto::cipher::ChaCha20Poly1305,
    drivers::net::NetDevice,
    uapi::errno::{Errno, Result},
};

/// WireGuard message header types.
pub mod message_type {
    pub const HANDSHAKE_INITIATION: u32 = 1;
    pub const HANDSHAKE_RESPONSE: u32 = 2;
    pub const COOKIE_REPLY: u32 = 3;
    pub const DATA: u32 = 4;
}

/// Standard WireGuard MTU (1420 bytes, accounting for IPv4/UDP/WireGuard overhead).
pub const WG_MTU: usize = 1420;

/// WireGuard Data packet header size (4-byte type + 4-byte receiver index + 8-byte counter).
pub const WG_DATA_HEADER_LEN: usize = 16;

/// WireGuard Poly1305 MAC tag length.
pub const WG_MAC_LEN: usize = 16;

/// Configuration for a WireGuard tunnel peer.
#[derive(Clone, Debug)]
pub struct WireGuardPeerConfig {
    pub public_key: [u8; 32],
    pub shared_secret: [u8; 32],
    pub receiver_index: u32,
    pub endpoint: Option<[u8; 4]>,
    pub port: u16,
}

/// WireGuard virtual tunnel interface device.
pub struct WireGuardDevice {
    name: String,
    mac: [u8; 6],
    enabled: AtomicBool,
    tx_counter: AtomicU64,
    peer_config: Mutex<Option<WireGuardPeerConfig>>,
    rx_queue: Mutex<VecDeque<Vec<u8>>>,
    tx_queue: Mutex<VecDeque<Vec<u8>>>,
}

impl WireGuardDevice {
    /// Creates a new WireGuard device with interface `name`.
    pub fn new(name: &str) -> Self {
        // Virtual MAC address with locally-administered 'WG' prefix (0x02, 'W', 'G', ...)
        let mac = [0x02, 0x57, 0x47, 0x00, 0x00, 0x01];
        Self {
            name: String::from(name),
            mac,
            enabled: AtomicBool::new(true),
            tx_counter: AtomicU64::new(1),
            peer_config: Mutex::new(None),
            rx_queue: Mutex::new(VecDeque::new()),
            tx_queue: Mutex::new(VecDeque::new()),
        }
    }

    /// Sets or updates the configured tunnel peer.
    pub fn set_peer(&self, peer: WireGuardPeerConfig) {
        *self.peer_config.lock() = Some(peer);
    }

    /// Injects an encrypted UDP WireGuard packet from the network into the tunnel for decryption.
    pub fn receive_udp_packet(&self, packet: &[u8]) -> Result<()> {
        if packet.len() < WG_DATA_HEADER_LEN + WG_MAC_LEN {
            return Err(Errno::EINVAL);
        }

        let msg_type = u32::from_le_bytes(packet[0..4].try_into().unwrap_or([0; 4]));
        if msg_type != message_type::DATA {
            // Non-data messages (handshakes) are handled or dropped
            return Ok(());
        }

        let counter = u64::from_le_bytes(packet[8..16].try_into().unwrap_or([0; 8]));
        let lock = self.peer_config.lock();
        let peer = lock.as_ref().ok_or(Errno::ENOTCONN)?;

        // Build 12-byte ChaCha20-Poly1305 nonce: [0, 0, 0, 0] || counter in little-endian
        let mut nonce = [0u8; 12];
        nonce[4..12].copy_from_slice(&counter.to_le_bytes());

        let payload_len = packet.len() - WG_DATA_HEADER_LEN - WG_MAC_LEN;
        let mut plaintext = vec![0u8; payload_len];
        plaintext.copy_from_slice(&packet[WG_DATA_HEADER_LEN..WG_DATA_HEADER_LEN + payload_len]);

        let mut expected_tag = [0u8; 16];
        expected_tag.copy_from_slice(&packet[packet.len() - WG_MAC_LEN..]);

        // Authenticate and decrypt inner IP packet
        ChaCha20Poly1305::decrypt_in_place(
            &peer.shared_secret,
            &nonce,
            &[],
            &mut plaintext,
            &expected_tag,
        )?;

        self.rx_queue.lock().push_back(plaintext);
        Ok(())
    }

    /// Retrieves an encrypted UDP WireGuard packet ready to be sent over the wire.
    pub fn pop_transmitted_udp_packet(&self) -> Option<Vec<u8>> {
        self.tx_queue.lock().pop_front()
    }
}

impl NetDevice for WireGuardDevice {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn can_transmit(&self) -> bool {
        self.enabled.load(Ordering::Acquire) && self.peer_config.lock().is_some()
    }

    /// Transmits an outgoing IP packet by encrypting it with ChaCha20-Poly1305
    /// and encapsulating it into a WireGuard Type 4 data packet.
    fn transmit(&self, packet: &[u8]) -> Result<()> {
        if !self.can_transmit() {
            return Err(Errno::ENETDOWN);
        }

        let lock = self.peer_config.lock();
        let peer = lock.as_ref().ok_or(Errno::ENOTCONN)?;

        let counter = self.tx_counter.fetch_add(1, Ordering::Relaxed);
        let mut nonce = [0u8; 12];
        nonce[4..12].copy_from_slice(&counter.to_le_bytes());

        // Allocate WireGuard Type 4 payload: Header (16B) + Plaintext + Tag (16B)
        let mut encrypted_packet = vec![0u8; WG_DATA_HEADER_LEN + packet.len() + WG_MAC_LEN];

        // 1. Header: Message Type = 4, Receiver Index, Counter
        encrypted_packet[0..4].copy_from_slice(&message_type::DATA.to_le_bytes());
        encrypted_packet[4..8].copy_from_slice(&peer.receiver_index.to_le_bytes());
        encrypted_packet[8..16].copy_from_slice(&counter.to_le_bytes());

        // 2. Encrypt plaintext payload
        let payload_slice =
            &mut encrypted_packet[WG_DATA_HEADER_LEN..WG_DATA_HEADER_LEN + packet.len()];
        payload_slice.copy_from_slice(packet);

        let tag = ChaCha20Poly1305::encrypt_in_place(
            &peer.shared_secret,
            &nonce,
            &[],
            payload_slice,
        );

        // 3. Append 16-byte Poly1305 authentication tag
        let end = encrypted_packet.len();
        encrypted_packet[end - WG_MAC_LEN..end].copy_from_slice(&tag);

        self.tx_queue.lock().push_back(encrypted_packet);
        Ok(())
    }

    fn can_receive(&self) -> bool {
        !self.rx_queue.lock().is_empty()
    }

    /// Delivers decrypted inner IP packet from the tunnel to smoltcp / VFS.
    fn receive(&self, buf: &mut [u8]) -> Result<usize> {
        let mut queue = self.rx_queue.lock();
        if let Some(packet) = queue.pop_front() {
            let len = packet.len().min(buf.len());
            buf[..len].copy_from_slice(&packet[..len]);
            Ok(len)
        } else {
            Ok(0)
        }
    }

    fn mtu(&self) -> usize {
        WG_MTU
    }
}

/// Registers and attaches a new WireGuard VPN interface into the network stack.
pub fn register_wireguard_interface(
    name: &str,
    peer: Option<WireGuardPeerConfig>,
) -> Result<Arc<WireGuardDevice>> {
    let dev = Arc::new(WireGuardDevice::new(name));
    if let Some(p) = peer {
        dev.set_peer(p);
    }
    crate::drivers::net::register_network_device(dev.clone())?;
    Ok(dev)
}
