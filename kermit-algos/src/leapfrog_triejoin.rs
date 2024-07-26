use std::marker::PhantomData;

use kermit_iters::trie::TrieIterator;

pub trait LeapfrogTriejoinIterator<KT: PartialOrd + PartialEq + Clone> {
    fn init(&mut self);
    fn next(&mut self) -> Result<(), &'static str>;
    fn search(&mut self);
    fn seek(&mut self, seek_key: &KT);
    fn open(&mut self);
    fn up(&mut self) -> bool;
}

pub struct LeapfrogTriejoinIter<KT: PartialOrd + PartialEq + Clone, IT: TrieIterator<KT>> {
    p: usize,
    iters: Vec<IT>,
    phantom: PhantomData<KT>,
}

impl<KT: PartialOrd + PartialEq + Clone, IT: TrieIterator<KT>> LeapfrogTriejoinIter<KT, IT> {
    pub fn new(iters: Vec<IT>) -> Self {
        let mut iter = LeapfrogTriejoinIter {
            p: 0,
            iters,
            phantom: PhantomData,
        };
        iter.init();
        iter
    }

    fn at_end(&self) -> bool {
        for iter in &self.iters {
            if iter.at_end() {
                return true;
            }
        }
        false
    }

    fn k(&self) -> usize {
        self.iters.len()
    }

    fn iters(&self) -> &Vec<IT> {
        &self.iters
    }

    fn iters_mut(&mut self) -> &mut Vec<IT> {
        &mut self.iters
    }
}

impl<KT: PartialOrd + PartialEq + Clone, IT: TrieIterator<KT>> LeapfrogTriejoinIterator<KT>
    for LeapfrogTriejoinIter<KT, IT>
{
    fn init(&mut self) {
        if !self.at_end() {
            self.p = 0;
            self.search();
        }
    }

    fn search(&mut self) {
        while true {
            let x_prime = self.iters[(self.p - 1) % self.k()]
                .key()
                .expect("Not at root").clone();
            let x = self.iters[self.p].key().expect("Not at root");
            if x == &x_prime {
                return;
            }
            self.iters[self.p].seek(&x_prime).expect("Happy");
            if self.iters[self.p].at_end() {
                return;
            }
        }
    }

    fn next(&mut self) -> Result<(), &'static str> {
        self.iters[self.p].next().expect("Happy");
        if !self.iters[self.p].at_end() {
            self.p = self.p + 1 % self.k();
            self.search();
        }
        Ok(())
    }

    fn seek(&mut self, seek_key: &KT) {
        self.iters[self.p].seek(seek_key).expect("Happy");
        if !self.iters[self.p].at_end() {
            self.p = self.p + 1 % self.k();
            self.search();
        }
    }

    fn open(&mut self) {
        todo!()
    }

    fn up(&mut self) -> bool {
        todo!()
    }
}
