use crate::state::ScriptState;
use rs_pack::cache::script::LAST;

/// A script opcode handler function pointer.
///
/// Each handler receives a mutable reference to the current [`ScriptState`] and
/// executes the logic for a single VM opcode. Returns `Ok(())` on success or a
/// [`ScriptError`](crate::ScriptError) on failure.
pub type Handler = fn(&mut ScriptState) -> crate::Result<()>;

/// A fixed-size lookup table mapping script opcodes to their [`Handler`] functions.
///
/// Internally stores a boxed array of `Option<Handler>` with `LAST` slots (one per
/// possible opcode), plus a count of how many slots are populated. Created during
/// engine initialization by `register_ops` and consumed by `vm::execute` to dispatch
/// opcodes at runtime.
///
/// # Call Stack
/// **Called by:** `Engine::new` via `register_ops` (rs-engine/src/engine.rs)
/// **Used by:** `vm::execute` (rs-vm/src/vm.rs)
pub struct OpsRegistry {
    table: Box<[Option<Handler>; LAST as usize]>,
    count: usize,
}

impl OpsRegistry {
    /// Creates a new empty `OpsRegistry` with all opcode slots set to `None`.
    ///
    /// # Returns
    /// An `OpsRegistry` with zero registered handlers and a table of `LAST` empty slots.
    pub fn new() -> Self {
        Self {
            table: Box::new([const { None }; LAST as usize]),
            count: 0,
        }
    }

    /// Registers a handler function for the given opcode.
    ///
    /// If the opcode slot was previously empty, the internal count is incremented.
    /// If a handler was already registered for this opcode, it is silently replaced
    /// without changing the count.
    ///
    /// # Arguments
    /// * `opcode` - The opcode number to register. Must be less than `LAST`.
    /// * `handler` - The function pointer to invoke when this opcode is executed.
    ///
    /// # Panics
    /// Panics if `opcode as usize` is out of bounds for the internal table.
    pub(crate) fn insert(&mut self, opcode: u16, handler: Handler) {
        let slot = &mut self.table[opcode as usize];
        if slot.is_none() {
            self.count += 1;
        }
        *slot = Some(handler);
    }

    /// Merges all registered handlers from another `OpsRegistry` into this one.
    ///
    /// For each opcode that has a handler in `other`, the handler is copied into
    /// `self`. If `self` already had a handler for that opcode, it is replaced
    /// without double-counting. Opcodes not registered in `other` are left unchanged.
    ///
    /// # Arguments
    /// * `other` - The source registry to merge from. Consumed by this call.
    ///
    /// # Side Effects
    /// Updates the internal handler count to reflect newly added (not replaced) entries.
    pub fn extend(&mut self, other: OpsRegistry) {
        for (i, slot) in other.table.into_iter().enumerate() {
            if let Some(handler) = slot {
                if self.table[i].is_none() {
                    self.count += 1;
                }
                self.table[i] = Some(handler);
            }
        }
    }

    /// Looks up the handler registered for the given opcode.
    ///
    /// # Arguments
    /// * `opcode` - The opcode to look up. Must be less than `LAST`.
    ///
    /// # Returns
    /// `Some(handler)` if a handler is registered for this opcode, or `None` if the
    /// slot is empty.
    ///
    /// # Safety
    /// Uses `get_unchecked` internally for performance -- the caller must ensure
    /// `opcode` is within bounds (less than `LAST`). An out-of-bounds opcode causes
    /// undefined behavior.
    ///
    /// # Call Stack
    /// **Called by:** `vm::execute` during opcode dispatch
    #[inline(always)]
    pub(crate) fn get(&self, opcode: u16) -> Option<Handler> {
        unsafe { *self.table.get_unchecked(opcode as usize) }
    }

    /// Returns the number of opcodes that have registered handlers.
    ///
    /// # Returns
    /// The count of non-`None` slots in the handler table.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if no handlers have been registered.
    ///
    /// # Returns
    /// `true` when the handler count is zero, `false` otherwise.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let ops = OpsRegistry::new();
        assert!(ops.is_empty());
        assert_eq!(ops.len(), 0);
    }

    #[test]
    fn insert_and_len() {
        let mut ops = OpsRegistry::new();
        ops.insert(0, |_s| Ok(()));
        assert_eq!(ops.len(), 1);
        assert!(!ops.is_empty());
    }

    #[test]
    fn insert_same_opcode_doesnt_double_count() {
        let mut ops = OpsRegistry::new();
        ops.insert(100, |_s| Ok(()));
        ops.insert(100, |_s| Ok(()));
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn insert_different_opcodes() {
        let mut ops = OpsRegistry::new();
        ops.insert(0, |_s| Ok(()));
        ops.insert(1, |_s| Ok(()));
        ops.insert(2, |_s| Ok(()));
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn get_existing_returns_some() {
        let mut ops = OpsRegistry::new();
        ops.insert(42, |_s| Ok(()));
        assert!(ops.get(42).is_some());
    }

    #[test]
    fn get_missing_returns_none() {
        let ops = OpsRegistry::new();
        assert!(ops.get(42).is_none());
    }

    #[test]
    fn extend_merges() {
        let mut a = OpsRegistry::new();
        a.insert(0, |_s| Ok(()));
        a.insert(1, |_s| Ok(()));

        let mut b = OpsRegistry::new();
        b.insert(2, |_s| Ok(()));
        b.insert(3, |_s| Ok(()));

        a.extend(b);
        assert_eq!(a.len(), 4);
        assert!(a.get(0).is_some());
        assert!(a.get(1).is_some());
        assert!(a.get(2).is_some());
        assert!(a.get(3).is_some());
    }

    #[test]
    fn extend_overlapping_counts_correctly() {
        let mut a = OpsRegistry::new();
        a.insert(0, |_s| Ok(()));

        let mut b = OpsRegistry::new();
        b.insert(0, |_s| Ok(())); // same opcode
        b.insert(1, |_s| Ok(()));

        a.extend(b);
        assert_eq!(a.len(), 2); // 0 was already counted
    }

    #[test]
    fn extend_empty_into_existing() {
        let mut a = OpsRegistry::new();
        a.insert(0, |_s| Ok(()));
        let b = OpsRegistry::new();
        a.extend(b);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn extend_into_empty() {
        let mut a = OpsRegistry::new();
        let mut b = OpsRegistry::new();
        b.insert(0, |_s| Ok(()));
        a.extend(b);
        assert_eq!(a.len(), 1);
    }
}
