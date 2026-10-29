// SPDX-License-Identifier: GPL-2.0

//! VFS NodeOps and FileOps implementations for cgroup v2.
//!
//! Provides directory nodes (with `mkdir`, `rmdir`, `lookup`) and synthetic control file
//! nodes implementing Linux cgroup v2 semantics (`cgroup.controllers`, `cgroup.procs`,
//! `cgroup.subtree_control`, `cgroup.events`, `cpu.max`, `memory.max`, etc.).

use alloc::{
    boxed::Box,
    format,
    string::String,
    sync::Arc,
    vec,
    vec::Vec,
};

use crate::{
    fs::{
        cgroup::tree::{CgroupNode, alloc_cgroup_ino},
        template::{BufFileOps, WriteHook, dir_mode, ro_file_mode, rw_file_mode},
        vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
    uapi::errno::{Errno, Result},
};

/// VFS NodeOps for a cgroup directory.
pub struct CgroupDirNodeOps {
    cgroup: Arc<CgroupNode>,
}

impl CgroupDirNodeOps {
    pub fn new(cgroup: Arc<CgroupNode>) -> Self {
        Self { cgroup }
    }
}

impl NodeOps for CgroupDirNodeOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        // First check if it matches an existing sub-cgroup directory
        {
            let inner = self.cgroup.inner.read();
            if let Some(sub) = inner.subdirs.get(name) {
                return Ok(cgroup_dir_to_inode(Arc::clone(sub)));
            }
        }

        // Otherwise check control files
        if is_cgroup_control_file(name, &self.cgroup) {
            return Ok(cgroup_control_file_to_inode(Arc::clone(&self.cgroup), name));
        }

        Err(Errno::ENOENT)
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        let mut entries = Vec::new();
        // Collect sub-cgroup directories
        {
            let inner = self.cgroup.inner.read();
            for name in inner.subdirs.keys() {
                entries.push(name.clone());
            }
        }
        // Collect standard control files
        for f in list_cgroup_control_files(&self.cgroup) {
            entries.push(String::from(f));
        }

        Ok(Box::new(CgroupDirFileOps { entries }))
    }

    fn mkdir(&self, name: &str, _mode: Mode) -> Result<Arc<INode>> {
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(Errno::EINVAL);
        }

        // Cannot create directory with name of a control file
        if is_cgroup_control_file(name, &self.cgroup) {
            return Err(Errno::EEXIST);
        }

        let mut inner = self.cgroup.inner.write();
        if inner.subdirs.contains_key(name) {
            return Err(Errno::EEXIST);
        }

        if let Some(max_desc) = inner.max_descendants {
            if inner.subdirs.len() >= max_desc {
                return Err(Errno::EAGAIN);
            }
        }

        let child = CgroupNode::new_child(name, Arc::clone(&self.cgroup));
        inner.subdirs.insert(String::from(name), Arc::clone(&child));
        drop(inner);

        Ok(cgroup_dir_to_inode(child))
    }

    fn rmdir(&self, name: &str) -> Result<()> {
        let mut inner = self.cgroup.inner.write();
        let sub = inner.subdirs.get(name).ok_or(Errno::ENOENT)?;

        // In Linux cgroup v2, a cgroup cannot be deleted if it has processes
        // or sub-cgroups.
        let sub_inner = sub.inner.read();
        if !sub_inner.pids.is_empty() || !sub_inner.subdirs.is_empty() {
            return Err(Errno::EBUSY);
        }
        drop(sub_inner);

        inner.subdirs.remove(name);
        Ok(())
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            mode: dir_mode(),
            nlink: 2,
            ..Default::default()
        })
    }
}

pub struct CgroupDirFileOps {
    entries: Vec<String>,
}

impl FileOps for CgroupDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::from(b".\n..\n");
        for name in &self.entries {
            out.extend_from_slice(name.as_bytes());
            out.push(b'\n');
        }
        Ok(out)
    }
}

/// Helper converting a CgroupNode into its VFS INode directory representation.
pub fn cgroup_dir_to_inode(cgroup: Arc<CgroupNode>) -> Arc<INode> {
    let ino = cgroup.ino;
    Arc::new(INode::new_with_attr(
        Box::new(CgroupDirNodeOps::new(cgroup)),
        ino,
        INodeAttr {
            mode: dir_mode(),
            nlink: 2,
            ..Default::default()
        },
    ))
}

fn is_cgroup_control_file(name: &str, cgroup: &CgroupNode) -> bool {
    let files = list_cgroup_control_files(cgroup);
    files.iter().any(|f| *f == name)
}

fn list_cgroup_control_files(cgroup: &CgroupNode) -> Vec<&'static str> {
    let mut files = vec![
        "cgroup.controllers",
        "cgroup.subtree_control",
        "cgroup.procs",
        "cgroup.threads",
        "cgroup.events",
        "cgroup.type",
        "cgroup.stat",
        "cgroup.max.descendants",
        "cgroup.max.depth",
        "cgroup.freeze",
    ];

    if cgroup.is_controller_enabled("cpu") {
        files.extend_from_slice(&["cpu.weight", "cpu.max", "cpu.stat"]);
    }
    if cgroup.is_controller_enabled("memory") {
        files.extend_from_slice(&["memory.current", "memory.min", "memory.low", "memory.high", "memory.max", "memory.stat", "memory.events"]);
    }
    if cgroup.is_controller_enabled("pids") {
        files.extend_from_slice(&["pids.current", "pids.max", "pids.events"]);
    }
    if cgroup.is_controller_enabled("io") {
        files.extend_from_slice(&["io.stat", "io.max", "io.weight"]);
    }

    files
}

/// VFS NodeOps for cgroup synthetic control files.
pub struct CgroupFileNodeOps {
    cgroup: Arc<CgroupNode>,
    filename: String,
}

impl NodeOps for CgroupFileNodeOps {
    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        let is_write = flags.contains(OpenFlags::WRITE);
        let writable = is_control_file_writable(&self.filename);

        if is_write && !writable {
            crate::return_errno!(EPERM, "control file is read-only");
        }

        let content = generate_control_file_content(&self.cgroup, &self.filename)?;

        if writable {
            let cgroup = Arc::clone(&self.cgroup);
            let filename = self.filename.clone();
            let hook = Box::new(move |buf: &[u8]| {
                handle_control_file_write(&cgroup, &filename, buf)
            }) as WriteHook;
            Ok(Box::new(BufFileOps::with_write_hook(content, hook)))
        } else {
            Ok(Box::new(BufFileOps::new(content)))
        }
    }

    fn getattr(&self) -> Result<INodeAttr> {
        let writable = is_control_file_writable(&self.filename);
        let mode = if writable { rw_file_mode() } else { ro_file_mode() };
        Ok(INodeAttr {
            size: 0,
            mode,
            nlink: 1,
            ..Default::default()
        })
    }
}

pub fn cgroup_control_file_to_inode(cgroup: Arc<CgroupNode>, filename: &str) -> Arc<INode> {
    let ino = alloc_cgroup_ino();
    let writable = is_control_file_writable(filename);
    let mode = if writable { rw_file_mode() } else { ro_file_mode() };
    Arc::new(INode::new_with_attr(
        Box::new(CgroupFileNodeOps {
            cgroup,
            filename: String::from(filename),
        }),
        ino,
        INodeAttr {
            size: 0,
            mode,
            nlink: 1,
            ..Default::default()
        },
    ))
}

fn is_control_file_writable(filename: &str) -> bool {
    matches!(
        filename,
        "cgroup.subtree_control"
            | "cgroup.procs"
            | "cgroup.threads"
            | "cgroup.type"
            | "cgroup.max.descendants"
            | "cgroup.max.depth"
            | "cgroup.freeze"
            | "cpu.weight"
            | "cpu.max"
            | "memory.min"
            | "memory.low"
            | "memory.high"
            | "memory.max"
            | "pids.max"
            | "io.max"
            | "io.weight"
    )
}

fn generate_control_file_content(cgroup: &Arc<CgroupNode>, filename: &str) -> Result<Vec<u8>> {
    match filename {
        "cgroup.controllers" => {
            let ctrls = cgroup.controllers();
            let mut s = ctrls.join(" ");
            if !s.is_empty() {
                s.push('\n');
            }
            Ok(s.into_bytes())
        }
        "cgroup.subtree_control" => {
            let inner = cgroup.inner.read();
            let mut items: Vec<&str> = inner.subtree_control.iter().map(|s| s.as_str()).collect();
            items.sort_unstable();
            let mut s = items.join(" ");
            if !s.is_empty() {
                s.push('\n');
            }
            Ok(s.into_bytes())
        }
        "cgroup.procs" => Ok(cgroup.read_procs()),
        "cgroup.threads" => Ok(cgroup.read_procs()), // 1:1 process/thread model currently
        "cgroup.events" => {
            let pop = if cgroup.populated() { 1 } else { 0 };
            let frozen = if cgroup.inner.read().frozen { 1 } else { 0 };
            Ok(format!("populated {}\nfrozen {}\n", pop, frozen).into_bytes())
        }
        "cgroup.type" => Ok(b"domain\n".to_vec()),
        "cgroup.stat" => {
            let inner = cgroup.inner.read();
            let nr_desc = inner.subdirs.len();
            let nr_pop = cgroup.nr_populated_cgroups();
            Ok(format!("nr_descendants {}\nnr_dying_descendants 0\nnr_populated_cgroups {}\n", nr_desc, nr_pop).into_bytes())
        }
        "cgroup.max.descendants" => {
            let inner = cgroup.inner.read();
            if let Some(m) = inner.max_descendants {
                Ok(format!("{}\n", m).into_bytes())
            } else {
                Ok(b"max\n".to_vec())
            }
        }
        "cgroup.max.depth" => {
            let inner = cgroup.inner.read();
            if let Some(d) = inner.max_depth {
                Ok(format!("{}\n", d).into_bytes())
            } else {
                Ok(b"max\n".to_vec())
            }
        }
        "cgroup.freeze" => {
            let inner = cgroup.inner.read();
            let v = if inner.frozen { 1 } else { 0 };
            Ok(format!("{}\n", v).into_bytes())
        }
        "cpu.weight" => {
            let inner = cgroup.inner.read();
            Ok(format!("{}\n", inner.cpu_weight).into_bytes())
        }
        "cpu.max" => {
            let inner = cgroup.inner.read();
            Ok(format!("{} {}\n", inner.cpu_max.0, inner.cpu_max.1).into_bytes())
        }
        "cpu.stat" => {
            Ok(b"usage_usec 0\nuser_usec 0\nsystem_usec 0\nnr_periods 0\nnr_throttled 0\nthrottled_usec 0\n".to_vec())
        }
        "memory.current" => {
            // Rough estimation of memory usage from vmspace if root, or 0
            Ok(b"0\n".to_vec())
        }
        "memory.min" => Ok(b"0\n".to_vec()),
        "memory.low" => Ok(b"0\n".to_vec()),
        "memory.high" => {
            let inner = cgroup.inner.read();
            Ok(format!("{}\n", inner.memory_high).into_bytes())
        }
        "memory.max" => {
            let inner = cgroup.inner.read();
            Ok(format!("{}\n", inner.memory_max).into_bytes())
        }
        "memory.stat" => {
            Ok(b"anon 0\nfile 0\nkernel_stack 0\npagetables 0\npercpu 0\nsock 0\nshmem 0\nfile_mapped 0\nfile_dirty 0\nfile_writeback 0\nswapcached 0\nanon_thp 0\nfile_thp 0\nshmem_thp 0\ninactive_anon 0\nactive_anon 0\ninactive_file 0\nactive_file 0\nunevictable 0\nslob_reclaimable 0\nslob_unreclaimable 0\npgfault 0\npgmajfault 0\nworkingset_refault 0\nworkingset_activate 0\nworkingset_nodereclaim 0\npgrefill 0\npgscan 0\npgsteal 0\npgactivate 0\npgdeactivate 0\nlazyfree 0\nlazyfreed 0\nthp_fault_alloc 0\nthp_collapse_alloc 0\n".to_vec())
        }
        "memory.events" => {
            Ok(b"low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\n".to_vec())
        }
        "pids.current" => {
            let inner = cgroup.inner.read();
            let count = if cgroup.is_root() {
                crate::proc::allproc_count()
            } else {
                inner.pids.len()
            };
            Ok(format!("{}\n", count).into_bytes())
        }
        "pids.max" => {
            let inner = cgroup.inner.read();
            Ok(format!("{}\n", inner.pids_max).into_bytes())
        }
        "pids.events" => {
            Ok(b"max 0\n".to_vec())
        }
        "io.stat" => Ok(Vec::new()),
        "io.max" => Ok(b"default\n".to_vec()),
        "io.weight" => Ok(b"default 100\n".to_vec()),
        _ => Err(Errno::ENOENT),
    }
}

fn handle_control_file_write(cgroup: &Arc<CgroupNode>, filename: &str, buf: &[u8]) -> Result<usize> {
    let s = core::str::from_utf8(buf).map_err(|_| Errno::EINVAL)?;
    let trimmed = s.trim();

    match filename {
        "cgroup.procs" | "cgroup.threads" => {
            cgroup.write_procs(buf)
        }
        "cgroup.subtree_control" => {
            // Linux format: "+cpu +memory -pids"
            let available = cgroup.controllers();
            let mut inner = cgroup.inner.write();
            for token in trimmed.split_whitespace() {
                if let Some(ctrl) = token.strip_prefix('+') {
                    if !available.contains(&ctrl) {
                        return Err(Errno::ENOENT);
                    }
                    inner.subtree_control.insert(String::from(ctrl));
                } else if let Some(ctrl) = token.strip_prefix('-') {
                    inner.subtree_control.remove(ctrl);
                } else {
                    return Err(Errno::EINVAL);
                }
            }
            Ok(buf.len())
        }
        "cgroup.max.descendants" => {
            let mut inner = cgroup.inner.write();
            if trimmed == "max" {
                inner.max_descendants = None;
            } else {
                let val: usize = trimmed.parse().map_err(|_| Errno::EINVAL)?;
                inner.max_descendants = Some(val);
            }
            Ok(buf.len())
        }
        "cgroup.max.depth" => {
            let mut inner = cgroup.inner.write();
            if trimmed == "max" {
                inner.max_depth = None;
            } else {
                let val: usize = trimmed.parse().map_err(|_| Errno::EINVAL)?;
                inner.max_depth = Some(val);
            }
            Ok(buf.len())
        }
        "cgroup.freeze" => {
            let mut inner = cgroup.inner.write();
            inner.frozen = trimmed == "1";
            Ok(buf.len())
        }
        "cpu.weight" => {
            let val: usize = trimmed.parse().map_err(|_| Errno::EINVAL)?;
            if !(1..=10000).contains(&val) {
                return Err(Errno::EINVAL);
            }
            cgroup.inner.write().cpu_weight = val;
            Ok(buf.len())
        }
        "cpu.max" => {
            let mut parts = trimmed.split_whitespace();
            let quota = parts.next().ok_or(Errno::EINVAL)?;
            let period = if let Some(p) = parts.next() {
                p.parse::<u64>().map_err(|_| Errno::EINVAL)?
            } else {
                100000
            };
            cgroup.inner.write().cpu_max = (String::from(quota), period);
            Ok(buf.len())
        }
        "memory.high" => {
            cgroup.inner.write().memory_high = String::from(trimmed);
            Ok(buf.len())
        }
        "memory.max" => {
            cgroup.inner.write().memory_max = String::from(trimmed);
            Ok(buf.len())
        }
        "pids.max" => {
            cgroup.inner.write().pids_max = String::from(trimmed);
            Ok(buf.len())
        }
        "cgroup.type" | "memory.min" | "memory.low" | "io.max" | "io.weight" => {
            // Accepted no-op for compatibility
            Ok(buf.len())
        }
        _ => Err(Errno::EPERM),
    }
}
