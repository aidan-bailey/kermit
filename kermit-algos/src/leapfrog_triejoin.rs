use {
    crate::join_algo::JoinAlgo,
    kermit_iters::trie::{TrieIterable, TrieIterator},
};

/// A trait for iterators that implement the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
pub trait LeapfrogTriejoinIterator<'a, KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    /// Initializes the iterator.
    fn init(&mut self) -> Option<&'a KT>;

    /// Proceed to the next key.
    fn leapfrog_next(&mut self) -> Option<&'a KT>;

    /// Proceed to the next matching key.
    fn search(&mut self) -> Option<&'a KT>;

    /// Position the iterator at a least
    /// upper bound for seekKey,
    /// i.e. the least key ≥ seekKey, or
    /// move to end if no such key exists.
    /// The sought key must be ≥ the
    /// key at the current position.
    fn seek(&mut self, seek_key: &KT) -> Option<&'a KT>;

    /// Check if the iterator is at the end.
    fn at_end(&self) -> bool;

    /// Proceed to the first key at the next depth.
    fn open(&mut self) -> Option<&'a KT>;

    /// Proceed to the parent key at the previous depth.
    fn up(&mut self) -> Option<&'a KT>;
}

/// An iterator that performs the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
pub struct LeapfrogTriejoinIter<'a, KT, IT>
where
    KT: PartialOrd + PartialEq + Clone,
    IT: TrieIterator<'a, KT>,
{
    /// The key of the current position.
    pub key: Option<&'a KT>,
    p: usize,
    iters: Vec<Option<IT>>,
    current_iters: Vec<(usize, IT)>,
    iter_indexes_at_variable: Vec<Vec<usize>>,
    depth: usize,
}

impl<'a, KT, IT> LeapfrogTriejoinIter<'a, KT, IT>
where
    KT: PartialOrd + PartialEq + Clone,
    IT: TrieIterator<'a, KT>,
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
            key: None,
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

        for i in &self.iter_indexes_at_variable[self.depth - 1] {
            let iter = self.iters[*i].take();
            self.current_iters
                .push((*i, iter.expect("There should alway be an iterator here")));
        }
    }

    fn k(&self) -> usize { self.current_iters.len() }
}

impl<'a, KT, IT> LeapfrogTriejoinIterator<'a, KT> for LeapfrogTriejoinIter<'a, KT, IT>
where
    KT: PartialOrd + PartialEq + Clone,
    IT: TrieIterator<'a, KT>,
{
    fn init(&mut self) -> Option<&'a KT> {
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

    fn leapfrog_next(&mut self) -> Option<&'a KT> {
        self.key = None;
        self.current_iters[self.p].1.next()?;
        self.p = (self.p + 1) % self.k();
        self.search()
    }

    fn search(&mut self) -> Option<&'a KT> {
        self.key = None;
        let prime_i = if self.p == 0 {
            self.k() - 1
        } else {
            self.p - 1
        };
        let mut x_prime = self.current_iters[prime_i].1.key()?.clone();
        loop {
            let x = self.current_iters[self.p].1.key()?;
            if x == &x_prime {
                self.key = Some(x);
                break self.key;
            }
            x_prime = self.current_iters[self.p].1.seek(&x_prime)?.clone();
            self.p = (self.p + 1) % self.k();
        }
    }

    fn seek(&mut self, seek_key: &KT) -> Option<&'a KT> {
        self.current_iters[self.p].1.seek(seek_key)?;
        if !self.current_iters[self.p].1.at_end() {
            self.p = (self.p + 1) % self.k();
            self.search()
        } else {
            None
        }
    }

    fn at_end(&self) -> bool {
        for (_, iter) in &self.current_iters {
            if iter.at_end() {
                return true;
            }
        }
        false
    }

    fn open(&mut self) -> Option<&'a KT> {
        self.depth += 1;
        self.update_iters();
        for (_, iter) in &mut self.current_iters {
            iter.open()?;
        }
        self.init()
    }

    fn up(&mut self) -> Option<&'a KT> {
        for (_, iter) in &mut self.current_iters {
            iter.up()?;
        }
        self.depth -= 1;
        self.update_iters();
        self.key
    }
}

impl<'a, KT, IT> Iterator for LeapfrogTriejoinIter<'a, KT, IT>
where
    KT: PartialOrd + PartialEq + Clone + 'a,
    IT: TrieIterator<'a, KT>,
{
    type Item = Option<Vec<&'a KT>>;

    fn next(&mut self) -> Option<Self::Item> {
        // If not at top, or leaf, complain
        if self.depth != 0 {
            panic!("Not at top level");
        }

        None

        // if self.index < self.todos.list.len() {
        // let result = Some(&self.todos.list[self.index]);
        // self.index += 1;
        // result
        // } else {
        // None
        // }
    }
}

pub struct LeapfrogTriejoin {}

impl<'a, KT, ITB> JoinAlgo<'a, KT, ITB> for LeapfrogTriejoin
where
    KT: Ord + Clone,
    ITB: TrieIterable<'a, KT>,
{
    fn join(_variables: Vec<usize>, _rel_variables: Vec<Vec<usize>>, _iterables: Vec<&ITB>) {
        print!("Nice!")
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::leapfrog_triejoin::{LeapfrogTriejoinIter, LeapfrogTriejoinIterator},
        kermit_ds::{relation_builder::RelationBuilder, relation_trie::trie_builder::TrieBuilder},
        kermit_iters::trie::TrieIterable,
    };

    #[test]
    fn test_classic() {
        let t1 = TrieBuilder::<i32>::new(1)
            .add_tuples(vec![vec![1], vec![2], vec![3]])
            .build();
        let t2 = TrieBuilder::<i32>::new(1)
            .add_tuples(vec![vec![1], vec![2], vec![3]])
            .build();
        let t1_iter = t1.trie_iter();
        let t2_iter = t2.trie_iter();
        let mut triejoin_iter =
            LeapfrogTriejoinIter::new(vec![0], vec![vec![0], vec![0]], vec![t1_iter, t2_iter]);
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key.unwrap().clone(), 1);
        // assert_eq!(triejoin_iter.next().unwrap(), &2);
        // assert_eq!(triejoin_iter.next().unwrap(), &3);
    }

    #[test]
    fn more_complicated() {
        let r = TrieBuilder::<i32>::new(2)
            .add_tuples(vec![vec![7, 4]])
            .build();
        let s = TrieBuilder::<i32>::new(2)
            .add_tuples(vec![vec![4, 1], vec![4, 4], vec![4, 5], vec![4, 9]])
            .build();
        let t = TrieBuilder::<i32>::new(2)
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
        assert_eq!(triejoin_iter.key.unwrap().clone(), 7);
        assert!(triejoin_iter.next().is_none());
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key.unwrap().clone(), 4);
        assert!(triejoin_iter.next().is_none());
        triejoin_iter.open();
        assert_eq!(triejoin_iter.key.unwrap().clone(), 5);
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
