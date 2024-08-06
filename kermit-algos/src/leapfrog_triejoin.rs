use {kermit_iters::trie::TrieIterator, std::marker::PhantomData};

pub trait LeapfrogTriejoinIterator<KT: PartialOrd + PartialEq + Clone> {
    fn init(&mut self) -> Option<&KT>;
    fn next(&mut self) -> Option<&KT>;
    fn search(&mut self) -> Option<&KT>;
    fn seek(&mut self, seek_key: &KT) -> Option<&KT>;
    fn at_end(&self) -> bool;
    fn open(&mut self) -> Option<&KT>;
    fn up(&mut self) -> Option<&KT>;
}

pub struct LeapfrogTriejoinIter<KT: PartialOrd + PartialEq + Clone, IT: TrieIterator<KT>> {
    pub key: Option<KT>,
    p: usize,
    iters: Vec<Option<IT>>,
    current_iters: Vec<(usize, IT)>,
    iter_indexes_at_variable: Vec<Vec<usize>>,
    depth: usize,
    phantom: PhantomData<KT>,
}

impl<KT: PartialOrd + PartialEq + Clone, IT: TrieIterator<KT>> LeapfrogTriejoinIter<KT, IT> {
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

        let iters = iters.into_iter().map(|iter| Some(iter)).collect();

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

impl<KT: PartialOrd + PartialEq + Clone, IT: TrieIterator<KT>> LeapfrogTriejoinIterator<KT>
    for LeapfrogTriejoinIter<KT, IT>
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
