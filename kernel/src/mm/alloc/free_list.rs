//! General Metadata-Indexed Free Lists.
//!
//! Provides reusable doubly-linked free lists whose nodes are stored as
//! 32-bit indices into a caller-owned metadata array, instead of intrusive
//! pointers inside free memory itself.
//!
//! This decouples list management from the contents of the managed memory,
//! preventing corruption from wild writes and invalid pointer dereferences.
//! The lists are fully generic: any node type implementing [`FreeListNode`]
//! can be tracked, and any number of lists can be grouped with
//! [`FreeListArray`].
//!
//! Typical use is one [`FreeList`] per buddy order, slab class, or zone:
//!
//! ```ignore
//! use crate::mm::alloc::free_list::{FreeListArray, FreeListNode};
//!
//! const ORDERS: usize = 21;
//! struct Meta { next: u32, prev: u32 }
//! impl FreeListNode for Meta { /* ... */ }
//!
//! let mut lists = FreeListArray::<ORDERS>::new();
//! let mut meta = [/* ... */];
//! lists.push(0, 42, &mut meta);
//! let head = lists.pop(0, &mut meta);
//! ```
//!
//! @author Ananta <kusumaananta042@gmail.com>

/// Sentinel representing a null/empty link. No valid node index may equal this.
pub const NULL_INDEX: u32 = u32::MAX;

/// Backwards-compatible alias for [`NULL_INDEX`].
/// Prefer `NULL_INDEX` in new generic code; `NO_FRAME` remains for frame allocators.
pub const NO_FRAME: u32 = NULL_INDEX;

/// Links required for a node to participate in a [`FreeList`].
///
/// Implementors expose the intrusive `next`/`prev` index fields stored in
/// the caller-owned metadata array. The [`FreeList`] manipulates nodes only
/// through this trait and never touches managed memory itself.
pub trait FreeListNode {
    /// Index of the next node in the list, or [`NULL_INDEX`] if none.
    fn next_link(&self) -> u32;
    /// Index of the previous node in the list, or [`NULL_INDEX`] if none.
    fn prev_link(&self) -> u32;
    /// Sets the next-node link.
    fn set_next_link(&mut self, next: u32);
    /// Sets the previous-node link.
    fn set_prev_link(&mut self, prev: u32);
    /// Clears both links to [`NULL_INDEX`].
    fn clear_links(&mut self) {
        self.set_next_link(NULL_INDEX);
        self.set_prev_link(NULL_INDEX);
    }
}

/// A single doubly-linked free list over indices into a metadata array.
#[derive(Debug, Clone, Copy)]
pub struct FreeList {
    head: u32,
}

impl FreeList {
    /// Creates an empty free list.
    pub const fn new() -> Self {
        Self { head: NULL_INDEX }
    }

    /// Resets the list to the empty state (does not touch nodes).
    pub fn clear(&mut self) {
        self.head = NULL_INDEX;
    }

    /// Returns the index at the head of the list, if any.
    #[inline(always)]
    pub fn head(&self) -> Option<usize> {
        if self.head != NULL_INDEX {
            Some(self.head as usize)
        } else {
            None
        }
    }

    /// Checks if the list is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.head == NULL_INDEX
    }

    /// Pushes node `idx` onto the head of the list.
    ///
    /// Silently ignores out-of-bounds indices so callers in early-boot or
    /// allocator fast paths never panic.
    pub fn push<T: FreeListNode>(&mut self, idx: usize, nodes: &mut [T]) {
        if idx >= nodes.len() {
            return;
        }

        let old_head = self.head;
        nodes[idx].set_next_link(old_head);
        nodes[idx].set_prev_link(NULL_INDEX);

        if old_head != NULL_INDEX && (old_head as usize) < nodes.len() {
            nodes[old_head as usize].set_prev_link(idx as u32);
        }

        self.head = idx as u32;
    }

    /// Pops the head node, returning its index.
    pub fn pop<T: FreeListNode>(&mut self, nodes: &mut [T]) -> Option<usize> {
        let head = self.head;
        if head == NULL_INDEX || (head as usize) >= nodes.len() {
            return None;
        }

        let head_idx = head as usize;
        let next = nodes[head_idx].next_link();

        if next != NULL_INDEX && (next as usize) < nodes.len() {
            nodes[next as usize].set_prev_link(NULL_INDEX);
        }

        self.head = next;
        nodes[head_idx].clear_links();

        Some(head_idx)
    }

    /// Removes an arbitrary node `idx` from the list.
    ///
    /// If `idx` is not the head and has no valid neighbours, the list head is
    /// left unchanged; the node's links are always cleared.
    pub fn remove<T: FreeListNode>(&mut self, idx: usize, nodes: &mut [T]) {
        if idx >= nodes.len() {
            return;
        }

        let prev = nodes[idx].prev_link();
        let next = nodes[idx].next_link();

        if prev != NULL_INDEX && (prev as usize) < nodes.len() {
            nodes[prev as usize].set_next_link(next);
        } else if self.head == idx as u32 {
            self.head = next;
        }

        if next != NULL_INDEX && (next as usize) < nodes.len() {
            nodes[next as usize].set_prev_link(prev);
        }

        nodes[idx].clear_links();
    }
}

impl Default for FreeList {
    fn default() -> Self {
        Self::new()
    }
}

/// A fixed-size array of [`FreeList`]s, e.g. one list per buddy order,
/// size class, or memory zone.
#[derive(Debug, Clone, Copy)]
pub struct FreeListArray<const N: usize> {
    lists: [FreeList; N],
}

impl<const N: usize> FreeListArray<N> {
    /// Creates an array of `N` empty free lists.
    pub const fn new() -> Self {
        Self {
            lists: [FreeList::new(); N],
        }
    }

    /// Resets all lists to the empty state.
    pub fn clear(&mut self) {
        for list in self.lists.iter_mut() {
            list.clear();
        }
    }

    /// Number of lists in the array.
    #[inline(always)]
    pub const fn len(&self) -> usize {
        N
    }

    /// Checks if the array holds zero lists.
    #[inline(always)]
    pub const fn is_empty_array(&self) -> bool {
        N == 0
    }

    /// Returns the head index of list `list_idx`, if any.
    #[inline(always)]
    pub fn head(&self, list_idx: usize) -> Option<usize> {
        if list_idx < N {
            self.lists[list_idx].head()
        } else {
            None
        }
    }

    /// Checks if the list `list_idx` is empty (out-of-range counts as empty).
    #[inline(always)]
    pub fn is_empty(&self, list_idx: usize) -> bool {
        list_idx >= N || self.lists[list_idx].is_empty()
    }

    /// Pushes node `idx` onto the head of list `list_idx`.
    pub fn push<T: FreeListNode>(&mut self, list_idx: usize, idx: usize, nodes: &mut [T]) {
        if list_idx >= N {
            return;
        }
        self.lists[list_idx].push(idx, nodes);
    }

    /// Pops the head node from list `list_idx`, returning its index.
    pub fn pop<T: FreeListNode>(&mut self, list_idx: usize, nodes: &mut [T]) -> Option<usize> {
        if list_idx >= N {
            return None;
        }
        self.lists[list_idx].pop(nodes)
    }

    /// Removes node `idx` from list `list_idx`.
    pub fn remove<T: FreeListNode>(&mut self, list_idx: usize, idx: usize, nodes: &mut [T]) {
        if list_idx >= N {
            return;
        }
        self.lists[list_idx].remove(idx, nodes);
    }
}

impl<const N: usize> Default for FreeListArray<N> {
    fn default() -> Self {
        Self::new()
    }
}
