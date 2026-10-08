//! Pruning policy — the second Layout dimension of
//! [`HashTrie`](super::HashTrie).
//!
//! Singleton pruning (SIGMOD 2020 §3.3.1, Figure 5) stores a subtrie that holds
//! exactly one tuple as that tuple's row id instead of one hash table per
//! remaining level. It is a *shape*: fixed at construction, it changes which
//! node variants exist. Encoding it as a type parameter lets the `NoPruning`
//! instantiation compile to the pre-pruning code — the `Singleton` payload and
//! the iterator's singleton frame are uninhabited there, so every arm that
//! handles them is dead code the compiler removes. Bench axis:
//! `ds_layout_pruning`.

use kermit_iters::{LayoutOption, RowId};

/// What a `HashTrieNode::Singleton` stores: the [`RowId`] of its one tuple,
/// in the trie's buffer, when pruning is on; [`Never`] when it is off, which
/// makes the variant uninhabited.
pub trait SingletonPayload {
    /// Wrap `row` as the payload of a pruned subtrie.
    fn from_row(row: RowId) -> Self;
    /// The id of the row stored below the pruned node. A reference into the
    /// node, so an iterator frame can hold it for the trie's lifetime and
    /// lend it as a one-id chain (`slice::from_ref`).
    fn row(&self) -> &RowId;
    /// Consume the payload, yielding the id it stored.
    fn into_row(self) -> RowId;
}

/// The iterator's stand-in for the one-entry table a pruned level would
/// have held. One implementor per policy; the `NoPruning` one is [`Never`],
/// so the iterator's singleton arms vanish in that instantiation.
pub trait SingletonFrame<'a>: Sized {
    /// A frame for the pruned subtrie's row `row`, positioned on its single
    /// entry. `hash` is the hash of the row's value at the frame's depth,
    /// computed once by the caller (`HashTrieIter::singleton_frame`, which
    /// reads the row and checks the depth).
    // A borrow of the payload's id, as the frame borrowed its tuple before:
    // `row()` hands it back with the trie lifetime, and `leaf_tuples` lends
    // it as a one-id chain via `slice::from_ref`.
    fn new(row: &'a RowId, hash: u64) -> Self;
    /// The row id of the tuple stored below the pruned node.
    fn row(&self) -> &'a RowId;
    /// `Some(hash)` unless exhausted.
    fn key(&self) -> Option<u64>;
    /// Advance past the single entry.
    fn exhaust(&mut self);
    /// Position on the entry iff `hash` matches; otherwise exhaust.
    fn lookup(&mut self, hash: u64) -> bool;
    /// Whether the single entry has been passed.
    fn at_end(&self) -> bool;
}

/// Compile-time pruning policy of a [`HashTrie`](super::HashTrie).
///
/// The two associated types and the constant must agree: `ENABLED == false`
/// requires `Payload` and `Frame` to be uninhabited (so no `Singleton` node
/// or frame can exist), and `ENABLED == true` requires them inhabited (so
/// the prune and unprune paths can actually store a row id). Nothing in the
/// type system enforces that pairing, which is why the implementor set is
/// closed by design: `SingletonPayload` and `SingletonFrame` live in
/// this private module, so [`NoPruning`] and [`SingletonPruning`] are the
/// only policies that can ever exist and the pairing is checked once, here.
pub trait PruningPolicy: LayoutOption + Copy + Default + 'static {
    /// What a `HashTrieNode::Singleton` holds under this policy. `Send`, so
    /// a parallel build can move finished subtries between threads.
    type Payload: SingletonPayload + Send;
    /// The iterator frame standing in for a pruned level.
    type Frame<'a>: SingletonFrame<'a>;
    /// Folded by the compiler: `insert_at`'s prune and unprune branches are
    /// `if P::ENABLED { … }`.
    const ENABLED: bool;
}

/// An uninhabited type. A `Singleton(Never)` variant can never be built,
/// and rustc's layout computation omits uninhabited variants, so in
/// practice the enum containing it is laid out as the enum without it.
/// That is an optimisation, not a language guarantee — the two size tests
/// pin it: `node_does_not_grow_under_the_pruning_policy` (in
/// `implementation.rs`) and `off_frame_is_the_bare_table_pair` (in
/// `hash_trie_iter.rs`).
///
/// Also the eager payload of `HashTrieNode::Unexpanded` (see
/// `expansion.rs`), for the same reason.
#[derive(Copy, Clone, Debug)]
pub enum Never {}

impl SingletonPayload for Never {
    fn from_row(_row: RowId) -> Self {
        unreachable!("NoPruning never constructs a Singleton (P::ENABLED is false)")
    }

    fn row(&self) -> &RowId { match *self {} }

    fn into_row(self) -> RowId { match self {} }
}

impl<'a> SingletonFrame<'a> for Never {
    fn new(_row: &'a RowId, _hash: u64) -> Self {
        unreachable!("NoPruning never pushes a singleton frame")
    }

    fn row(&self) -> &'a RowId { match *self {} }

    fn key(&self) -> Option<u64> { match *self {} }

    fn exhaust(&mut self) { match *self {} }

    fn lookup(&mut self, _hash: u64) -> bool { match *self {} }

    fn at_end(&self) -> bool { match *self {} }
}

impl SingletonPayload for RowId {
    fn from_row(row: RowId) -> Self { row }

    fn row(&self) -> &RowId { self }

    fn into_row(self) -> RowId { self }
}

/// One emulated level of a pruned subtrie holding the row `row`;
/// `exhausted` plays the role of a table frame's past-end bucket index.
///
/// The level itself is not stored: a frame's depth is its position in the
/// iterator's stack, which `HashTrieIter` reads off `stack.len()`.
#[derive(Debug)]
pub struct SingletonFrameOn<'a> {
    // Borrowed from the payload: `leaf_tuples` lends it as a one-id chain
    // via `slice::from_ref`.
    row: &'a RowId,
    hash: u64,
    exhausted: bool,
}

impl<'a> SingletonFrame<'a> for SingletonFrameOn<'a> {
    fn new(row: &'a RowId, hash: u64) -> Self {
        Self {
            row,
            hash,
            exhausted: false,
        }
    }

    fn row(&self) -> &'a RowId { self.row }

    fn key(&self) -> Option<u64> { (!self.exhausted).then_some(self.hash) }

    fn exhaust(&mut self) { self.exhausted = true; }

    fn lookup(&mut self, hash: u64) -> bool {
        self.exhausted = hash != self.hash;
        !self.exhausted
    }

    fn at_end(&self) -> bool { self.exhausted }
}

/// Pruning off: `HashTrie` compiles to the pre-pruning code path, because
/// both the `Singleton` node payload and the singleton frame are
/// uninhabited here and every arm handling them is dead. Bench axis value
/// `"off"`. The default `P`.
#[derive(Copy, Clone, Default, Debug)]
pub struct NoPruning;

impl LayoutOption for NoPruning {
    const NAME: &'static str = "off";
}

impl PruningPolicy for NoPruning {
    type Frame<'a> = Never;
    type Payload = Never;

    const ENABLED: bool = false;
}

/// Singleton pruning on (paper Figure 5). Bench axis value `"on"`.
#[derive(Copy, Clone, Default, Debug)]
pub struct SingletonPruning;

impl LayoutOption for SingletonPruning {
    const NAME: &'static str = "on";
}

impl PruningPolicy for SingletonPruning {
    type Frame<'a> = SingletonFrameOn<'a>;
    type Payload = RowId;

    const ENABLED: bool = true;
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::LayoutOption};

    #[test]
    fn policy_names_are_the_axis_values() {
        assert_eq!(<NoPruning as LayoutOption>::NAME, "off");
        assert_eq!(<SingletonPruning as LayoutOption>::NAME, "on");
    }

    #[test]
    fn enabled_matches_the_marker() {
        const { assert!(!NoPruning::ENABLED) };
        const { assert!(SingletonPruning::ENABLED) };
    }

    #[test]
    fn row_payload_round_trips_the_id() {
        let p = <RowId as SingletonPayload>::from_row(7);
        assert_eq!(p.row(), &7);
        assert_eq!(p.into_row(), 7);
    }

    #[test]
    fn never_is_uninhabited() {
        assert_eq!(std::mem::size_of::<Never>(), 0);
        // A reference to an uninhabited payload can never be produced; the
        // `Option` below is `None` by construction and the branch is dead.
        let none: Option<&Never> = None;
        assert!(none.is_none());
    }

    #[test]
    fn frame_emulates_a_one_entry_table() {
        let row: RowId = 7;
        let mut f = <SingletonFrameOn<'_> as SingletonFrame<'_>>::new(&row, 0xBEEF);
        assert_eq!(f.key(), Some(0xBEEF));
        assert!(!f.at_end());
        assert!(!f.lookup(0xDEAD));
        assert!(f.at_end());
        assert_eq!(f.key(), None);
        assert!(f.lookup(0xBEEF));
        assert!(!f.at_end());
        f.exhaust();
        assert!(f.at_end());
        assert_eq!(f.row(), &row);
    }

    #[test]
    fn frame_is_a_borrow_a_hash_and_a_flag() {
        // The emulated level is the frame's position in the iterator stack,
        // not a stored field: a pointer, a `u64` and a `bool`, no more.
        assert_eq!(std::mem::size_of::<SingletonFrameOn<'static>>(), 24);
    }
}
