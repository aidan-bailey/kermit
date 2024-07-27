use std::marker::PhantomData;

use kermit_iters::trie::{TrieIterable, TrieIterator};

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
    iters: Vec<IT>,
    phantom: PhantomData<KT>,
}

impl<KT: PartialOrd + PartialEq + Clone, IT: TrieIterator<KT>> LeapfrogTriejoinIter<KT, IT> {
    pub fn new(iters: Vec<IT>) -> Self {
        let mut iter = LeapfrogTriejoinIter {
            key: None,
            p: 0,
            iters,
            phantom: PhantomData,
        };
        iter.init();
        iter
    }

    fn k(&self) -> usize {
        self.iters.len()
    }
}

impl<KT: PartialOrd + PartialEq + Clone, IT: TrieIterator<KT>> LeapfrogTriejoinIterator<KT>
    for LeapfrogTriejoinIter<KT, IT>
{
    fn init(&mut self) -> Option<&KT> {
        if !self.at_end() {
            self.iters.sort_unstable_by(|a, b| {
                let a_key = a.key().expect("Not at root");
                let b_key = b.key().expect("Not at root");
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
        self.iters[self.p].next()?;
        if !self.iters[self.p].at_end() {
            self.p = if self.p == self.k() - 1 {
                0
            } else {
                self.p + 1
            };
            self.search()
        } else {
            None
        }
    }

    fn search(&mut self) -> Option<&KT> {
        self.key = None;
        let prime_i = if self.p == 0 {
            self.k() - 1
        } else {
            self.p - 1
        };
        let mut x_prime = self.iters[prime_i].key().expect("Not at root").clone();
        loop {
            let x = self.iters[self.p].key().expect("Not at root");
            if x == &x_prime {
                self.key = Some(x_prime);
                break self.key.as_ref();
            }
            self.iters[self.p].seek(&x_prime).expect("Happy");
            if self.iters[self.p].at_end() {
                break None;
            } else {
                x_prime = self.iters[self.p].key().expect("Not at root").clone();
                self.p = if self.p == self.k() - 1 {
                    0
                } else {
                    self.p + 1
                };
            }
        }
    }

    fn seek(&mut self, seek_key: &KT) -> Option<&KT> {
        self.iters[self.p].seek(seek_key)?;
        if !self.iters[self.p].at_end() {
            self.p = if self.p == self.k() - 1 {
                0
            } else {
                self.p + 1
            };
            self.search()
        } else {
            None
        }
    }

    fn at_end(&self) -> bool {
        for iter in &self.iters {
            if iter.at_end() {
                return true;
            }
        }
        false
    }

    fn open(&mut self) -> Option<&KT> {
        for iter in &mut self.iters {
            iter.open()?;
        }
        self.init()
    }

    fn up(&mut self) -> Option<&KT> {
        for iter in &mut self.iters {
            iter.up()?;
        }
        self.init()
    }
}

pub fn leapfrog_triejoin<KT: PartialOrd + PartialEq + Clone>(
    trie_iterables: Vec<&impl TrieIterable<KT>>,
) {
    let mut iters = trie_iterables
        .into_iter()
        .map(|t| t.trie_iter())
        .collect::<Vec<_>>();
    let mut iter = LeapfrogTriejoinIter::new(iters);
}
