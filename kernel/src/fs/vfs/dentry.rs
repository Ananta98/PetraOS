use super::types::Inode;
use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

pub struct Dentry {
    pub name: String,
    pub inode: Arc<Inode>,
    pub parent: Mutex<Option<Weak<Dentry>>>,
    pub children: Mutex<BTreeMap<String, Arc<Dentry>>>,
}

impl Dentry {
    pub fn new(name: String, inode: Arc<Inode>) -> Self {
        Self {
            name,
            inode,
            parent: Mutex::new(None),
            children: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn add_child(parent: &Arc<Self>, name: String, child_inode: Arc<Inode>) -> Arc<Self> {
        let child = Arc::new(Self {
            name: name.clone(),
            inode: child_inode,
            parent: Mutex::new(Some(Arc::downgrade(parent))),
            children: Mutex::new(BTreeMap::new()),
        });
        parent.children.lock().insert(name, child.clone());
        child
    }

    pub fn remove_child(parent: &Arc<Self>, name: &str) {
        parent.children.lock().remove(name);
    }

    /// Compute the absolute path from root dentry down to this node.
    pub fn full_path(&self) -> String {
        let mut components = Vec::new();
        let mut current_name = self.name.clone();
        let mut current_parent = self.parent.lock().clone();

        while let Some(weak_parent) = current_parent {
            if let Some(parent_arc) = weak_parent.upgrade() {
                if !current_name.is_empty() && current_name != "/" {
                    components.push(current_name);
                }
                current_name = parent_arc.name.clone();
                current_parent = parent_arc.parent.lock().clone();
            } else {
                break;
            }
        }

        if components.is_empty() {
            return String::from("/");
        }

        components.reverse();
        let mut path = String::new();
        for comp in components {
            path.push('/');
            path.push_str(&comp);
        }
        path
    }
}
