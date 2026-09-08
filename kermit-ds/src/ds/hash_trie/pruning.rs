//! Pruning policy — the second Layout dimension of [`HashTrie`](super::HashTrie).
//!
//! Singleton pruning (SIGMOD 2020 §3.3.1, Figure 5) stores a subtrie that
//! holds exactly one tuple as that tuple instead of one hash table per
//! remaining level. It is a *shape*: fixed at construction, it changes which
//! node variants exist. Encoding it as a type parameter lets the `NoPruning`
//! instantiation compile to the pre-pruning code — the `Singleton` payload
//! and the iterator's singleton frame are uninhabited there, so every arm
//! that handles them is dead code the compiler removes. Bench axis:
//! `ds_layout_pruning`.

use kermit_iters::LayoutOption;

/// The tuple a `HashTrieNode::Singleton` stores. `Vec<usize>` when pruning
/// is on; [`Never`] when it is off, which makes the variant uninhabited.
pub trait SingletonPayload {
    /// Wrap `tuple` as the payload of a pruned subtrie.
    fn from_tuple(tuple: Vec<usize>) -> Self;
    /// The tuple stored below the pruned node.
    fn tuple(&self) -> &Vec<usize>;
    /// Consume the payload, yielding the tuple it stored.
    fn into_tuple(self) -> Vec<usize>;
}

/// The iterator's stand-in for the one-entry table a pruned level would
/// have held. One implementor per policy; the `NoPruning` one is [`Never`],
/// so the iterator's singleton arms vanish in that instantiation.
pub trait SingletonFrame<'a>: Sized {
    /// A frame at `depth` for `tuple`, positioned on its single entry.
    /// `hash == H::hash(tuple[depth])`, computed once by the caller.
    // `&'a Vec`, not `&'a [usize]`: the frame stores the borrow and hands it
    // back from `tuple()`, which feeds `leaf_tuples`' `&[Vec<usize>]` via
    // `slice::from_ref`.
    #[allow(clippy::ptr_arg)]
    fn new(tuple: &'a Vec<usize>, depth: usize, hash: u64) -> Self;
    /// The trie level this frame stands in for.
    fn depth(&self) -> usize;
    /// The tuple stored below the pruned node.
    fn tuple(&self) -> &'a Vec<usize>;
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
pub trait PruningPolicy: LayoutOption + Copy + Default + 'static {
    /// What a `HashTrieNode::Singleton` holds under this policy.
    type Payload: SingletonPayload;
    /// The iterator frame standing in for a pruned level.
    type Frame<'a>: SingletonFrame<'a>;
    /// Folded by the compiler: `insert_at`'s prune and unprune branches are
    /// `if P::ENABLED { … }`.
    const ENABLED: bool;
}

/// An uninhabited type. A `Singleton(Never)` variant can never be built, so
/// the enum containing it has the layout of the enum without it.
#[derive(Copy, Clone, Debug)]
pub enum Never {}

impl SingletonPayload for Never {
    fn from_tuple(_tuple: Vec<usize>) -> Self {
        unreachable!("NoPruning never constructs a Singleton (P::ENABLED is false)")
    }

    fn tuple(&self) -> &Vec<usize> { match *self {} }

    fn into_tuple(self) -> Vec<usize> { match self {} }
}

impl<'a> SingletonFrame<'a> for Never {
    fn new(_tuple: &'a Vec<usize>, _depth: usize, _hash: u64) -> Self {
        unreachable!("NoPruning never pushes a singleton frame")
    }

    fn depth(&self) -> usize { match *self {} }

    fn tuple(&self) -> &'a Vec<usize> { match *self {} }

    fn key(&self) -> Option<u64> { match *self {} }

    fn exhaust(&mut self) { match *self {} }

    fn lookup(&mut self, _hash: u64) -> bool { match *self {} }

    fn at_end(&self) -> bool { match *self {} }
}

impl SingletonPayload for Vec<usize> {
    fn from_tuple(tuple: Vec<usize>) -> Self { tuple }

    fn tuple(&self) -> &Vec<usize> { self }

    fn into_tuple(self) -> Vec<usize> { self }
}

/// Level `depth` of a pruned subtrie holding `tuple`; `exhausted` plays the
/// role of a table frame's past-end bucket index.
#[derive(Debug)]
pub struct SingletonFrameOn<'a> {
    // `&Vec`, not `&[usize]`: `leaf_tuples` returns `&[Vec<usize>]` via
    // `slice::from_ref`.
    tuple: &'a Vec<usize>,
    depth: usize,
    hash: u64,
    exhausted: bool,
}

impl<'a> SingletonFrame<'a> for SingletonFrameOn<'a> {
    fn new(tuple: &'a Vec<usize>, depth: usize, hash: u64) -> Self {
        debug_assert!(
            depth < tuple.len(),
            "singleton frame at depth {depth} below a {}-attribute tuple",
            tuple.len()
        );
        Self {
            tuple,
            depth,
            hash,
            exhausted: false,
        }
    }

    fn depth(&self) -> usize { self.depth }

    fn tuple(&self) -> &'a Vec<usize> { self.tuple }

    fn key(&self) -> Option<u64> { (!self.exhausted).then_some(self.hash) }

    fn exhaust(&mut self) { self.exhausted = true; }

    fn lookup(&mut self, hash: u64) -> bool {
        self.exhausted = hash != self.hash;
        !self.exhausted
    }

    fn at_end(&self) -> bool { self.exhausted }
}

/// Pruning off: the pre-pruning `HashTrie`, bit for bit. Bench axis value
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
    type Payload = Vec<usize>;

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
    fn vec_payload_round_trips_the_tuple() {
        let p = <Vec<usize> as SingletonPayload>::from_tuple(vec![1, 2, 3]);
        assert_eq!(p.tuple(), &vec![1, 2, 3]);
        assert_eq!(p.into_tuple(), vec![1, 2, 3]);
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
        let tuple = vec![7, 8, 9];
        let mut f = <SingletonFrameOn<'_> as SingletonFrame<'_>>::new(&tuple, 1, 0xBEEF);
        assert_eq!(f.depth(), 1);
        assert_eq!(f.key(), Some(0xBEEF));
        assert!(!f.at_end());
        assert!(!f.lookup(0xDEAD));
        assert!(f.at_end());
        assert_eq!(f.key(), None);
        assert!(f.lookup(0xBEEF));
        assert!(!f.at_end());
        f.exhaust();
        assert!(f.at_end());
        assert_eq!(f.tuple(), &tuple);
    }
}
