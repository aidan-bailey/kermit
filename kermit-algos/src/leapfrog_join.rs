use kermit_iters::linear::LinearIterator;

pub trait LeapfrogJoinIterator<KT: PartialOrd + PartialEq + Clone> {
    fn init(&mut self) -> Option<&KT>;
    fn next(&mut self) -> Option<&KT>;
    fn search(&mut self) -> Option<&KT>;
    fn seek(&mut self, seek_key: &KT) -> Option<&KT>;
    fn at_end(&self) -> bool;
}

pub struct LeapfrogJoinIter<KT: PartialOrd + PartialEq + Clone, IT: LinearIterator<KT>> {
    pub key: Option<KT>,
    p: usize,
    iters: Vec<IT>,
}

impl<KT: PartialOrd + PartialEq + Clone, IT: LinearIterator<KT>> LeapfrogJoinIter<KT, IT> {
    pub fn new(iters: Vec<IT>) -> Self {
        let mut iter = LeapfrogJoinIter {
            key: None,
            p: 0,
            iters,
        };
        iter.init();
        iter
    }

    fn k(&self) -> usize {
        self.iters.len()
    }
}

impl<KT: PartialOrd + PartialEq + Clone, IT: LinearIterator<KT>> LeapfrogJoinIterator<KT>
    for LeapfrogJoinIter<KT, IT>
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
        let mut x_prime = self.iters[prime_i].key()?.clone();
        loop {
            let x = self.iters[self.p].key()?;
            if x == &x_prime {
                self.key = Some(x.clone());
                break self.key.as_ref();
            }
            x_prime = self.iters[self.p].seek(&x_prime)?.clone();
            self.p = (self.p + 1) % self.k();
        }
    }

    fn seek(&mut self, seek_key: &KT) -> Option<&KT> {
        self.iters[self.p].seek(seek_key)?;
        if !self.iters[self.p].at_end() {
            self.p = (self.p + 1) % self.k();
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
}
