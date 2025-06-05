use {
    crate::join_algo::JoinAlgo,
    kermit_iters::trie::{TrieIterable, TrieIterator},
};

/// A trait for iterators that implement the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
pub trait LeapfrogTriejoinIterator<'a>: TrieIterator<'a> {
    /// Initializes the iterator.
    fn init(&mut self) -> Option<&'a Self::KT>;

    /// Proceed to the next matching key.
    fn search(&mut self) -> Option<&'a Self::KT>;

    fn leapfrog_next(&mut self) -> Option<&'a Self::KT>;
}

/// An iterator that performs the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
pub struct LeapfrogTriejoinIter<'a, IT>
where
    IT: TrieIterator<'a>,
{
    /// The key of the current position.
    pub(self) stack: Vec<&'a IT::KT>,
    p: usize,
    iters: Vec<Option<IT>>,
    current_iters: Vec<(usize, IT)>,
    iter_indexes_at_variable: Vec<Vec<usize>>,
    depth: usize,
}

impl<'a, IT> LeapfrogTriejoinIter<'a, IT>
where
    IT: TrieIterator<'a>,
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
            stack: Vec::with_capacity(variables.len()),
            p: 0,
            iters,
            current_iters: Vec::new(),
            iter_indexes_at_variable,
            depth: 0,
        }
    }

    fn update_iters(&mut self) {
        while let Some((i, iter)) = self.current_iters.pop() {
            self.iters[i] = Some(iter);
        }

        if self.depth == 0 {
            return;
        }

        for i in &self.iter_indexes_at_variable[self.depth - 1] {
            let iter = self.iters[*i].take();
            self.current_iters
                .push((*i, iter.expect("There should alway be an iterator here")));
        }
    }

    fn k(&self) -> usize { self.current_iters.len() }
}

impl<'a, IT> TrieIterator<'a> for LeapfrogTriejoinIter<'a, IT>
where
    IT: TrieIterator<'a> + 'a,
{
    type KT = IT::KT;

    fn key(&self) -> Option<&'a Self::KT> { self.stack.last().copied() }

    fn open(&mut self) -> Option<&'a Self::KT> {
        self.depth += 1;
        self.update_iters();
        for (_, iter) in &mut self.current_iters {
            iter.open()?;
        }
        self.init()
    }

    fn up(&mut self) -> Option<&'a Self::KT> {
        if self.depth == 0 {
            panic!("Cannot go any more up")
        }
        for (_, iter) in &mut self.current_iters {
            iter.up();
        }
        self.depth -= 1;
        self.update_iters();
        self.key()
    }

    fn next(&mut self) -> Option<&'a Self::KT> {
        self.stack.pop();
        self.current_iters[self.p].1.next()?;
        self.p = (self.p + 1) % self.k();
        self.search()
    }

    fn at_end(&self) -> bool {
        if self.depth == 0 {
            return true;
        }
        for (_, iter) in &self.current_iters {
            if iter.at_end() {
                return true;
            }
        }
        false
    }

    fn seek(&mut self, seek_key: &Self::KT) -> Option<&'a Self::KT> {
        self.current_iters[self.p].1.seek(seek_key)?;
        if !self.current_iters[self.p].1.at_end() {
            self.p = (self.p + 1) % self.k();
            self.search()
        } else {
            None
        }
    }
}

impl<'a, IT> LeapfrogTriejoinIterator<'a> for LeapfrogTriejoinIter<'a, IT>
where
    IT: TrieIterator<'a> + 'a,
{
    fn init(&mut self) -> Option<&'a Self::KT> {
        if !self.at_end() {
            self.current_iters.sort_unstable_by(|a, b| {
                let a_key = a.1.key().expect("Not at root");
                let b_key = b.1.key().expect("Not at root");
                if a_key < b_key {
                    std::cmp::Ordering::Less
                } else if a_key > b_key {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            self.p = 0;
            self.search()
        } else {
            None
        }
    }

    fn search(&mut self) -> Option<&'a Self::KT> {
        self.stack.pop();
        let prime_i = if self.p == 0 {
            self.k() - 1
        } else {
            self.p - 1
        };
        let mut x_prime = self.current_iters[prime_i].1.key()?.clone();
        loop {
            let x = self.current_iters[self.p].1.key()?;
            if x == &x_prime {
                self.stack.push(x);
                break self.key();
            }
            x_prime = self.current_iters[self.p].1.seek(&x_prime)?.clone();
            self.p = (self.p + 1) % self.k();
        }
    }

    fn leapfrog_next(&mut self) -> Option<&'a Self::KT> { TrieIterator::next(self) }
}

impl<'a, IT> Iterator for LeapfrogTriejoinIter<'a, IT>
where
    IT: TrieIterator<'a> + 'a,
{
    type Item = Vec<&'a IT::KT>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.depth == 0 {
                self.open()?;
                if self.depth == self.stack.capacity() {
                    return Some(self.stack.clone());
                }
            } else if self.depth == self.stack.capacity() {
                // At leaf
                TrieIterator::next(self);
                if self.at_end() {
                    // find next path
                    while self.at_end() {
                        if self.depth == 1 {
                            return None;
                        }
                        self.up();
                        TrieIterator::next(self);
                    }
                } else if self.stack.is_empty() {
                    panic!("Stack should never be empty at leaf")
                } else {
                    return Some(self.stack.clone());
                }
            } else {
                while self.depth < self.stack.capacity() {
                    self.open();
                }
                return Some(self.stack.clone());
            }
        }
    }
}

pub struct LeapfrogTriejoin {}

impl<ITB> JoinAlgo<ITB> for LeapfrogTriejoin
where
    ITB: TrieIterable,
{
    fn join(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&ITB>,
    ) -> Vec<Vec<ITB::KT>> {
        let trie_iters: Vec<_> = iterables.into_iter().map(|i| i.trie_iter()).collect();
        LeapfrogTriejoinIter::new(variables, rel_variables, trie_iters)
            .map(|v| v.into_iter().cloned().collect::<Vec<_>>())
            .collect::<Vec<_>>()
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::leapfrog_triejoin::{LeapfrogTriejoinIter, LeapfrogTriejoinIterator},
        kermit_ds::{
            ds::relation_trie::RelationTrie,
            relation_builder::{Builder, RelationBuilder},
        },
        kermit_iters::trie::{TrieIterable, TrieIterator},
    };

    #[test]
    fn test_classic() {
        let t1: RelationTrie<i32> = Builder::<RelationTrie<i32>>::new(1)
            .add_tuples(vec![vec![1], vec![2], vec![3]])
            .build();
        let t2 = Builder::<RelationTrie<i32>>::new(1)
            .add_tuples(vec![vec![1], vec![2], vec![3]])
            .build();
        let t1_iter = t1.trie_iter();
        let t2_iter = t2.trie_iter();
        let mut triejoin_iter =
            LeapfrogTriejoinIter::new(vec![0], vec![vec![0], vec![0]], vec![t1_iter, t2_iter]);
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 1_i32);
        triejoin_iter.leapfrog_next();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 2);
        triejoin_iter.leapfrog_next();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 3);
        triejoin_iter.leapfrog_next();
        assert!(triejoin_iter.at_end());
        triejoin_iter.up();
        assert!(triejoin_iter.at_end());
        let res = triejoin_iter
            .map(|v| v.into_iter().copied().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        assert_eq!(res, vec![vec![1_i32], vec![2_i32], vec![3_i32]]);
    }

    #[test]
    fn more_complicated() {
        let r = Builder::<RelationTrie<i32>>::new(2)
            .add_tuples(vec![vec![7, 4]])
            .build();
        let s = Builder::<RelationTrie<i32>>::new(2)
            .add_tuples(vec![vec![4, 1], vec![4, 4], vec![4, 5], vec![4, 9]])
            .build();
        let t = Builder::<RelationTrie<i32>>::new(2)
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
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 7);
        triejoin_iter.leapfrog_next();
        assert!(triejoin_iter.at_end());
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 4);
        triejoin_iter.leapfrog_next();
        assert!(triejoin_iter.at_end());
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 5);
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
    // fn test_inputs_outputs(cardinality: usize, inputs: Vec<Vec<Vec<i32>>>,
    // expected: Vec<Vec<i32>>) { let tries: Vec<_> = inputs
    // .into_iter()
    // .map(|input|
    // TrieBuilder::<i32>::new(cardinality).add_tuples(input).build())
    // .collect();
    // let res = leapfrog_triejoin(tries.iter().collect());
    // assert_eq!(res, expected);
    // }
}
