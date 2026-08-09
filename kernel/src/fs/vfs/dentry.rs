use super::types::Inode;
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::{Arc, Weak};

pub struct Dentry {
    pub name: String,
    pub inode: Arc<Inode>,
    pub parent: Spinlock<Option<Weak<Dentry>>>,
    pub children: Spinlock<BTreeMap<String, Arc<Dentry>>>,
}

impl Dentry {
    pub fn new(name: String, inode: Arc<Inode>) -> Self {
        Self {
            name,
            inode,
            parent: Spinlock::new(None),
            children: Spinlock::new(BTreeMap::new()),
        }
    }

    pub fn add_child(parent: &Arc<Self>, name: String, child_inode: Arc<Inode>) -> Arc<Self> {
        let child = Arc::new(Self {
            name: name.clone(),
            inode: child_inode,
            parent: Spinlock::new(Some(Arc::downgrade(parent))),
            children: Spinlock::new(BTreeMap::new()),
        });
        parent.children.lock().insert(name, child.clone());
        child
    }

    pub fn remove_child(parent: &Arc<Self>, name: &str) {
        parent.children.lock().remove(name);
    }
}

