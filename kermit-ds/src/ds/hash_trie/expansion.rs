//! Expansion policy — the third Layout dimension of
//! [`HashTrie`](super::HashTrie).
//!
//! Lazy child expansion (SIGMOD 2020 §3.3.1, Figure 6) builds only the root
//! table at construction. Every child below it keeps its tuples as a list
//! until a probe first opens it, and only then is that child's table built,
//! one level at a time. It is a *shape*: an unexpanded child is a node
//! state, so it is a Layout, not a Config. Under [`EagerExpansion`] the
//! state's payload is the uninhabited [`Never`], the `Unexpanded` variant
//! vanishes from the node's layout, and the instantiation compiles to the
//! eager code. Bench axis: `ds_layout_expansion`.
//!
//! The payload is generic over the node type `N` rather than naming
//! `HashTrieNode`. That keeps these two traits, public but in a private
//! module like `pruning.rs`'s, free of the crate-private node type, which
//! `private_interfaces` would reject.

use {
    super::pruning::Never,
    kermit_iters::LayoutOption,
    std::cell::{OnceCell, Ref, RefCell},
};

/// What a `HashTrieNode::Unexpanded` holds: `Box<LazyChild<N>>` under
/// [`LazyExpansion`], [`Never`] under [`EagerExpansion`].
pub trait PendingChild<N>: Sized {
    /// An unexpanded child holding `tuples`, in insertion order.
    fn from_tuples(tuples: Vec<Vec<usize>>) -> Self;
    /// The built table, once a probe has expanded this child.
    fn built(&self) -> Option<&N>;
    /// [`built`](Self::built) for `insert`, which holds the trie mutably.
    fn built_mut(&mut self) -> Option<&mut N>;
    /// The tuples not yet built into a table; empty once expanded.
    fn pending(&self) -> Ref<'_, Vec<Vec<usize>>>;
    /// Appends `tuple` to an unexpanded child. Callers check
    /// [`built_mut`](Self::built_mut) first.
    fn push(&mut self, tuple: Vec<usize>);
    /// The built table, building it first from the pending tuples if no
    /// probe has yet. The tuples are moved into `build`, never copied, so
    /// an expanded child stores each tuple once.
    fn expand(&self, build: impl FnOnce(Vec<Vec<usize>>) -> N) -> &N;
    /// Heap bytes of the payload itself (the box), excluding the pending
    /// tuples and the built table, which `heap_size_bytes` counts
    /// separately.
    fn own_heap_bytes(&self) -> usize;
}

/// Compile-time expansion policy of a [`HashTrie`](super::HashTrie).
///
/// As with `PruningPolicy`, the associated type and the constant must
/// agree: `LAZY == false` requires an uninhabited `Pending`, so no
/// `Unexpanded` node can exist. `PendingChild` lives in this private
/// module, so [`EagerExpansion`] and [`LazyExpansion`] are the only
/// policies and the pairing is checked here, once.
pub trait ExpansionPolicy: LayoutOption + Copy + Default + 'static {
    /// What a `HashTrieNode::Unexpanded` holds under this policy, for a
    /// node type `N`.
    type Pending<N>: PendingChild<N>;
    /// Folded by the compiler: `insert_at`'s lazy branches are
    /// `if E::LAZY { … }`.
    const LAZY: bool;
}

impl<N> PendingChild<N> for Never {
    fn from_tuples(_tuples: Vec<Vec<usize>>) -> Self {
        unreachable!("EagerExpansion never constructs an Unexpanded child (E::LAZY is false)")
    }

    fn built(&self) -> Option<&N> { match *self {} }

    fn built_mut(&mut self) -> Option<&mut N> { match *self {} }

    fn pending(&self) -> Ref<'_, Vec<Vec<usize>>> { match *self {} }

    fn push(&mut self, _tuple: Vec<usize>) { match *self {} }

    fn expand(&self, _build: impl FnOnce(Vec<Vec<usize>>) -> N) -> &N { match *self {} }

    fn own_heap_bytes(&self) -> usize { match *self {} }
}

/// An unexpanded child: its tuples until a probe reaches it, the table built
/// from them afterwards.
///
/// `RefCell` and `OnceCell` make lazy tries `!Sync` (still `Send`). Nothing
/// in the workspace shares a relation across threads; a parallel prober
/// would need a different cell.
pub struct LazyChild<N> {
    /// The tuples below this bucket, in insertion order. Moved into the
    /// build by expansion, leaving an empty, unallocated vector.
    pending: RefCell<Vec<Vec<usize>>>,
    /// The table this level would have held, once a probe has reached it.
    built: OnceCell<N>,
}

impl<N> PendingChild<N> for Box<LazyChild<N>> {
    fn from_tuples(tuples: Vec<Vec<usize>>) -> Self {
        Box::new(LazyChild {
            pending: RefCell::new(tuples),
            built: OnceCell::new(),
        })
    }

    fn built(&self) -> Option<&N> { self.built.get() }

    fn built_mut(&mut self) -> Option<&mut N> { self.built.get_mut() }

    fn pending(&self) -> Ref<'_, Vec<Vec<usize>>> { self.pending.borrow() }

    fn push(&mut self, tuple: Vec<usize>) {
        debug_assert!(self.built.get().is_none(), "push into an expanded child");
        self.pending.get_mut().push(tuple);
    }

    // A visitor that opens an iterator on the same trie while
    // `for_each_tuple` holds `pending()` makes `borrow_mut` panic here
    // (`BorrowMutError`): a loud internal panic, not a wrong answer. No
    // caller in the workspace does that.
    fn expand(&self, build: impl FnOnce(Vec<Vec<usize>>) -> N) -> &N {
        self.built
            .get_or_init(|| build(std::mem::take(&mut *self.pending.borrow_mut())))
    }

    fn own_heap_bytes(&self) -> usize { std::mem::size_of::<LazyChild<N>>() }
}

/// Every level built at construction: today's structure. Bench axis value
/// `"eager"`. The default `E`.
#[derive(Copy, Clone, Default, Debug)]
pub struct EagerExpansion;

impl LayoutOption for EagerExpansion {
    const NAME: &'static str = "eager";
}

impl ExpansionPolicy for EagerExpansion {
    type Pending<N> = Never;

    const LAZY: bool = false;
}

/// Lazy child expansion (paper Figure 6). Bench axis value `"lazy"`.
#[derive(Copy, Clone, Default, Debug)]
pub struct LazyExpansion;

impl LayoutOption for LazyExpansion {
    const NAME: &'static str = "lazy";
}

impl ExpansionPolicy for LazyExpansion {
    type Pending<N> = Box<LazyChild<N>>;

    const LAZY: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    type Lazy = Box<LazyChild<Vec<Vec<usize>>>>;

    #[test]
    fn policy_names_are_the_axis_values() {
        assert_eq!(<EagerExpansion as LayoutOption>::NAME, "eager");
        assert_eq!(<LazyExpansion as LayoutOption>::NAME, "lazy");
    }

    #[test]
    fn lazy_matches_the_marker() {
        const { assert!(!EagerExpansion::LAZY) };
        const { assert!(LazyExpansion::LAZY) };
    }

    #[test]
    fn expand_moves_the_pending_tuples_once() {
        let mut child = Lazy::from_tuples(vec![vec![1, 2]]);
        child.push(vec![3, 4]);
        assert!(child.built().is_none());
        assert_eq!(*child.pending(), vec![vec![1, 2], vec![3, 4]]);
        let mut calls = 0;
        let built = child.expand(|tuples| {
            calls += 1;
            tuples
        });
        assert_eq!(built, &vec![vec![1, 2], vec![3, 4]]);
        // A second expand returns the same table without building again.
        let again = child.expand(|_| unreachable!("already built"));
        assert!(std::ptr::eq(built, again));
        assert_eq!(calls, 1);
        assert!(child.pending().is_empty());
        assert_eq!(child.pending().capacity(), 0);
    }

    #[test]
    fn built_mut_reaches_the_expanded_table() {
        let mut child = Lazy::from_tuples(vec![vec![1]]);
        assert!(child.built_mut().is_none());
        child.expand(|tuples| tuples);
        child.built_mut().unwrap().push(vec![2]);
        assert_eq!(child.built(), Some(&vec![vec![1], vec![2]]));
    }

    #[test]
    fn own_heap_bytes_is_the_box() {
        let child = Lazy::from_tuples(Vec::new());
        assert_eq!(
            child.own_heap_bytes(),
            std::mem::size_of::<LazyChild<Vec<Vec<usize>>>>()
        );
    }
}
