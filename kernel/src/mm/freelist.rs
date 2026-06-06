#[derive(Copy, Clone)]
#[repr(C)]
pub struct IntrusiveNode {
    pub next: *mut IntrusiveNode,
    pub prev: *mut IntrusiveNode,
}

unsafe impl Send for IntrusiveNode {}
unsafe impl Sync for IntrusiveNode {}

impl IntrusiveNode {
    pub const fn new() -> Self {
        Self {
            next: core::ptr::null_mut(),
            prev: core::ptr::null_mut(),
        }
    }
}

#[derive(Copy, Clone)]
pub struct IntrusiveList {
    pub head: *mut IntrusiveNode,
}

unsafe impl Send for IntrusiveList {}
unsafe impl Sync for IntrusiveList {}

impl IntrusiveList {
    pub const fn new() -> Self {
        Self {
            head: core::ptr::null_mut(),
        }
    }

    /// Prepend a node to the front of the list.
    ///
    /// # Safety
    /// Caller must guarantee that `node` is a valid, exclusive pointer to an `IntrusiveNode`
    /// and that it is not already inserted in another list.
    pub unsafe fn push_front(&mut self, node: *mut IntrusiveNode) {
        if node.is_null() {
            return;
        }
        unsafe {
            (*node).next = self.head;
            (*node).prev = core::ptr::null_mut();
            if !self.head.is_null() {
                (*self.head).prev = node;
            }
            self.head = node;
        }
    }

    /// Remove a node from this list.
    ///
    /// # Safety
    /// Caller must guarantee that `node` is currently a member of this list.
    pub unsafe fn remove(&mut self, node: *mut IntrusiveNode) {
        if node.is_null() {
            return;
        }
        unsafe {
            let prev = (*node).prev;
            let next = (*node).next;

            if !prev.is_null() {
                (*prev).next = next;
            } else {
                self.head = next;
            }

            if !next.is_null() {
                (*next).prev = prev;
            }

            (*node).next = core::ptr::null_mut();
            (*node).prev = core::ptr::null_mut();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.head.is_null()
    }
}
