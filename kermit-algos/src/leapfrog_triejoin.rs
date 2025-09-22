use {
    crate::{
        join_algo::JoinAlgo,
        leapfrog_join::{LeapfrogJoinIter, LeapfrogJoinIterator},
    },
    kermit_iters::{LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
};

/// A trait for iterators that implement the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
pub trait LeapfrogTriejoinIterator: LeapfrogJoinIterator {
    fn triejoin_open(&mut self) -> bool;

    fn triejoin_up(&mut self) -> bool;
}

/// An iterator that performs the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
pub struct LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    /// The key of the current position.
    arity: usize,
    iters: Vec<Option<IT>>,
    current_iters_indexes: Vec<usize>,
    iter_indexes_at_variable: Vec<Vec<usize>>,
    depth: usize,
    leapfrog: LeapfrogJoinIter<IT>,
}

impl<IT> LeapfrogJoinIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    type KT = IT::KT;

    fn leapfrog_next(&mut self) -> Option<Self::KT> { self.leapfrog.leapfrog_next() }

    fn key(&self) -> Option<Self::KT> {
        if self.depth == 0 {
            None
        } else {
            self.leapfrog.key()
        }
    }

    fn leapfrog_init(&mut self) -> bool { self.leapfrog.leapfrog_init() }

    fn leapfrog_search(&mut self) -> bool { self.leapfrog.leapfrog_search() }

    fn at_end(&self) -> bool {
        if self.depth == 0 {
            return true;
        }
        self.leapfrog.at_end()
    }

    fn leapfrog_seek(&mut self, seek_key: Self::KT) -> bool {
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
        let mut iter_indexes_at_variable: Vec<Vec<usize>> = Vec::new();
        for v in &variables {
            let mut iters_at_level_v: Vec<usize> = Vec::new();
            for (r_i, r) in rel_variables.iter().enumerate() {
                if r.contains(v) {
                    iters_at_level_v.push(r_i);
                }
            }
            iter_indexes_at_variable.push(iters_at_level_v);
        }

        let iters = iters.into_iter().map(Some).collect();

        LeapfrogTriejoinIter {
            iters,
            current_iters_indexes: Vec::new(),
            iter_indexes_at_variable,
            arity: variables.len(),
            depth: 0,
            leapfrog: LeapfrogJoinIter::new(vec![]),
        }
    }

    fn update_iters(&mut self) {
        while let Some(i) = self.current_iters_indexes.pop() {
            let iter = self
                .leapfrog
                .iterators
                .pop()
                .expect("There should always be an iterator here");
            self.iters[i] = Some(iter);
        }

        if self.depth == 0 {
            return;
        }

        let mut next_iters =
            Vec::<IT>::with_capacity(self.iter_indexes_at_variable[self.depth - 1].len());
        for i in &self.iter_indexes_at_variable[self.depth - 1] {
            let iter = self.iters[*i].take().expect("There is an iterator here");
            next_iters.push(iter);
            self.current_iters_indexes.push(*i);
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
            assert!(iter.up());
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
    fn open(&mut self) -> bool { self.triejoin_open() }

    fn up(&mut self) -> bool { self.triejoin_up() }
}

impl<IT> LinearIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    type KT = IT::KT;

    fn key(&self) -> Option<Self::KT> { LeapfrogJoinIterator::key(self) }

    fn next(&mut self) -> Option<Self::KT> { self.leapfrog_next() }

    fn seek(&mut self, seek_key: Self::KT) -> bool { self.leapfrog_seek(seek_key) }

    fn at_end(&self) -> bool { LeapfrogJoinIterator::at_end(self) }
}

impl<IT> IntoIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    type IntoIter = TrieIteratorWrapper<Self>;
    type Item = Vec<IT::KT>;

    fn into_iter(self) -> Self::IntoIter { TrieIteratorWrapper::new(self) }
}

pub struct LeapfrogTriejoin {}

impl<ITB> JoinAlgo<ITB> for LeapfrogTriejoin
where
    ITB: TrieIterable,
{
    fn join_iter(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&ITB>,
    ) -> impl Iterator<Item = Vec<ITB::KT>> {
        let trie_iters: Vec<_> = iterables.into_iter().map(|i| i.trie_iter()).collect();
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
        kermit_ds::{Relation, RelationBuilder, TreeTrie},
        kermit_iters::TrieIterable,
    };

    #[test]
    fn test_classic() {
        let t1 = TreeTrie::<i32>::builder(1.into())
            .add_tuples(vec![vec![1], vec![2], vec![3]])
            .build();
        let t2 = TreeTrie::<i32>::builder(1.into())
            .add_tuples(vec![vec![1], vec![2], vec![3]])
            .build();
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
        assert_eq!(res, vec![vec![1_i32], vec![2_i32], vec![3_i32]]);
    }

    #[test]
    fn more_complicated() {
        let r = TreeTrie::<i32>::builder(2.into())
            .add_tuples(vec![vec![7, 4]])
            .build();
        let s = TreeTrie::<i32>::builder(2.into())
            .add_tuples(vec![vec![4, 1], vec![4, 4], vec![4, 5], vec![4, 9]])
            .build();
        let t = TreeTrie::<i32>::builder(2.into())
            .add_tuples(vec![vec![7, 2], vec![7, 3], vec![7, 5]])
            .build();
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
        let r = TreeTrie::<i32>::builder(2.into())
            .add_tuples(vec![vec![1, 2], vec![2, 3]])
            .build();
        let s = TreeTrie::<i32>::builder(2.into())
            .add_tuples(vec![vec![2, 4], vec![3, 5]])
            .build();
        let t = TreeTrie::<i32>::builder(2.into())
            .add_tuples(vec![vec![4, 6], vec![5, 7]])
            .build();
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
