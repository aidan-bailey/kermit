use kermit_iters::key_type::KeyType;

pub trait Node {
    type KT: KeyType;
    fn new(key: Self::KT) -> Self;
    fn key(&self) -> &Self::KT;
}
