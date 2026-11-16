//! Append-only identities. Removed slots remain reserved for undo restoration.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arena<T> {
    pub(crate) slots: Vec<Option<T>>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self { slots: Vec::new() }
    }
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, value: T) -> u32 {
        let id = u32::try_from(self.slots.len()).expect("arena id space exhausted");
        self.slots.push(Some(value));
        id
    }

    pub fn get(&self, id: u32) -> Option<&T> {
        self.slots.get(id as usize)?.as_ref()
    }

    pub(crate) fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        self.slots.get_mut(id as usize)?.as_mut()
    }

    pub fn remove(&mut self, id: u32) -> Option<T> {
        self.slots.get_mut(id as usize)?.take()
    }

    /// Restores only a previously allocated, empty slot. Returns the value on failure.
    pub fn restore(&mut self, id: u32, value: T) -> Result<(), T> {
        match self.slots.get_mut(id as usize) {
            Some(slot @ None) => {
                *slot = Some(value);
                Ok(())
            }
            _ => Err(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn restore_rejects_live_and_unallocated_ids() {
        let mut arena = Arena::new();
        assert_eq!(arena.restore(0, 1), Err(1));
        let id = arena.insert(2);
        assert_eq!(arena.restore(id, 3), Err(3));
        assert_eq!(arena.get(id), Some(&2));
        assert_eq!(arena.remove(100), None);
    }

    proptest! {
        #[test]
        fn remove_restore_preserves_identity(values in prop::collection::vec(any::<i64>(), 1..200), pick in any::<usize>()) {
            let mut arena = Arena::new();
            let ids: Vec<_> = values.iter().map(|v| arena.insert(*v)).collect();
            let before = arena.clone();
            let id = ids[pick % ids.len()];
            let removed = arena.remove(id).unwrap();
            prop_assert_eq!(arena.get(id), None);
            prop_assert_eq!(arena.remove(id), None);
            prop_assert_eq!(arena.restore(id, removed), Ok(()));
            prop_assert_eq!(&arena, &before);
            arena.remove(id);
            let fresh = arena.insert(99);
            prop_assert!(fresh > *ids.last().unwrap());
            prop_assert_eq!(arena.get(id), None);
            prop_assert_eq!(arena.restore(id, removed), Ok(()));
            prop_assert_eq!(arena.get(id), Some(&removed));
        }
    }
}
