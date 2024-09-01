use {crate::iters::trie::TrieIterator, std::marker::PhantomData};

/// A trait for iterators that implement the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
pub trait LeapfrogTriejoinIterator<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    /// Initializes the iterator.
    fn init(&mut self) -> Option<&KT>;

    /// Proceed to the next key.
    fn next(&mut self) -> Option<&KT>;

    /// Proceed to the next matching key.
    fn search(&mut self) -> Option<&KT>;

    /// Position the iterator at a least
    /// upper bound for seekKey,
    /// i.e. the least key ≥ seekKey, or
    /// move to end if no such key exists.
    /// The sought key must be ≥ the
    /// key at the current position.
    fn seek(&mut self, seek_key: &KT) -> Option<&KT>;

    /// Check if the iterator is at the end.
    fn at_end(&self) -> bool;

    /// Proceed to the first key at the next depth.
    fn open(&mut self) -> Option<&KT>;

    /// Proceed to the parent key at the previous depth.
    fn up(&mut self) -> Option<&KT>;
}

/// An iterator that performs the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
pub struct LeapfrogTriejoinIter<'a, KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    /// The key of the current position.
    pub key: Option<KT>,
    p: usize,
    iters: Vec<Option<Box<dyn TrieIterator<KT> + 'a>>>,
    current_iters: Vec<(usize, Box<dyn TrieIterator<KT> + 'a>)>,
    iter_indexes_at_variable: Vec<Vec<usize>>,
    depth: usize,
    phantom: PhantomData<KT>,
}

impl<'a, KT> LeapfrogTriejoinIter<'a, KT>
where
    KT: PartialOrd + PartialEq + Clone,
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
    pub fn new(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
        iters: Vec<Box<dyn TrieIterator<KT> + 'a>>,
    ) -> Self {
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
            phantom: PhantomData,
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

impl<'a, KT: PartialOrd + PartialEq + Clone> LeapfrogTriejoinIterator<KT>
    for LeapfrogTriejoinIter<'a, KT>
{
    fn init(&mut self) -> Option<&KT> {
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

    fn next(&mut self) -> Option<&KT> {
        self.key = None;
        self.current_iters[self.p].1.next()?;
        self.p = (self.p + 1) % self.k();
        self.search()
    }

    fn search(&mut self) -> Option<&KT> {
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
                self.key = Some(x.clone());
                break self.key.as_ref();
            }
            x_prime = self.current_iters[self.p].1.seek(&x_prime)?.clone();
            self.p = (self.p + 1) % self.k();
        }
    }

    fn seek(&mut self, seek_key: &KT) -> Option<&KT> {
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

    fn open(&mut self) -> Option<&KT> {
        self.depth += 1;
        self.update_iters();
        for (_, iter) in &mut self.current_iters {
            iter.open()?;
        }
        self.init()
    }

    fn up(&mut self) -> Option<&KT> {
        for (_, iter) in &mut self.current_iters {
            iter.up()?;
        }
        self.depth -= 1;
        self.update_iters();
        self.key.as_ref()
    }
}
