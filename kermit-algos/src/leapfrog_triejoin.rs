use std::marker::PhantomData;

use kermit_iters::trie::TrieIterator;

pub trait LeapfrogTriejoinIterator<KT: PartialOrd + PartialEq + Clone> {
    fn init(&mut self) -> Result<(), &'static str>;
    fn next(&mut self) -> Result<(), &'static str>;
    fn search(&mut self) -> Result<(), &'static str>;
    fn seek(&mut self, seek_key: &KT);
    fn at_end(&self) -> bool;
    fn open(&mut self) -> Result<(), &'static str>;
    fn up(&mut self) -> Result<(), &'static str>;
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
    fn init(&mut self) -> Result<(), &'static str> {
        if !self.at_end() {
            self.iters.sort_unstable_by(|a, b| {
                let a_key = a.key().expect("Not at root");
                let b_key = b.key().expect("Not at root");
                if a_key < b_key {
                    return std::cmp::Ordering::Less;
                } else if a_key > b_key {
                    return std::cmp::Ordering::Greater;
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            self.p = 0;
            self.search()
        } else {
            Ok(())
        }
    }

    fn next(&mut self) -> Result<(), &'static str> {
        self.iters[self.p].next()?;
        if !self.iters[self.p].at_end() {
            self.p = if self.p == self.k() - 1 {
                0
            } else {
                self.p + 1
            };
            self.search()
        } else {
            Ok(())
        }
    }

    fn search(&mut self) -> Result<(), &'static str> {
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
                break Ok(());
            }
            self.iters[self.p].seek(&x_prime).expect("Happy");
            if self.iters[self.p].at_end() {
                break Ok(());
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

    fn seek(&mut self, seek_key: &KT) -> Result<(), &'static str> {
        self.iters[self.p].seek(seek_key)?;
        if !self.iters[self.p].at_end() {
            self.p = if self.p == self.k() - 1 {
                0
            } else {
                self.p + 1
            };
            self.search()
        } else {
            Ok(())
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

    fn open(&mut self) -> Result<(), &'static str> {
        for iter in &mut self.iters {
            iter.open()?;
        }
        self.init()
    }

    fn up(&mut self) -> Result<(), &'static str> {
        for iter in &mut self.iters {
            iter.up()?;
        }
        self.init()
    }
}
