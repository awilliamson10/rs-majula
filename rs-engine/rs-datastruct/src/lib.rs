mod hashtable;
mod linklist;

pub use hashtable::HashTable;
pub use linklist::LinkList;

/// A single node in an arena-based linked list. Stores an optional value and
/// indices into the arena `Vec` for the previous and next neighbors. Index 0
/// is reserved as the sentinel node whose `value` is always `None`.
///
/// # Fields
///
/// * `value` - The stored element, or `None` for the sentinel / freed slots.
/// * `prev` - Arena index of the previous node (0 = sentinel).
/// * `next` - Arena index of the next node (0 = sentinel).
pub(crate) struct Entry<T> {
    pub(crate) value: Option<T>,
    pub(crate) prev: usize,
    pub(crate) next: usize,
}
