use {
    crate::{
        join_algo::JoinAlgo,
        leapfrog_join::{LeapfrogJoinIter, LeapfrogJoinIterator},
    },
    kermit_iters::{LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
    kermit_parser::{JoinQuery, Term},
    std::collections::HashMap,
};

/// Extension of [`LeapfrogJoinIterator`] with trie navigation for the
/// [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
pub trait LeapfrogTriejoinIterator: LeapfrogJoinIterator {
    /// Descends one level in the trie, opening child iterators at the current
    /// key and initializing the leapfrog join at the new depth.
    fn triejoin_open(&mut self) -> bool;

    /// Ascends one level, restoring the leapfrog join state at the parent
    /// depth.
    fn triejoin_up(&mut self) -> bool;
}

/// An iterator that performs the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
///
/// Coordinates multiple trie iterators (one per body predicate) to compute a
/// multi-way join. At each variable/depth level, only the iterators that
/// participate in that variable are active in the inner [`LeapfrogJoinIter`].
/// Iterators are swapped in and out of the leapfrog as the depth changes.
pub struct LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    /// Number of variables in the join (determines maximum depth).
    arity: usize,
    /// Pool of trie iterators, indexed by body predicate position. `None` when
    /// an iterator is currently borrowed by the leapfrog.
    iterator_pool: Vec<Option<IT>>,
    /// Tracks which iterators (by index into `iterator_pool`) are currently in
    /// the leapfrog, so they can be returned on depth change.
    active_iter_indices: Vec<usize>,
    /// For each variable index, the list of iterator indices that participate
    /// at that depth.
    variable_to_iter_map: Vec<Vec<usize>>,
    /// Current depth in the join (0 = not yet opened, 1..arity = active).
    depth: usize,
    /// The inner leapfrog join operating at the current depth.
    leapfrog: LeapfrogJoinIter<IT>,
}

impl<IT> LeapfrogJoinIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    fn leapfrog_next(&mut self) -> Option<usize> {
        self.leapfrog.leapfrog_next()
    }

    fn key(&self) -> Option<usize> {
        if self.depth == 0 {
            None
        } else {
            self.leapfrog.key()
        }
    }

    fn leapfrog_init(&mut self) -> bool {
        self.leapfrog.leapfrog_init()
    }

    fn leapfrog_search(&mut self) -> bool {
        self.leapfrog.leapfrog_search()
    }

    fn at_end(&self) -> bool {
        if self.depth == 0 {
            return true;
        }
        self.leapfrog.at_end()
    }

    fn leapfrog_seek(&mut self, seek_key: usize) -> bool {
        self.leapfrog.leapfrog_seek(seek_key)
    }
}

impl<IT> LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    /// Construct a new `LeapfrogTriejoinIter` with the given iterators.
    ///
    /// Q(a, b, c) = R(a, b) S(b, c), T(a, c)
    /// variables = [a, b, c]
    /// rel_variables = [[a, b], [b, c], [a, c]]
    ///
    /// # Arguments
    /// * `variables` - The variables and their ordering.
    /// * `rel_variables` - The variables in their relations.
    /// * `iters` - Trie iterators.
    pub fn new(variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iters: Vec<IT>) -> Self {
        // Build the variable-to-iterator lookup table. For each variable index,
        // collect the indices (into `iters` / `rel_variables`) of every relation
        // that mentions that variable. These indices tell the triejoin which
        // iterators to activate in the leapfrog at each depth level.
        let mut variable_to_iter_map: Vec<Vec<usize>> = Vec::new();
        for v in &variables {
            let mut iters_at_level_v: Vec<usize> = Vec::new();
            for (r_i, r) in rel_variables.iter().enumerate() {
                if r.contains(v) {
                    iters_at_level_v.push(r_i);
                }
            }
            variable_to_iter_map.push(iters_at_level_v);
        }

        let iterator_pool = iters.into_iter().map(Some).collect();

        LeapfrogTriejoinIter {
            iterator_pool,
            active_iter_indices: Vec::new(),
            variable_to_iter_map,
            arity: variables.len(),
            depth: 0,
            leapfrog: LeapfrogJoinIter::new(vec![]),
        }
    }

    /// Swaps iterators between the pool (`self.iterator_pool`) and the active
    /// leapfrog (`self.leapfrog`) based on which iterators participate at
    /// the current depth.
    fn update_iters(&mut self) {
        while let Some(i) = self.active_iter_indices.pop() {
            let iter = self
                .leapfrog
                .iterators
                .pop()
                .expect("There should always be an iterator here");
            self.iterator_pool[i] = Some(iter);
        }

        if self.depth == 0 {
            return;
        }

        let mut next_iters =
            Vec::<IT>::with_capacity(self.variable_to_iter_map[self.depth - 1].len());
        for i in &self.variable_to_iter_map[self.depth - 1] {
            let iter = self.iterator_pool[*i]
                .take()
                .expect("There is an iterator here");
            next_iters.push(iter);
            self.active_iter_indices.push(*i);
        }
        self.leapfrog = LeapfrogJoinIter::new(next_iters);
    }
}

impl<IT> LeapfrogTriejoinIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    fn triejoin_open(&mut self) -> bool {
        if self.depth == self.arity {
            return false;
        }
        self.depth += 1;
        self.update_iters();
        for iter in &mut self.leapfrog.iterators {
            if !iter.open() {
                return false;
            }
        }
        self.leapfrog_init()
    }

    fn triejoin_up(&mut self) -> bool {
        if self.depth == 0 {
            return false;
        }
        for iter in &mut self.leapfrog.iterators {
            assert!(
                iter.up(),
                "Iterator must be able to move up from non-root depth"
            );
        }
        self.depth -= 1;
        self.update_iters();
        true
    }
}

impl<IT> TrieIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    fn open(&mut self) -> bool {
        self.triejoin_open()
    }

    fn up(&mut self) -> bool {
        self.triejoin_up()
    }
}

impl<IT> LinearIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    fn key(&self) -> Option<usize> {
        LeapfrogJoinIterator::key(self)
    }

    fn next(&mut self) -> Option<usize> {
        self.leapfrog_next()
    }

    fn seek(&mut self, seek_key: usize) -> bool {
        self.leapfrog_seek(seek_key)
    }

    fn at_end(&self) -> bool {
        LeapfrogJoinIterator::at_end(self)
    }
}

impl<IT> IntoIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    type IntoIter = TrieIteratorWrapper<Self>;
    type Item = Vec<usize>;

    fn into_iter(self) -> Self::IntoIter {
        let arity = self.arity;
        TrieIteratorWrapper::with_arity(self, arity)
    }
}

/// Indexes the variables in a [`JoinQuery`] for the triejoin algorithm.
///
/// Performs three passes over the query:
/// 1. Register head variables — assigns each unique name a numeric index,
///    starting from 0. Head variables come first so the output tuple order
///    matches the head declaration.
/// 2. Register body-only variables — any variable not already seen in the head
///    gets the next available index.
/// 3. Build per-relation variable index lists — for each body predicate,
///    collect the indices of the variables it contains.
///
/// Placeholders (`_`) and atoms are skipped in all passes.
///
/// Returns `(variables, rel_variables)` where `variables` is `0..num_vars` and
/// `rel_variables[i]` lists the variable indices for body predicate `i`.
fn build_variable_index(query: &JoinQuery) -> (Vec<usize>, Vec<Vec<usize>>) {
    let mut var_to_index: HashMap<String, usize> = HashMap::new();
    let mut next_index: usize = 0;

    // Helper: assigns a fresh index to a variable name on first sight,
    // returns the existing index on subsequent encounters.
    let register_var = |name: &str, map: &mut HashMap<String, usize>, next: &mut usize| {
        *map.entry(name.to_string()).or_insert_with(|| {
            let idx = *next;
            *next += 1;
            idx
        })
    };

    // Pass 1: head variables — establishes output tuple ordering.
    for t in &query.head.terms {
        if let Term::Var(ref vname) = t {
            let _ = register_var(vname, &mut var_to_index, &mut next_index);
        }
    }

    // Pass 2: body-only variables — any variable not already seen in the head.
    for pred in &query.body {
        for t in &pred.terms {
            if let Term::Var(ref vname) = t {
                let _ = register_var(vname, &mut var_to_index, &mut next_index);
            }
        }
    }

    let variables: Vec<usize> = (0..var_to_index.len()).collect();

    // Pass 3: build per-relation variable index lists. Placeholders and atoms
    // are skipped — they occupy trie levels but don't bind a join variable.
    let mut rel_variables: Vec<Vec<usize>> = Vec::with_capacity(query.body.len());
    for pred in &query.body {
        let mut rel_vars_for_pred: Vec<usize> = Vec::new();
        for t in &pred.terms {
            if let Term::Var(ref vname) = t {
                if let Some(idx) = var_to_index.get(vname) {
                    rel_vars_for_pred.push(*idx);
                }
            }
        }
        rel_variables.push(rel_vars_for_pred);
    }

    (variables, rel_variables)
}

/// Entry point for the Leapfrog Triejoin algorithm, implementing
/// [`JoinAlgo`](crate::JoinAlgo) for any [`TrieIterable`] data structure.
pub struct LeapfrogTriejoin {}

impl<DS> JoinAlgo<DS> for LeapfrogTriejoin
where
    DS: TrieIterable,
{
    fn join_iter(
        query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> {
        let (variables, rel_variables) = build_variable_index(&query);

        let trie_iters: Vec<_> = query
            .body
            .iter()
            .map(|pred| {
                let ds = datastructures
                    .get(&pred.name)
                    .expect("Missing datastructure for predicate name");
                ds.trie_iter()
            })
            .collect();

        LeapfrogTriejoinIter::new(variables, rel_variables, trie_iters).into_iter()
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::{
            leapfrog_join::LeapfrogJoinIterator,
            leapfrog_triejoin::{LeapfrogTriejoinIter, LeapfrogTriejoinIterator},
        },
        kermit_ds::{Relation, TreeTrie},
        kermit_iters::TrieIterable,
    };

    /// Collect triejoin results end-to-end via `into_iter().collect()`.
    fn triejoin_collect(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, relations: Vec<&TreeTrie>,
    ) -> Vec<Vec<usize>> {
        let iters: Vec<_> = relations.iter().map(|r| r.trie_iter()).collect();
        LeapfrogTriejoinIter::new(variables, rel_variables, iters)
            .into_iter()
            .collect()
    }

    // -- Original manual-stepping tests --

    #[test]
    fn test_classic() {
        let t1 = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let t2 = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let t1_iter = t1.trie_iter();
        let t2_iter = t2.trie_iter();
        let mut triejoin_iter =
            LeapfrogTriejoinIter::new(vec![0], vec![vec![0], vec![0]], vec![t1_iter, t2_iter]);
        triejoin_iter.triejoin_open();
        assert_eq!(triejoin_iter.key(), Some(1));
        assert_eq!(triejoin_iter.leapfrog_next(), Some(2));
        assert_eq!(triejoin_iter.leapfrog_next(), Some(3));
        triejoin_iter.leapfrog_next();
        assert!(triejoin_iter.at_end());
        triejoin_iter.triejoin_up();
        assert!(triejoin_iter.at_end());
        let res = triejoin_iter.into_iter().collect::<Vec<_>>();
        assert_eq!(res, vec![vec![1], vec![2], vec![3]]);
    }

    #[test]
    fn more_complicated() {
        let r = TreeTrie::from_tuples(2.into(), vec![vec![7, 4]]);
        let s = TreeTrie::from_tuples(
            2.into(),
            vec![vec![4, 1], vec![4, 4], vec![4, 5], vec![4, 9]],
        );
        let t = TreeTrie::from_tuples(2.into(), vec![vec![7, 2], vec![7, 3], vec![7, 5]]);
        let r_iter = r.trie_iter();
        let s_iter = s.trie_iter();
        let t_iter = t.trie_iter();
        let mut triejoin_iter = LeapfrogTriejoinIter::new(
            vec![0, 1, 2],
            vec![vec![0, 1], vec![1, 2], vec![0, 2]],
            vec![r_iter, s_iter, t_iter],
        );
        triejoin_iter.triejoin_open();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 7);
        triejoin_iter.leapfrog_next();
        assert!(triejoin_iter.at_end());
        triejoin_iter.triejoin_open();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 4);
        triejoin_iter.leapfrog_next();
        assert!(triejoin_iter.at_end());
        triejoin_iter.triejoin_open();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 5);
    }

    #[test]
    fn chain() {
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![4, 6], vec![5, 7]]);
        let r_iter = r.trie_iter();
        let s_iter = s.trie_iter();
        let t_iter = t.trie_iter();
        let mut triejoin_iter = LeapfrogTriejoinIter::new(
            vec![0, 1, 2, 3],
            vec![vec![0, 1], vec![1, 2], vec![2, 3]],
            vec![r_iter, s_iter, t_iter],
        );
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(1));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(2));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(4));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(6));

        assert!(triejoin_iter.triejoin_up());
        assert!(triejoin_iter.triejoin_up());
        assert!(triejoin_iter.triejoin_up());

        assert_eq!(triejoin_iter.leapfrog_next(), Some(2));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(3));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(5));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(7));
    }

    // -- End-to-end collect tests --

    #[test]
    fn unary_intersection() {
        let r = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let s = TreeTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]);
        assert_eq!(
            triejoin_collect(vec![0], vec![vec![0], vec![0]], vec![&r, &s]),
            vec![vec![2], vec![3]],
        );
    }

    #[test]
    fn unary_no_match() {
        let r = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2]]);
        let s = TreeTrie::from_tuples(1.into(), vec![vec![3], vec![4]]);
        assert_eq!(
            triejoin_collect(vec![0], vec![vec![0], vec![0]], vec![&r, &s]),
            Vec::<Vec<usize>>::new(),
        );
    }

    #[test]
    fn unary_empty_relation() {
        let r = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let s = TreeTrie::from_tuples(1.into(), vec![]);
        assert_eq!(
            triejoin_collect(vec![0], vec![vec![0], vec![0]], vec![&r, &s]),
            Vec::<Vec<usize>>::new(),
        );
    }

    #[test]
    fn unary_single_match() {
        let r = TreeTrie::from_tuples(1.into(), vec![vec![5]]);
        let s = TreeTrie::from_tuples(1.into(), vec![vec![5]]);
        assert_eq!(
            triejoin_collect(vec![0], vec![vec![0], vec![0]], vec![&r, &s]),
            vec![vec![5]],
        );
    }

    #[test]
    fn three_way_unary() {
        let r = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3], vec![4]]);
        let s = TreeTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![5]]);
        let t = TreeTrie::from_tuples(1.into(), vec![vec![3], vec![4], vec![5]]);
        assert_eq!(
            triejoin_collect(vec![0], vec![vec![0], vec![0], vec![0]], vec![&r, &s, &t]),
            vec![vec![3]],
        );
    }

    #[test]
    fn binary_natural_join() {
        // R(a,b) ⋈ S(b,c)
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]], vec![&r, &s]),
            vec![vec![1, 2, 4], vec![1, 3, 5]],
        );
    }

    #[test]
    fn no_match_at_shared_variable() {
        // R(a,b) ⋈ S(a,c) where a values are disjoint — mismatch at the
        // depth where both relations participate, producing correct empty result
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![3, 4]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![0, 2]], vec![&r, &s]),
            Vec::<Vec<usize>>::new(),
        );
    }

    #[test]
    fn triangle_join_collect() {
        // Q(a,b,c) :- R(a,b), S(b,c), T(a,c)
        let r = TreeTrie::from_tuples(2.into(), vec![vec![7, 4]]);
        let s = TreeTrie::from_tuples(
            2.into(),
            vec![vec![4, 1], vec![4, 4], vec![4, 5], vec![4, 9]],
        );
        let t = TreeTrie::from_tuples(2.into(), vec![vec![7, 2], vec![7, 3], vec![7, 5]]);
        assert_eq!(
            triejoin_collect(
                vec![0, 1, 2],
                vec![vec![0, 1], vec![1, 2], vec![0, 2]],
                vec![&r, &s, &t],
            ),
            vec![vec![7, 4, 5]],
        );
    }

    #[test]
    fn star_join() {
        // Q(a,b,c) :- R(a,b), S(a,c) — star pattern on variable a
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![1, 5], vec![1, 6], vec![2, 7]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![0, 2]], vec![&r, &s]),
            vec![
                vec![1, 2, 5],
                vec![1, 2, 6],
                vec![1, 3, 5],
                vec![1, 3, 6],
                vec![2, 4, 7],
            ],
        );
    }

    #[test]
    fn self_join() {
        // R(a,b) ⋈ R(b,c) — self-join where every a value has a matching b chain
        let r1 = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 1]]);
        let r2 = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 1]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]], vec![&r1, &r2]),
            vec![vec![1, 2, 1], vec![2, 1, 2]],
        );
    }

    #[test]
    fn four_way_chain() {
        // Q(a,b,c,d,e) :- R(a,b), S(b,c), T(c,d), U(d,e)
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![2, 3]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![3, 4]]);
        let u = TreeTrie::from_tuples(2.into(), vec![vec![4, 5]]);
        assert_eq!(
            triejoin_collect(
                vec![0, 1, 2, 3, 4],
                vec![vec![0, 1], vec![1, 2], vec![2, 3], vec![3, 4]],
                vec![&r, &s, &t, &u],
            ),
            vec![vec![1, 2, 3, 4, 5]],
        );
    }

    #[test]
    fn single_relation_passthrough() {
        // Join with just one relation returns all its tuples
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(
            triejoin_collect(vec![0, 1], vec![vec![0, 1]], vec![&r]),
            vec![vec![1, 2], vec![3, 4]],
        );
    }

    #[test]
    fn column_trie_binary_join() {
        use kermit_ds::ColumnTrie;
        let r = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3]]);
        let s = ColumnTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        let r_iter = r.trie_iter();
        let s_iter = s.trie_iter();
        let result: Vec<Vec<usize>> = LeapfrogTriejoinIter::new(
            vec![0, 1, 2],
            vec![vec![0, 1], vec![1, 2]],
            vec![r_iter, s_iter],
        )
        .into_iter()
        .collect();
        assert_eq!(result, vec![vec![1, 2, 4], vec![1, 3, 5]]);
    }

    #[test]
    fn binary_no_match_regression() {
        // Regression: R(a,b) ⋈ S(b,c) where b values are disjoint.
        // Previously emitted partial tuple [1] because triejoin_open incremented
        // depth before validating the leapfrog at depth 2 (variable b), and
        // TrieIteratorWrapper returned the incomplete stack.
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![3, 4]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]], vec![&r, &s]),
            Vec::<Vec<usize>>::new(),
        );
    }

    #[test]
    fn self_join_with_dead_ends() {
        // Regression: R(a,b) ⋈ R(b,c) where some a values don't chain.
        // R = {(1,2),(2,3),(3,4)} — only a=1→b=2→c=3 and a=2→b=3→c=4 produce
        // full 3-tuples. a=3→b=4 has no continuation in R(b=4,...).
        // Previously emitted spurious partial tuples like [3].
        let r1 = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3], vec![3, 4]]);
        let r2 = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3], vec![3, 4]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]], vec![&r1, &r2]),
            vec![vec![1, 2, 3], vec![2, 3, 4]],
        );
    }

    #[test]
    fn chain_join_collect() {
        // Q(a,b,c,d) :- R(a,b), S(b,c), T(c,d) — end-to-end collect
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![4, 6], vec![5, 7]]);
        assert_eq!(
            triejoin_collect(
                vec![0, 1, 2, 3],
                vec![vec![0, 1], vec![1, 2], vec![2, 3]],
                vec![&r, &s, &t],
            ),
            vec![vec![1, 2, 4, 6], vec![2, 3, 5, 7]],
        );
    }

    // #[test_case(
    // vec!["tests/data/a.csv", "tests/data/b.csv", "tests/data/c.csv"],
    // vec![vec![8]];
    // "a,b,c"
    // )]
    // #[test_case(
    // vec!["tests/data/onetoten.csv", "tests/data/onetoten.csv",
    // "tests/data/onetoten.csv"], vec![vec![1], vec![2], vec![3], vec![4],
    // vec![5], vec![6], vec![7], vec![8], vec![9], vec![10]]; "onetoten x
    // 3" )]
    // #[test_case(
    // vec!["tests/data/col_a.csv", "tests/data/col_b.csv",
    // "tests/data/col_c.csv"], vec![vec![7], vec![10], vec![20]];
    // "col_a, col_b, col_c"
    // )]
    // fn test_files(file_paths: Vec<&'static str>, expected: Vec<Vec<i32>>) {
    // let tries: Vec<_> = file_paths
    // .iter()
    // .map(|file_path| {
    // TrieBuilder::<i32>::new(1)
    // .from_file(file_path)
    // .unwrap()
    // .build()
    // })
    // .collect();
    // let res = leapfrog_triejoin(tries.iter().collect());
    // assert_eq!(res, expected);
    // }
    //
    // #[test_case(
    // 1,
    // vec![
    // vec![vec![1], vec![2], vec![3]],
    // vec![vec![1], vec![2], vec![3]]
    // ],
    // vec![vec![1], vec![2], vec![3]];
    // "1-ary"
    // )]
    // fn test_inputs_outputs(arity: usize, inputs: Vec<Vec<Vec<i32>>>,
    // expected: Vec<Vec<i32>>) { let tries: Vec<_> = inputs
    // .into_iter()
    // .map(|input|
    // TrieBuilder::<i32>::new(arity).add_tuples(input).build())
    // .collect();
    // let res = leapfrog_triejoin(tries.iter().collect());
    // assert_eq!(res, expected);
    // }
}
