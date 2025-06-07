use kermit_iters::{key_type::KeyType, linear::LinearIterator};

pub trait LeapfrogJoinIterator<'a> {
    type KT: KeyType;

    fn key(&self) -> Option<&'a Self::KT>;

    fn leapfrog_init(&mut self) -> bool;

    fn leapfrog_search(&mut self) -> bool;

    fn leapfrog_seek(&mut self, seek_key: &Self::KT) -> bool;

    fn leapfrog_next(&mut self) -> Option<&'a Self::KT>;

    fn at_end(&self) -> bool;
}

pub struct LeapfrogJoinIter<'a, IT>
where
    IT: LinearIterator<'a>,
{
    key: Option<&'a IT::KT>,
    pub(crate) iterators: Vec<IT>,
    p: usize,
}

impl<'a, IT> LeapfrogJoinIter<'a, IT>
where
    IT: LinearIterator<'a>,
{
    pub fn new(iterators: Vec<IT>) -> Self {
        LeapfrogJoinIter {
            key: None,
            iterators,
            p: 0,
        }
    }

    pub fn k(&self) -> usize { self.iterators.len() }
}

impl<'a, IT> LeapfrogJoinIterator<'a> for LeapfrogJoinIter<'a, IT>
where
    IT: LinearIterator<'a> + 'a,
{
    type KT = IT::KT;

    fn key(&self) -> Option<&'a Self::KT> { self.iterators[self.p].key() }

    fn leapfrog_init(&mut self) -> bool {
        for iter in &mut self.iterators {
            iter.next();
        }

        if self.at_end() {
            return false;
        }

        self.iterators.sort_unstable_by(|a, b| {
            let a_key = a.key().unwrap();
            let b_key = b.key().unwrap();
            match a_key.cmp(b_key) {
                | std::cmp::Ordering::Less => std::cmp::Ordering::Less,
                | std::cmp::Ordering::Greater => std::cmp::Ordering::Greater,
                | std::cmp::Ordering::Equal => std::cmp::Ordering::Equal,
            }
        });

        self.p = 0;
        self.leapfrog_search()
    }

    fn leapfrog_search(&mut self) -> bool {
        let prime_i = if self.p == 0 {
            self.k() - 1
        } else {
            self.p - 1
        };
        let mut x_prime = self.iterators[prime_i].key().unwrap();
        loop {
            let x = self.iterators[self.p].key().unwrap();
            if x == x_prime {
                self.key = Some(x);
                return true;
            } else {
                self.iterators[self.p].seek(x_prime);
                if self.iterators[self.p].at_end() {
                    return false;
                }
                x_prime = self.iterators[self.p].key().unwrap();
                self.p = (self.p + 1) % self.k();
            }
        }
    }

    fn leapfrog_next(&mut self) -> Option<&'a Self::KT> {
        self.iterators[self.p].next();
        if self.iterators[self.p].at_end() {
            None
        } else {
            self.p = (self.p + 1) % self.k();
            self.leapfrog_search();
            self.key()
        }
    }

    fn at_end(&self) -> bool {
        for iter in &self.iterators {
            if iter.at_end() {
                return true;
            }
        }
        false
    }

    fn leapfrog_seek(&mut self, seek_key: &Self::KT) -> bool {
        self.iterators[self.p].seek(seek_key);
        if self.iterators[self.p].at_end() {
            false
        } else {
            self.p = (self.p + 1) % self.k();
            self.leapfrog_search()
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::linear::LinearIterable};

    #[test]
    fn test_leapfrog_join_iter() {
        let v1 = vec![1, 2, 3];
        let v2 = vec![2, 3, 4];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(&2));
        assert_eq!(join_iter.leapfrog_next(), Some(&3));
        assert_eq!(join_iter.leapfrog_next(), None);
    }

    #[test]
    fn test_leapfrog_join_iter_empty() {
        let v1: Vec<i32> = vec![];
        let v2: Vec<i32> = vec![];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }

    #[test]
    fn test_leapfrog_join_iter_no_common_elements() {
        let v1 = vec![1, 3, 5];
        let v2 = vec![2, 4, 6];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }

    #[test]
    fn test_leapfrog_join_iter_single_iterator() {
        let v1 = vec![1, 2, 3];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(&1));
    }

    #[test]
    fn test_leapfrog_join_iter_multiple_vectors_with_common_elements() {
        let v1 = vec![1, 2, 3, 5];
        let v2 = vec![2, 4, 5, 6];
        let v3 = vec![2, 5, 7];

        let mut join_iter =
            LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(&2));
        assert_eq!(join_iter.leapfrog_next(), Some(&5));
        assert_eq!(join_iter.leapfrog_next(), None);
    }

    #[test]
    fn test_leapfrog_join_iter_no_common_elements_multiple_vectors() {
        let v1 = vec![1, 3, 5];
        let v2 = vec![2, 4, 6];
        let v3 = vec![7, 8, 9];

        let mut join_iter =
            LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }

    #[test]
    fn test_leapfrog_join_iter_empty_and_non_empty() {
        let v1: Vec<i32> = vec![];
        let v2 = vec![1, 2, 3];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }

    #[test]
    fn test_leapfrog_join_iter_multiple_empty_vectors() {
        let v1: Vec<i32> = vec![];
        let v2: Vec<i32> = vec![];
        let v3: Vec<i32> = vec![];

        let mut join_iter =
            LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }

    #[test]
    fn test_leapfrog_join_iter_single_common_element() {
        let v1 = vec![1, 2, 3];
        let v2 = vec![2];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(&2));
    }

    #[test]
    fn test_leapfrog_join_iter_multiple_vectors_with_duplicates() {
        let v1 = vec![1, 2, 3];
        let v2 = vec![2, 4];
        let v3 = vec![2, 5, 7];

        let mut join_iter =
            LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(&2));
    }

    #[test]
    fn test_leapfrog_join_iter_large_vectors() {
        let v1: Vec<i32> = (1..1000).collect();
        let v2: Vec<i32> = (500..1500).collect();

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(&500));
        assert_eq!(join_iter.leapfrog_next(), Some(&501));
    }

    #[test]
    fn test_leapfrog_join_iter_multiple_vectors_with_one_empty() {
        let v1 = vec![1, 2, 3];
        let v2: Vec<i32> = vec![];
        let v3 = vec![2, 4, 6];

        let mut join_iter =
            LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }
}
