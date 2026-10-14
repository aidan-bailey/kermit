//! Expansion policy — the third Layout dimension of
//! [`HashTrie`](super::HashTrie).
//!
//! Lazy child expansion (SIGMOD 2020 §3.3.1, Figure 6) builds only the root
//! table at construction. Every child below it keeps its tuples as a chain
//! of rows threaded through the trie's links (`chain.rs`) until a probe
//! first opens it, and only then is that child's table built, one level at
//! a time, re-threading the chain's rows into the child's lists. It is a
//! *shape*: an unexpanded child is a node state, so it is a Layout, not a
//! Config. Under [`EagerExpansion`] the state's payload is the uninhabited
//! [`Never`], the `Unexpanded` variant vanishes from the node's layout, and
//! the instantiation compiles to the eager code. Bench axis:
//! `ds_layout_expansion`.
//!
//! The payload is generic over the node type `N` rather than naming
//! `HashTrieNode`. That keeps these two traits, public but in a private
//! module like `pruning.rs`'s, free of the crate-private node type, which
//! `private_interfaces` would reject. `Chain`, which `PendingChild` does
//! name, is nominally `pub` for the same reason.

use {
    super::{chain::Chain, pruning::Never},
    kermit_iters::{ChainLinks, LayoutOption, RowId},
    std::cell::{Cell, OnceCell},
};

/// What a `HashTrieNode::Unexpanded` holds: `Box<LazyChild<N>>` under
/// [`LazyExpansion`], [`Never`] under [`EagerExpansion`].
pub trait PendingChild<N>: Sized {
    /// An unexpanded child holding `chain`: its tuples' rows in the trie's
    /// buffer, in insertion order, threaded through the trie's links.
    fn from_chain(chain: Chain) -> Self;
    /// The built table, once a probe has expanded this child.
    fn built(&self) -> Option<&N>;
    /// [`built`](Self::built) for `insert`, which holds the trie mutably.
    fn built_mut(&mut self) -> Option<&mut N>;
    /// The chain of rows not yet built into a table; `None` once expanded.
    fn pending(&self) -> Option<Chain>;
    /// Appends `row` to the pending chain, through the trie's `links`.
    /// Callers check [`built_mut`](Self::built_mut) first.
    fn push(&mut self, links: &ChainLinks, row: RowId);
    /// The built table, building it first from the pending chain if no
    /// probe has yet. The chain is taken, so an expanded child keeps no
    /// list, and `build` re-threads its rows through the trie's links,
    /// under the `&self` a probe holds, which the links' atomics allow.
    fn expand(&self, build: impl FnOnce(Chain) -> N) -> &N;
    /// Heap bytes of the payload itself (the box), excluding the built
    /// table, which `heap_size_bytes` counts separately. The pending chain
    /// lives in the box, so it adds nothing.
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
    fn from_chain(_chain: Chain) -> Self {
        unreachable!("EagerExpansion never constructs an Unexpanded child (E::LAZY is false)")
    }

    fn built(&self) -> Option<&N> { match *self {} }

    fn built_mut(&mut self) -> Option<&mut N> { match *self {} }

    fn pending(&self) -> Option<Chain> { match *self {} }

    fn push(&mut self, _links: &ChainLinks, _row: RowId) { match *self {} }

    fn expand(&self, _build: impl FnOnce(Chain) -> N) -> &N { match *self {} }

    fn own_heap_bytes(&self) -> usize { match *self {} }
}

/// An unexpanded child: the chain of its tuples' rows until a probe
/// reaches it, the table built from them afterwards.
///
/// `Cell` and `OnceCell` make lazy tries `!Sync` (still `Send`). Nothing
/// in the workspace shares a relation across threads; a parallel prober
/// would need a different cell.
pub struct LazyChild<N> {
    /// The rows below this bucket, in insertion order, threaded through the
    /// trie's links: a `Copy` value, which expansion takes, leaving `None`.
    pending: Cell<Option<Chain>>,
    /// The table this level would have held, once a probe has reached it.
    built: OnceCell<N>,
}

impl<N> PendingChild<N> for Box<LazyChild<N>> {
    fn from_chain(chain: Chain) -> Self {
        Box::new(LazyChild {
            pending: Cell::new(Some(chain)),
            built: OnceCell::new(),
        })
    }

    fn built(&self) -> Option<&N> { self.built.get() }

    fn built_mut(&mut self) -> Option<&mut N> { self.built.get_mut() }

    fn pending(&self) -> Option<Chain> { self.pending.get() }

    fn push(&mut self, links: &ChainLinks, row: RowId) {
        debug_assert!(self.built.get().is_none(), "push into an expanded child");
        self.pending
            .get_mut()
            .as_mut()
            .expect("an unexpanded child holds its pending chain")
            .push(links, row);
    }

    // The chain is taken before `build` re-threads its rows, so no later
    // `pending` (nor `for_each_tuple`) reads it again; until `build`
    // returns, the child is neither pending nor built.
    fn expand(&self, build: impl FnOnce(Chain) -> N) -> &N {
        self.built.get_or_init(|| {
            build(
                self.pending
                    .take()
                    .expect("an unexpanded child holds its pending chain until it is built"),
            )
        })
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

    /// A lazy child over a stand-in node type: the row ids it was built from.
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

    /// The pending chain grows by `push`, then `expand` takes it into the
    /// build, once: the child is built and holds no chain.
    #[test]
    fn expand_takes_the_pending_chain_once() {
        let links = ChainLinks::new(2);
        let mut child = Lazy::from_chain(Chain::start(&links, 0));
        child.push(&links, 1);
        assert!(child.built().is_none());
        let chain = child.pending().expect("not yet expanded");
        assert_eq!(chain.walk(&links).collect::<Vec<_>>(), vec![0, 1]);
        let mut calls = 0;
        let built = child.expand(|chain| {
            calls += 1;
            chain.walk(&links).collect()
        });
        assert_eq!(built, &vec![0, 1]);
        // A second expand returns the same table without building again.
        let again = child.expand(|_| unreachable!("already built"));
        assert!(std::ptr::eq(built, again));
        assert_eq!(calls, 1);
        assert_eq!(child.pending(), None, "an expanded child keeps no chain");
    }

    #[test]
    fn built_mut_reaches_the_expanded_table() {
        let links = ChainLinks::new(1);
        let mut child = Lazy::from_chain(Chain::start(&links, 0));
        assert!(child.built_mut().is_none());
        child.expand(|chain| chain.walk(&links).collect());
        child.built_mut().unwrap().push(1);
        assert_eq!(child.built(), Some(&vec![0, 1]));
    }

    #[test]
    fn own_heap_bytes_is_the_box() {
        let links = ChainLinks::new(1);
        let child = Lazy::from_chain(Chain::start(&links, 0));
        assert_eq!(
            child.own_heap_bytes(),
            std::mem::size_of::<LazyChild<Vec<RowId>>>()
        );
    }

    /// The pending chain is stored in the box itself, 12 bytes with `len`'s
    /// niche for the `Option`, so a lazy child allocates only its box.
    #[test]
    fn the_pending_chain_lives_in_the_box() {
        assert_eq!(std::mem::size_of::<Cell<Option<Chain>>>(), 12);
    }
}
