//! Expansion policy — the third Layout dimension of
//! [`HashTrie`](super::HashTrie).
//!
//! Lazy child expansion (SIGMOD 2020 §3.3.1, Figure 6) builds only the root
//! table at construction. Every child below it keeps its tuples' row ids as a
//! list until a probe first opens it, and only then is that child's table
//! built, one level at a time. It is a *shape*: an unexpanded child is a node
//! state, so it is a Layout, not a Config. Under [`EagerExpansion`] the state's
//! payload is the uninhabited [`Never`], the `Unexpanded` variant vanishes from
//! the node's layout, and the instantiation compiles to the eager code. Bench
//! axis: `ds_layout_expansion`.
//!
//! The payload is generic over the node type `N` rather than naming
//! `HashTrieNode`. That keeps these two traits, public but in a private
//! module like `pruning.rs`'s, free of the crate-private node type, which
//! `private_interfaces` would reject.

use {
    super::pruning::Never,
    kermit_iters::{LayoutOption, RowId},
    std::cell::{OnceCell, Ref, RefCell},
};

/// What a `HashTrieNode::Unexpanded` holds: `Box<LazyChild<N>>` under
/// [`LazyExpansion`], [`Never`] under [`EagerExpansion`].
pub trait PendingChild<N>: Sized {
    /// An unexpanded child holding `rows`, the row ids of its tuples in the
    /// trie's buffer, in insertion order.
    fn from_rows(rows: Vec<RowId>) -> Self;
    /// The built table, once a probe has expanded this child.
    fn built(&self) -> Option<&N>;
    /// [`built`](Self::built) for `insert`, which holds the trie mutably.
    fn built_mut(&mut self) -> Option<&mut N>;
    /// The row ids not yet built into a table; empty once expanded.
    fn pending(&self) -> Ref<'_, Vec<RowId>>;
    /// Appends `row` to an unexpanded child. Callers check
    /// [`built_mut`](Self::built_mut) first.
    fn push(&mut self, row: RowId);
    /// The built table, building it first from the pending ids if no
    /// probe has yet. The list is moved into `build`, never copied, so an
    /// expanded child keeps no list.
    fn expand(&self, build: impl FnOnce(Vec<RowId>) -> N) -> &N;
    /// Heap bytes of the payload itself (the box), excluding the pending
    /// ids and the built table, which `heap_size_bytes` counts separately.
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
    /// node type `N`. `Send` whenever `N` is, so a parallel build can move
    /// finished subtries between threads: `LazyChild`'s cells are `Send`,
    /// only `!Sync`.
    type Pending<N: Send>: PendingChild<N> + Send;
    /// Folded by the compiler: `insert_at`'s lazy branches are
    /// `if E::LAZY { … }`.
    const LAZY: bool;
}

impl<N> PendingChild<N> for Never {
    fn from_rows(_rows: Vec<RowId>) -> Self {
        unreachable!("EagerExpansion never constructs an Unexpanded child (E::LAZY is false)")
    }

    fn built(&self) -> Option<&N> { match *self {} }

    fn built_mut(&mut self) -> Option<&mut N> { match *self {} }

    fn pending(&self) -> Ref<'_, Vec<RowId>> { match *self {} }

    fn push(&mut self, _row: RowId) { match *self {} }

    fn expand(&self, _build: impl FnOnce(Vec<RowId>) -> N) -> &N { match *self {} }

    fn own_heap_bytes(&self) -> usize { match *self {} }
}

/// An unexpanded child: its tuples' row ids until a probe reaches it, the
/// table built from them afterwards.
///
/// `RefCell` and `OnceCell` make lazy tries `!Sync` (still `Send`). Nothing
/// in the workspace shares a relation across threads; a parallel prober
/// would need a different cell.
pub struct LazyChild<N> {
    /// The row ids of the tuples below this bucket, in insertion order.
    /// Moved into the build by expansion, leaving an empty, unallocated
    /// vector.
    pending: RefCell<Vec<RowId>>,
    /// The table this level would have held, once a probe has reached it.
    built: OnceCell<N>,
}

impl<N> PendingChild<N> for Box<LazyChild<N>> {
    fn from_rows(rows: Vec<RowId>) -> Self {
        Box::new(LazyChild {
            pending: RefCell::new(rows),
            built: OnceCell::new(),
        })
    }

    fn built(&self) -> Option<&N> { self.built.get() }

    fn built_mut(&mut self) -> Option<&mut N> { self.built.get_mut() }

    fn pending(&self) -> Ref<'_, Vec<RowId>> { self.pending.borrow() }

    fn push(&mut self, row: RowId) {
        debug_assert!(self.built.get().is_none(), "push into an expanded child");
        self.pending.get_mut().push(row);
    }

    // A visitor that opens an iterator on the same trie while
    // `for_each_tuple` holds `pending()` makes `borrow_mut` panic here
    // (`BorrowMutError`): a loud internal panic, not a wrong answer. No
    // caller in the workspace does that.
    fn expand(&self, build: impl FnOnce(Vec<RowId>) -> N) -> &N {
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
    type Pending<N: Send> = Never;

    const LAZY: bool = false;
}

/// Lazy child expansion (paper Figure 6). Bench axis value `"lazy"`.
#[derive(Copy, Clone, Default, Debug)]
pub struct LazyExpansion;

impl LayoutOption for LazyExpansion {
    const NAME: &'static str = "lazy";
}

impl ExpansionPolicy for LazyExpansion {
    type Pending<N: Send> = Box<LazyChild<N>>;

    const LAZY: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    type Lazy = Box<LazyChild<Vec<RowId>>>;

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
    fn expand_moves_the_pending_rows_once() {
        let mut child = Lazy::from_rows(vec![0]);
        child.push(1);
        assert!(child.built().is_none());
        assert_eq!(*child.pending(), vec![0, 1]);
        let mut calls = 0;
        let built = child.expand(|rows| {
            calls += 1;
            rows
        });
        assert_eq!(built, &vec![0, 1]);
        // A second expand returns the same table without building again.
        let again = child.expand(|_| unreachable!("already built"));
        assert!(std::ptr::eq(built, again));
        assert_eq!(calls, 1);
        assert!(child.pending().is_empty());
        assert_eq!(child.pending().capacity(), 0);
    }

    #[test]
    fn built_mut_reaches_the_expanded_table() {
        let mut child = Lazy::from_rows(vec![0]);
        assert!(child.built_mut().is_none());
        child.expand(|rows| rows);
        child.built_mut().unwrap().push(1);
        assert_eq!(child.built(), Some(&vec![0, 1]));
    }

    #[test]
    fn own_heap_bytes_is_the_box() {
        let child = Lazy::from_rows(Vec::new());
        assert_eq!(
            child.own_heap_bytes(),
            std::mem::size_of::<LazyChild<Vec<RowId>>>()
        );
    }
}
