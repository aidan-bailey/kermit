pub mod iterator;
pub mod node;
pub mod tuple_trie;
pub mod variable_type;

#[cfg(test)]
mod tests {
    use crate::{
        iterator::{TrieIter, TrieIterator},
        node::TrieFields,
        tuple_trie::Trie,
        variable_type::VariableType,
    };

    #[test]
    fn trie_new() {
        let empty_tri = Trie::<u64>::new(1);
        assert_eq!(empty_tri.arity(), 1);
        assert!(empty_tri.is_empty());
    }

    #[test]
    fn trie_insert() {
        let mut trie = Trie::<u64>::new(2);

        let _ = trie.insert(vec![1, 2]);

        assert_eq!(trie.size(), 1);
        // check first level child
        let child = &trie.children()[0];
        assert_eq!(child.key(), &1);
        assert_eq!(child.size(), 1);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), &2);
        assert_eq!(child.size(), 0);

        let _ = trie.insert(vec![0, 2]);

        assert_eq!(trie.size(), 2);
        // check first level child
        let child = &trie.children()[0];
        assert_eq!(child.key(), &0);
        assert_eq!(child.size(), 1);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), &2);
        assert_eq!(child.size(), 0);

        let _ = trie.insert(vec![1, 1]);

        assert_eq!(trie.size(), 2);
        // check first level child
        let child = &trie.children()[1];
        assert_eq!(child.key(), &1);
        assert_eq!(child.size(), 2);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), &1);
        assert_eq!(child.size(), 0);
    }

    #[test]
    fn trie_search() {
        let mut trie = Trie::<u64>::new(2);

        let _ = trie.insert(vec![1, 2]);
        let _ = trie.insert(vec![0, 2]);
        let _ = trie.insert(vec![1, 1]);

        assert!(trie.search(vec![1, 2]).unwrap().is_some());
        assert!(trie.search(vec![0, 2]).unwrap().is_some());
        assert!(trie.search(vec![1, 1]).unwrap().is_some());
        assert!(trie.search(vec![0, 1]).unwrap().is_none());
    }

    #[test]
    fn trie_remove() {
        let mut trie = Trie::<u64>::new(2);

        let _ = trie.insert(vec![1, 2]);
        let _ = trie.insert(vec![0, 2]);
        let _ = trie.insert(vec![1, 1]);

        assert!(trie.search(vec![1, 2]).unwrap().is_some());
        let _ = trie.remove(vec![1, 2]);
        assert!(trie.search(vec![1, 2]).unwrap().is_none());
    }

    #[test]
    fn trie_iterator() {
        let mut trie = Trie::<u64>::new(3);

        assert!(trie.insert(vec![1, 3, 4]).is_ok());
        assert!(trie.insert(vec![1, 3, 5]).is_ok());
        assert!(trie.insert(vec![1, 4, 6]).is_ok());
        assert!(trie.insert(vec![1, 4, 8]).is_ok());
        assert!(trie.insert(vec![1, 4, 9]).is_ok());
        assert!(trie.insert(vec![1, 5, 2]).is_ok());
        assert!(trie.insert(vec![3, 5, 2]).is_ok());

        let mut iter = TrieIter::new(&trie);

        assert!(iter.open().is_ok());
        assert_eq!(iter.key().unwrap(), &1);
        assert!(iter.open().is_ok());
        assert_eq!(iter.key().unwrap(), &3);
        assert!(iter.open().is_ok());
        assert_eq!(iter.key().unwrap(), &4);

        assert!(iter.next().is_ok());
        assert_eq!(iter.key().unwrap(), &5);

        assert!(iter.up().is_ok());
        assert_eq!(iter.key().unwrap(), &3);

        assert!(iter.next().is_ok());
        assert_eq!(iter.key().unwrap(), &4);

        assert!(iter.open().is_ok());
        assert_eq!(iter.key().unwrap(), &6);

        assert!(iter.seek(&9).is_ok());

        assert!(iter.up().is_ok());
        assert_eq!(iter.key().unwrap(), &4);

        assert!(iter.up().is_ok());
        assert_eq!(iter.key().unwrap(), &1);

        assert!(iter.next().is_ok());
        assert_eq!(iter.key().unwrap(), &3);

        assert!(iter.open().is_ok());
        assert_eq!(iter.key().unwrap(), &5);
        assert!(iter.open().is_ok());
        assert_eq!(iter.key().unwrap(), &2);
        assert!(iter.open().is_err());
    }

    // Variable types
    #[test]
    fn trie_with_variable_type() {
        let mut trie = Trie::<VariableType>::new(2);

        let _ = trie.insert(vec![VariableType::Int(1), VariableType::String("2".to_string())]);
        let _ = trie.insert(vec![VariableType::Int(0), VariableType::Float(2.)]);
        let _ = trie.insert(vec![VariableType::Int(1), VariableType::Int(1)]);

        assert!(trie.search(vec![VariableType::Int(1), VariableType::String("2".to_string())]).unwrap().is_some());
        assert!(trie.search(vec![VariableType::Int(0), VariableType::Float(2.)]).unwrap().is_some());
        assert!(trie.search(vec![VariableType::Int(1), VariableType::Int(1)]).unwrap().is_some());
    }

    // Read from file
    #[test]
    fn trie_read_from_file() {
        let trie = Trie::<String>::from_file::<String, &str>(3, "Test.csv").unwrap();
        assert_eq!(trie.children()[0].key(), "1");
        assert_eq!(trie.children()[1].key(), "3");
        assert_eq!(trie.children()[0].children()[0].key(), "3");
        assert_eq!(trie.children()[0].children()[1].key(), "4");
        assert_eq!(trie.children()[0].children()[2].key(), "5");
        assert_eq!(trie.children()[1].children()[0].key(), "5");
        assert_eq!(trie.children()[0].children()[0].children()[0].key(), "4");
        assert_eq!(trie.children()[0].children()[0].children()[1].key(), "5");
        assert_eq!(trie.children()[0].children()[1].children()[0].key(), "6");
        assert_eq!(trie.children()[0].children()[1].children()[1].key(), "8");
        assert_eq!(trie.children()[0].children()[1].children()[2].key(), "9");
        assert_eq!(trie.children()[0].children()[2].children()[0].key(), "2");
        assert_eq!(trie.children()[1].children()[0].children()[0].key(), "2");
    }
}
