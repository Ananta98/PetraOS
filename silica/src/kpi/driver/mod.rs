// SPDX-License-Identifier: GPL-2.0

//! Driver class abstractions: character devices (`cdev`) and block devices (`gendisk`).

pub mod block;
pub mod char;

pub use block::{add_disk, del_gendisk, BlockDeviceOperations, GenDisk};
pub use char::{alloc_chrdev_region, cdev_add, cdev_del, CDev, FileOperations};
