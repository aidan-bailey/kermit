//! H2O's key columns: the cardinalities of a plan ([`KeyPlan`]) and the four
//! steps that turn one cardinality into a key column of `x` and a right-hand
//! table ([`KeyPool`]).
//!
//! For a column of cardinality n, with n/10 taken as integer division:
//!
//! 1. The pool is the keys `1..=n + n/10`, shuffled. Its first `n - n/10` keys
//!    are shared by `x` and the right-hand table, the next `n/10` are left-only
//!    (`x`'s only) and the last `n/10` are right-only. The integer split
//!    replaces upstream's `n * 0.9`, so every n is well defined.
//! 2. `x`'s column is the `n` shared and left keys, padded to the row count
//!    with uniform draws from them (upstream's `sample_all`), then shuffled.
//!    Every shared and left key appears at least once.
//! 3. The right-hand table is the shared keys and the right-only keys,
//!    shuffled: exactly n distinct keys.
//! 4. The match count is the number of `x`'s rows whose key is shared.

use {
    super::rng::{stream, KeyColumn, Step},
    anyhow::ensure,
    kermit_ds::RowId,
};

/// One H2O plan's cardinalities: `rows` = N, the rows of `x` and the keys of
/// `big`; `small` and `medium`, the keys of the other two right-hand tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "KeyPlanFields")]
pub struct KeyPlan {
    /// N, the rows of `x` and the keys of `big`.
    rows: usize,
    /// The keys of `small`.
    small: usize,
    /// The keys of `medium`.
    medium: usize,
}

impl KeyPlan {
    /// A plan of `rows` fact-table rows, `small` keys in `small` and `medium`
    /// keys in `medium`.
    ///
    /// # Errors
    ///
    /// Unless `1 <= small <= rows`, `1 <= medium <= rows` and
    /// `rows <= RowId::MAX`.
    pub fn new(rows: usize, small: usize, medium: usize) -> anyhow::Result<Self> {
        ensure!(
            rows <= RowId::MAX as usize,
            "H2O plan: rows = {rows} exceeds RowId::MAX = {}",
            RowId::MAX
        );
        ensure!(
            (1..=rows).contains(&small),
            "H2O plan: small = {small} is outside 1..={rows}"
        );
        ensure!(
            (1..=rows).contains(&medium),
            "H2O plan: medium = {medium} is outside 1..={rows}"
        );
        Ok(Self {
            rows,
            small,
            medium,
        })
    }

    /// The plan of scale `scale`: `(N, N / 1_000_000, N / 1_000)`, through
    /// [`KeyPlan::new`], so a scale below 1,000,000 is an error (`small`
    /// would be 0).
    ///
    /// # Errors
    ///
    /// As [`KeyPlan::new`].
    pub fn for_scale(scale: u32) -> anyhow::Result<Self> {
        let rows = scale as usize;
        Self::new(rows, rows / 1_000_000, rows / 1_000)
    }

    /// N, the rows of `x` and the keys of `big`.
    pub fn rows(&self) -> usize { self.rows }

    /// The keys of `small`.
    pub fn small(&self) -> usize { self.small }

    /// The keys of `medium`.
    pub fn medium(&self) -> usize { self.medium }

    /// The cardinality of `column`'s keys: `small`, `medium` or `rows`.
    pub(crate) fn cardinality(&self, column: KeyColumn) -> usize {
        match column {
            | KeyColumn::Id1 => self.small,
            | KeyColumn::Id2 => self.medium,
            | KeyColumn::Id3 => self.rows,
        }
    }
}

/// `KeyPlan`'s serialized form, checked by [`KeyPlan::new`] when it is read
/// back, so a deserialized plan holds the same invariants as a new one.
#[derive(serde::Deserialize)]
struct KeyPlanFields {
    rows: usize,
    small: usize,
    medium: usize,
}

impl TryFrom<KeyPlanFields> for KeyPlan {
    type Error = anyhow::Error;

    fn try_from(fields: KeyPlanFields) -> anyhow::Result<Self> {
        Self::new(fields.rows, fields.small, fields.medium)
    }
}

/// Step 1 of key generation for one column of cardinality `n`: the keys
/// `1..=n + n/10`, shuffled (stream `Pool`). The first `n - n/10` are shared,
/// the next `n/10` are left-only (`x`'s only), and the last `n/10` are
/// right-only.
pub(crate) struct KeyPool {
    /// The shuffled keys: shared, then left-only, then right-only.
    keys: Vec<usize>,
    /// The column's cardinality.
    n: usize,
    /// The seed every stream of this column is drawn from.
    seed: u32,
    /// The column, which numbers its streams.
    column: KeyColumn,
}

impl KeyPool {
    /// The pool of a column of cardinality `n` under `seed`.
    pub(crate) fn new(n: usize, seed: u32, column: KeyColumn) -> Self {
        let mut keys: Vec<usize> = (1..=n + n / 10).collect();
        stream(seed, column, Step::Pool).shuffle(&mut keys);
        Self {
            keys,
            n,
            seed,
            column,
        }
    }

    /// The keys that `x` and the right-hand table share: `keys[..n - n/10]`.
    pub(crate) fn shared(&self) -> &[usize] { &self.keys[..self.n - self.n / 10] }

    /// The keys only `x` has: `keys[n - n/10..n]`. Only the tests need
    /// them on their own; `x_column` takes them with the shared keys.
    #[cfg(test)]
    pub(crate) fn left(&self) -> &[usize] { &self.keys[self.n - self.n / 10..self.n] }

    /// The keys only the right-hand table has: `keys[n..]`.
    pub(crate) fn right(&self) -> &[usize] { &self.keys[self.n..] }

    /// Step 2, `x`'s column (upstream's `sample_all`): `keys[..n]`, then
    /// `rows - n` draws `keys[bounded(n)]` (stream `Pad`), then shuffled
    /// (stream `LeftShuffle`). Every shared and left key appears.
    ///
    /// # Panics
    ///
    /// Panics if `rows < n`.
    pub(crate) fn x_column(&self, rows: usize) -> Vec<usize> {
        assert!(
            rows >= self.n,
            "KeyPool::x_column: rows = {rows} is below n = {}",
            self.n
        );
        let mut x = Vec::with_capacity(rows);
        x.extend_from_slice(&self.keys[..self.n]);
        let mut pad = stream(self.seed, self.column, Step::Pad);
        for _ in self.n..rows {
            x.push(self.keys[pad.bounded(self.n as u64) as usize]);
        }
        stream(self.seed, self.column, Step::LeftShuffle).shuffle(&mut x);
        x
    }

    /// Step 3, the right-hand table: the shared keys and the right-only keys,
    /// shuffled (stream `RightShuffle`): exactly `n` distinct keys.
    pub(crate) fn right_table(&self) -> Vec<usize> {
        let mut table = Vec::with_capacity(self.n);
        table.extend_from_slice(self.shared());
        table.extend_from_slice(self.right());
        stream(self.seed, self.column, Step::RightShuffle).shuffle(&mut table);
        table
    }

    /// Step 4: how many of `x`'s keys are shared, by a bitmap over
    /// `0..=n + n/10`. `x` must hold keys of this pool.
    pub(crate) fn matches(&self, x: &[usize]) -> u64 {
        let mut is_shared = vec![false; self.keys.len() + 1];
        for &key in self.shared() {
            is_shared[key] = true;
        }
        x.iter().filter(|&&key| is_shared[key]).count() as u64
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::collections::HashSet};

    fn sorted(mut keys: Vec<usize>) -> Vec<usize> {
        keys.sort_unstable();
        keys
    }

    #[test]
    fn pool_splits_have_the_right_sizes() {
        for n in [7, 20, 200, 2000] {
            let pool = KeyPool::new(n, 108, KeyColumn::Id3);
            let tenth = n / 10;
            assert_eq!(pool.shared().len(), n - tenth, "shared, n = {n}");
            assert_eq!(pool.left().len(), tenth, "left, n = {n}");
            assert_eq!(pool.right().len(), tenth, "right, n = {n}");
            let together = [pool.shared(), pool.left(), pool.right()].concat();
            assert_eq!(
                sorted(together.clone()),
                (1..=n + tenth).collect::<Vec<_>>(),
                "n = {n}: the splits are not 1..=n + n/10, each once"
            );
            assert_ne!(
                together,
                sorted(together.clone()),
                "n = {n}: the pool was not shuffled"
            );
        }
        // n / 10 = 0: nothing is left-only or right-only.
        let pool = KeyPool::new(7, 108, KeyColumn::Id3);
        assert!(pool.left().is_empty() && pool.right().is_empty());
    }

    #[test]
    fn the_right_table_is_shared_and_right_without_duplicates() {
        for n in [7, 20, 200] {
            let pool = KeyPool::new(n, 108, KeyColumn::Id1);
            let table = pool.right_table();
            assert_eq!(table.len(), n, "n = {n}");
            let distinct: HashSet<usize> = table.iter().copied().collect();
            assert_eq!(distinct.len(), n, "n = {n}: the table repeats a key");
            let expected: HashSet<usize> =
                pool.shared().iter().chain(pool.right()).copied().collect();
            assert_eq!(distinct, expected, "n = {n}");
        }
        let pool = KeyPool::new(200, 108, KeyColumn::Id1);
        assert_ne!(
            pool.right_table(),
            [pool.shared(), pool.right()].concat(),
            "the right table was not shuffled"
        );
    }

    #[test]
    fn x_column_covers_shared_and_left_and_never_right() {
        for (n, rows) in [(20, 2000), (200, 2000), (7, 50)] {
            let pool = KeyPool::new(n, 108, KeyColumn::Id2);
            let x = pool.x_column(rows);
            assert_eq!(x.len(), rows, "n = {n}");
            let present: HashSet<usize> = x.iter().copied().collect();
            for key in pool.shared().iter().chain(pool.left()) {
                assert!(present.contains(key), "n = {n}: key {key} is missing");
            }
            for key in pool.right() {
                assert!(!present.contains(key), "n = {n}: right-only key {key} in x");
            }
        }
    }

    #[test]
    #[should_panic(expected = "below n")]
    fn x_column_needs_at_least_n_rows() { KeyPool::new(20, 108, KeyColumn::Id2).x_column(19); }

    #[test]
    fn the_x_column_at_n_equal_rows_is_a_permutation() {
        let pool = KeyPool::new(2000, 108, KeyColumn::Id3);
        let x = pool.x_column(2000);
        let covered = [pool.shared(), pool.left()].concat();
        assert_ne!(x, covered, "the column was not shuffled");
        assert_eq!(sorted(x), sorted(covered));
    }

    #[test]
    fn matches_count_the_rows_with_a_shared_key() {
        for n in [20, 200] {
            let pool = KeyPool::new(n, 108, KeyColumn::Id1);
            let x = pool.x_column(2000);
            let shared: HashSet<usize> = pool.shared().iter().copied().collect();
            let expected = x.iter().filter(|&&key| shared.contains(&key)).count();
            assert_eq!(pool.matches(&x), expected as u64, "n = {n}");
            assert!(expected > 0, "n = {n}: no row has a shared key");
            assert!(expected < x.len(), "n = {n}: every row has a shared key");
        }
        // n / 10 = 0: every key of x is shared.
        let pool = KeyPool::new(7, 108, KeyColumn::Id1);
        assert_eq!(pool.matches(&pool.x_column(50)), 50);
        assert_eq!(pool.matches(&[]), 0);
    }

    #[test]
    fn key_plan_rejects_bad_cardinalities() {
        let past_row_ids = RowId::MAX as usize + 1;
        let cases = [
            (100, 0, 10, "small = 0 is outside 1..=100"),
            (100, 10, 0, "medium = 0 is outside 1..=100"),
            (100, 101, 10, "small = 101 is outside 1..=100"),
            (100, 10, 101, "medium = 101 is outside 1..=100"),
            (
                past_row_ids,
                1,
                1,
                "rows = 4294967296 exceeds RowId::MAX = 4294967295",
            ),
        ];
        for (rows, small, medium, message) in cases {
            let Err(err) = KeyPlan::new(rows, small, medium) else {
                panic!("({rows}, {small}, {medium}) was accepted");
            };
            assert!(
                err.to_string().contains(message),
                "({rows}, {small}, {medium}): {err}"
            );
            assert!(err.to_string().starts_with("H2O plan: "), "{err}");
        }
        // The bounds themselves are fine.
        assert!(KeyPlan::new(100, 100, 100).is_ok());
        assert!(KeyPlan::new(100, 1, 1).is_ok());
        assert!(KeyPlan::new(RowId::MAX as usize, 1, 1).is_ok());
    }

    #[test]
    fn key_plan_for_scale_divides_by_a_million_and_a_thousand() {
        let plan = KeyPlan::for_scale(10_000_000).unwrap();
        assert_eq!(
            (plan.rows(), plan.small(), plan.medium()),
            (10_000_000, 10, 10_000)
        );
        let plan = KeyPlan::for_scale(1_000_000).unwrap();
        assert_eq!(
            (plan.rows(), plan.small(), plan.medium()),
            (1_000_000, 1, 1_000)
        );
        let Err(err) = KeyPlan::for_scale(999_999) else {
            panic!("a scale below 1,000,000 was accepted");
        };
        assert!(err.to_string().contains("small = 0"), "{err}");
    }

    #[test]
    fn key_plan_serializes_its_three_cardinalities() {
        let plan = KeyPlan::new(2000, 20, 200).unwrap();
        let json = serde_json::to_string(&plan).unwrap();
        assert_eq!(json, r#"{"rows":2000,"small":20,"medium":200}"#);
        assert_eq!(serde_json::from_str::<KeyPlan>(&json).unwrap(), plan);
    }

    #[test]
    fn a_deserialized_plan_is_checked_like_a_new_one() {
        let plan: KeyPlan =
            serde_json::from_str(r#"{"rows":2000,"small":20,"medium":200}"#).unwrap();
        assert_eq!(plan, KeyPlan::new(2000, 20, 200).unwrap());
        let Err(err) = serde_json::from_str::<KeyPlan>(r#"{"rows":2000,"small":0,"medium":200}"#)
        else {
            panic!("a plan with small = 0 was deserialized");
        };
        assert!(err.to_string().contains("small = 0"), "{err}");
    }

    #[test]
    fn the_cardinality_of_a_column_is_small_medium_or_rows() {
        let plan = KeyPlan::new(2000, 20, 200).unwrap();
        assert_eq!(plan.cardinality(KeyColumn::Id1), 20);
        assert_eq!(plan.cardinality(KeyColumn::Id2), 200);
        assert_eq!(plan.cardinality(KeyColumn::Id3), 2000);
    }

    #[test]
    fn the_pool_and_its_columns_match_the_reference_model() {
        let pool = KeyPool::new(20, 108, KeyColumn::Id3);
        assert_eq!(pool.shared(), [
            19, 11, 8, 5, 10, 13, 16, 15, 9, 12, 3, 22, 18, 14, 1, 7, 20, 4
        ]);
        assert_eq!(pool.left(), [21, 17]);
        assert_eq!(pool.right(), [2, 6]);
        let x = pool.x_column(30);
        assert_eq!(x, [
            22, 19, 10, 20, 10, 12, 10, 3, 8, 4, 11, 14, 9, 16, 1, 22, 19, 14, 17, 18, 19, 3, 5,
            11, 7, 21, 9, 13, 19, 15
        ]);
        assert_eq!(pool.right_table(), [
            15, 18, 12, 5, 7, 11, 20, 14, 8, 16, 2, 6, 4, 13, 19, 9, 22, 10, 1, 3
        ]);
        // Two rows (the left-only keys 21 and 17, once each) are not shared.
        assert_eq!(pool.matches(&x), 28);
    }
}
