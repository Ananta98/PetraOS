use alloc::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub suid: u32,
    pub sgid: u32,
    pub fsuid: u32,
    pub fsgid: u32,
}

impl Credentials {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            uid: 0,
            gid: 0,
            euid: 0,
            egid: 0,
            suid: 0,
            sgid: 0,
            fsuid: 0,
            fsgid: 0,
        })
    }
}
