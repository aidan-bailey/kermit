/// Models for synthetic graph generation.
pub enum GraphModel {
    /// Erdos-Renyi random graph: each of the n*(n-1)/2 possible edges exists
    /// independently with probability p.
    ErdosRenyi { n: usize, p: f64 },
}
