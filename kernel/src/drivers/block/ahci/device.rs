use super::command;
/// AHCI Block Device
///
/// Provides the `AhciBlockDevice` type that implements the `BlockDevice` trait,
/// supporting both physical SATA drives and a simulated fallback device.
use crate::drivers::block::BlockDevice;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::io::IoMem;
use ostd::mm::VmIo;
use ostd::mm::dma::DmaCoherent;
use ostd::sync::SpinLock;

/// An AHCI block device that can be backed by real hardware or simulated storage.
pub struct AhciBlockDevice {
    pub(super) name: String,
    pub(super) inner: SpinLock<AhciBlockDeviceInner>,
}

/// The inner state of an AHCI block device.
pub enum AhciBlockDeviceInner {
    /// A physical SATA device accessed through the AHCI HBA.
    Physical {
        abar: IoMem,
        port_no: usize,
        block_size: usize,
        num_blocks: usize,
        cmd_list: DmaCoherent,
        fis: DmaCoherent,
        cmd_table: DmaCoherent,
        dma_buf: DmaCoherent,
    },
    /// A simulated in-memory block device for testing when no hardware is present.
    Simulated { data: Vec<u8> },
}

impl BlockDevice for AhciBlockDevice {
    fn block_size(&self) -> usize {
        512
    }

    fn num_blocks(&self) -> usize {
        match &*self.inner.lock() {
            AhciBlockDeviceInner::Physical { num_blocks, .. } => *num_blocks,
            AhciBlockDeviceInner::Simulated { data } => data.len() / 512,
        }
    }

    fn read_blocks(&self, block_id: usize, buf: &mut [u8]) -> Result<(), ostd::Error> {
        assert_eq!(buf.len(), 512);
        let mut inner = self.inner.lock();
        match &mut *inner {
            AhciBlockDeviceInner::Physical {
                abar,
                port_no,
                cmd_list,
                fis: _,
                cmd_table,
                dma_buf,
                ..
            } => {
                command::send_command(
                    abar,
                    *port_no,
                    cmd_list,
                    cmd_table,
                    dma_buf,
                    false,
                    block_id as u64,
                    1,
                )?;
                dma_buf.read_bytes(0, buf)?;
            }
            AhciBlockDeviceInner::Simulated { data } => {
                let offset = block_id * 512;
                if offset + 512 > data.len() {
                    return Err(ostd::Error::InvalidArgs);
                }
                buf.copy_from_slice(&data[offset..offset + 512]);
            }
        }
        Ok(())
    }

    fn write_blocks(&self, block_id: usize, buf: &[u8]) -> Result<(), ostd::Error> {
        assert_eq!(buf.len(), 512);
        let mut inner = self.inner.lock();
        match &mut *inner {
            AhciBlockDeviceInner::Physical {
                abar,
                port_no,
                cmd_list,
                fis: _,
                cmd_table,
                dma_buf,
                ..
            } => {
                dma_buf.write_bytes(0, buf)?;
                command::send_command(
                    abar,
                    *port_no,
                    cmd_list,
                    cmd_table,
                    dma_buf,
                    true,
                    block_id as u64,
                    1,
                )?;
            }
            AhciBlockDeviceInner::Simulated { data } => {
                let offset = block_id * 512;
                if offset + 512 > data.len() {
                    return Err(ostd::Error::InvalidArgs);
                }
                data[offset..offset + 512].copy_from_slice(buf);
            }
        }
        Ok(())
    }
}
