use crate::{node::{Internal, Node, TrieFields}, variable_type::VariableType};
use csv::{self, Error};
use std::{fmt::Debug, fs::File, io::Read, mem::Discriminant, path::Path, str::FromStr};

/// Trie root
pub struct Trie<KT: PartialOrd + PartialEq> {
    arity: usize,
    children: Vec<Node<KT>>,
}

impl<KT: PartialOrd + PartialEq> Trie<KT> {
    /// Construct an empty Trie
    pub fn new(arity: usize) -> Trie<KT> {
        Trie {
            arity,
            children: vec![],
        }
    }

    pub fn from_tuples(arity: usize, tuples: Vec<Vec<KT>>) -> Trie<KT> {
        let mut trie = Trie::new(arity);
        for tuple in tuples {
            trie.insert(tuple).unwrap();
        }
        trie
    }

    pub fn from_file<KT2: PartialOrd + PartialEq + FromStr + Debug, P: AsRef<Path>>(arity: usize, filepath: P) -> Result<Trie<KT2>, Error> {
        let file = File::open(filepath)?;
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(b',')
            .double_quote(false)
            .escape(Some(b'\\'))
            .flexible(false)
            .comment(Some(b'#'))
            .from_reader(file);
        let mut tuples = vec![];
        for result in rdr.records() {
            let record = result?;
            let mut tuple: Vec<KT2> = vec![];
            for x in record.iter() {
                if let Ok(y) =  x.to_string().parse::<KT2>() {
                    tuple.push(y);
                }
            }
            tuples.push(tuple);
        }
        let trie = Trie::<KT2>::from_tuples(arity, tuples);
        Ok(trie)
    }

    pub fn insert(&mut self, mut tuple: Vec<KT>) -> Result<(), &'static str> {
        if tuple.len() != self.arity {
            return Err("Arity doesn't match.");
        }
        tuple.reverse();
        self.insert_deque(tuple);
        Ok(())
    }

    pub fn search(&self, tuple: Vec<KT>) -> Result<Option<&Node<KT>>, &'static str> {
        if tuple.len() != self.arity {
            return Err("Arity doesn't match.");
        }
        Ok(self.search_deque(tuple.into()))
    }

    pub fn remove(&mut self, tuple: Vec<KT>) -> Result<(), &'static str> {
        if tuple.len() != self.arity {
            return Err("Arity doesn't match.");
        }
        self.remove_deque(tuple.into());
        Ok(())
    }
}

impl<KT: PartialOrd + PartialEq> TrieFields<KT> for Trie<KT> {
    fn children(&self) -> &Vec<Node<KT>> {
        &self.children
    }
    fn arity(&self) -> usize {
        self.arity
    }
}

impl<KT: PartialOrd + PartialEq> Internal<KT> for Trie<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>> {
        &mut self.children
    }
}
