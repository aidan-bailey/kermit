use crate::key_type::KeyType;

pub trait JoinIterable {
    type KT: KeyType;
}
