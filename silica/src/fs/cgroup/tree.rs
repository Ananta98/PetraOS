// SPDX-License-Identifier: GPL-2.0

//! In-memory state and tree representations for cgroup v2.
//!
//! Maintains hierarchical cgroup nodes, controller enablement,
//! process membership sets, and population events.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::String,
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::{Once, RwLock};

use crate::{
    proc::{Pid, allproc_find, allproc_iter},
    uapi::errno::{Errno, Result},
};

static NEXT_CGROUP_INO: AtomicUsize = AtomicUsize::new(50_000);

/// Allocates unique inode number for cgroup nodes and files.
pub fn alloc_cgroup_ino() -> usize {
    NEXT_CGROUP_INO.fetch_add(1, Ordering::Relaxed)
}

/// A node in the cgroup v2 hierarchy.
pub struct CgroupNode {
    pub name: String,
    pub ino: usize,
    pub parent: Option<Weak<CgroupNode>>,
    pub inner: RwLock<CgroupNodeInner>,
}

pub struct CgroupNodeInner {
    pub subdirs: BTreeMap<String, Arc<CgroupNode>>,
    pub pids: BTreeSet<Pid>,
    pub subtree_control: BTreeSet<String>,
    pub max_descendants: Option<usize>,
    pub max_depth: Option<usize>,
    pub cpu_weight: usize,
    pub cpu_max: (String, u64), // (quota_str, period_us)
    pub memory_max: String,
    pub memory_high: String,
    pub pids_max: String,
    pub frozen: bool,
}

impl CgroupNode {
    pub fn new_root() -> Arc<Self> {
        let ino = alloc_cgroup_ino();
        let node = Arc::new(Self {
            name: String::new(),
            ino,
            parent: None,
            inner: RwLock::new(CgroupNodeInner {
                subdirs: BTreeMap::new(),
                pids: BTreeSet::new(),
                subtree_control: BTreeSet::new(),
                max_descendants: None,
                max_depth: None,
                cpu_weight: 100,
                cpu_max: (String::from("max"), 100000),
                memory_max: String::from("max"),
                memory_high: String::from("max"),
                pids_max: String::from("max"),
                frozen: false,
            }),
        });
        node
    }

    pub fn new_child(name: &str, parent: Arc<CgroupNode>) -> Arc<Self> {
        let ino = alloc_cgroup_ino();
        Arc::new(Self {
            name: String::from(name),
            ino,
            parent: Some(Arc::downgrade(&parent)),
            inner: RwLock::new(CgroupNodeInner {
                subdirs: BTreeMap::new(),
                pids: BTreeSet::new(),
                subtree_control: BTreeSet::new(),
                max_descendants: None,
                max_depth: None,
                cpu_weight: 100,
                cpu_max: (String::from("max"), 100000),
                memory_max: String::from("max"),
                memory_high: String::from("max"),
                pids_max: String::from("max"),
                frozen: false,
            }),
        })
    }

    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    /// Computes cgroup path relative to root, e.g. "/" or "/system.slice/foo".
    pub fn cgroup_path(&self) -> String {
        let mut path_parts = Vec::new();

        // Walk up to root
        let mut curr_node: Option<Arc<CgroupNode>> = None;
        if let Some(parent) = &self.parent {
            if let Some(p) = parent.upgrade() {
                curr_node = Some(p);
            }
        }

        path_parts.push(self.name.clone());

        while let Some(node) = curr_node {
            if !node.name.is_empty() {
                path_parts.push(node.name.clone());
            }
            let next = if let Some(parent) = &node.parent {
                parent.upgrade()
            } else {
                None
            };
            curr_node = next;
        }

        path_parts.reverse();
        let mut result = String::new();
        for part in path_parts.iter() {
            if !part.is_empty() {
                result.push('/');
                result.push_str(part);
            }
        }
        if result.is_empty() {
            result.push('/');
        }
        result
    }

    /// Returns available controllers for this cgroup.
    /// In cgroup v2, root provides all enabled controllers ("cpu memory io pids").
    /// Sub-cgroups provide whatever their parent enabled in `cgroup.subtree_control`.
    pub fn controllers(&self) -> Vec<&'static str> {
        if self.is_root() {
            vec!["cpu", "memory", "io", "pids"]
        } else if let Some(weak_p) = &self.parent {
            if let Some(parent) = weak_p.upgrade() {
                let p_inner = parent.inner.read();
                let mut v = Vec::new();
                for ctrl in &["cpu", "memory", "io", "pids"] {
                    if p_inner.subtree_control.contains(*ctrl) {
                        v.push(*ctrl);
                    }
                }
                v
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    }

    /// Checks if a controller is effectively enabled for this cgroup.
    pub fn is_controller_enabled(&self, ctrl: &str) -> bool {
        if self.is_root() {
            true
        } else if let Some(weak_p) = &self.parent {
            if let Some(parent) = weak_p.upgrade() {
                parent.inner.read().subtree_control.contains(ctrl)
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Count total processes in this cgroup and its descendants.
    pub fn populated(&self) -> bool {
        let inner = self.inner.read();
        if self.is_root() {
            // Root contains all unassigned processes, so check if any live process exists
            !allproc_iter().is_empty()
        } else {
            if !inner.pids.is_empty() {
                return true;
            }
            for sub in inner.subdirs.values() {
                if sub.populated() {
                    return true;
                }
            }
            false
        }
    }

    /// Populated status of directly attached processes.
    pub fn nr_populated_cgroups(&self) -> usize {
        let inner = self.inner.read();
        let mut count = 0;
        for sub in inner.subdirs.values() {
            if sub.populated() {
                count += 1;
            }
        }
        count
    }

    /// Generates `cgroup.procs` content.
    pub fn read_procs(&self) -> Vec<u8> {
        let mut pids_to_list = Vec::new();
        if self.is_root() {
            // In cgroup v2, every process in the system not assigned to a sub-cgroup
            // is in the root cgroup.
            let tree = default_cgroup_tree();
            let assigned_pids = tree.all_assigned_subgroup_pids();
            for proc in allproc_iter() {
                if !assigned_pids.contains(&proc.pid) {
                    pids_to_list.push(proc.pid.as_u32());
                }
            }
        } else {
            let inner = self.inner.read();
            for pid in &inner.pids {
                // Verify process still exists
                if allproc_find(*pid).is_some() {
                    pids_to_list.push(pid.as_u32());
                }
            }
        }

        pids_to_list.sort_unstable();
        let mut out = String::new();
        for pid in pids_to_list {
            out.push_str(&format!("{}\n", pid));
        }
        out.into_bytes()
    }

    /// Migrates a process into this cgroup.
    pub fn write_procs(&self, buf: &[u8]) -> Result<usize> {
        let s = core::str::from_utf8(buf).map_err(|_| Errno::EINVAL)?;
        let trimmed = s.trim();
        let pid_num: u32 = trimmed.parse().map_err(|_| Errno::EINVAL)?;
        let pid = Pid::from_u32(pid_num).ok_or(Errno::ESRCH)?;

        // Verify pid exists
        if allproc_find(pid).is_none() {
            return Err(Errno::ESRCH);
        }

        let tree = default_cgroup_tree();
        tree.move_process_to(pid, self)?;
        Ok(buf.len())
    }
}

/// Global cgroup tree registry.
pub struct CgroupTree {
    pub root: Arc<CgroupNode>,
}

impl CgroupTree {
    pub fn new() -> Self {
        Self {
            root: CgroupNode::new_root(),
        }
    }

    /// Gathers all PIDs that are assigned to any descendant sub-cgroup.
    pub fn all_assigned_subgroup_pids(&self) -> BTreeSet<Pid> {
        let mut set = BTreeSet::new();
        Self::collect_subgroup_pids(&self.root, &mut set);
        set
    }

    fn collect_subgroup_pids(node: &Arc<CgroupNode>, set: &mut BTreeSet<Pid>) {
        let inner = node.inner.read();
        for sub in inner.subdirs.values() {
            let sub_inner = sub.inner.read();
            for pid in &sub_inner.pids {
                set.insert(*pid);
            }
            drop(sub_inner);
            Self::collect_subgroup_pids(sub, set);
        }
    }

    /// Moves a process from its current cgroup to `target`.
    pub fn move_process_to(&self, pid: Pid, target: &CgroupNode) -> Result<()> {
        // If target is root, just remove pid from all sub-cgroups
        Self::remove_pid_recursive(&self.root, pid);
        if !target.is_root() {
            target.inner.write().pids.insert(pid);
        }
        Ok(())
    }

    fn remove_pid_recursive(node: &Arc<CgroupNode>, pid: Pid) {
        let mut inner = node.inner.write();
        inner.pids.remove(&pid);
        for sub in inner.subdirs.values() {
            Self::remove_pid_recursive(sub, pid);
        }
    }

    /// Finds which cgroup a process currently belongs to. Returns path string.
    pub fn find_proc_cgroup(&self, pid: Pid) -> String {
        if let Some(path) = Self::find_pid_in_node(&self.root, pid) {
            path
        } else {
            String::from("/")
        }
    }

    fn find_pid_in_node(node: &Arc<CgroupNode>, pid: Pid) -> Option<String> {
        let inner = node.inner.read();
        for sub in inner.subdirs.values() {
            let sub_inner = sub.inner.read();
            if sub_inner.pids.contains(&pid) {
                return Some(sub.cgroup_path());
            }
            drop(sub_inner);
            if let Some(path) = Self::find_pid_in_node(sub, pid) {
                return Some(path);
            }
        }
        None
    }
}

static CGROUP_TREE: Once<CgroupTree> = Once::new();

/// Returns the singleton cgroup v2 tree.
pub fn default_cgroup_tree() -> &'static CgroupTree {
    CGROUP_TREE.call_once(CgroupTree::new)
}
