# Sorted-Trie Seek Strategies Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the sorted tries' seek algorithm a Layout. `TreeTrie<S>` and
`ColumnTrie<S>` gain an `S: SeekStrategy` type parameter with three values,
`linear`, `binary` (the default) and `galloping`, each one implementation
shared by both tries. `--ds-layout-seek` selects the strategy, and reports
record it as the `ds_layout_seek` axis.

**Architecture:** There are four work packages; see "Work packages".

- **P1** (Tasks 1–3), all in `kermit-ds`: it generalises
  `#[derive(IntoTrieIter)]` to accept type parameters, adds
  `kermit-ds/src/seek.rs` with the three strategies, and makes both tries
  and their iterators generic. Every strategy computes only the offset that
  `slice::partition_point` would return, so each iterator's surrounding
  logic stays unchanged.
- **P2** (Tasks 4–5): in the `kermit` binary, the flag reaches a
  `SeekChoice` field on both sorted `Execution` cells, and
  `with_sorted_trie_layout!` turns it into a type. Then every strategy runs
  through the join, allocation and real-data suites.
- **P3** (Tasks 7–8): kermit-lab and the docs.
- **Controller** (Tasks 0, 6, 9 and 10): the gates, the two measurements
  and the checkpoints.

**Tech Stack:** Rust nightly workspace (proc-macro `syn`/`quote`, clap,
Criterion, `serde_json`), Python kermit-lab (pandas, pytest via uv), Nix dev
shell.

**Spec:** [`docs/specs/2026-10-03-sorted-trie-seek-strategy-design.md`](../../specs/2026-10-03-sorted-trie-seek-strategy-design.md)

---

## Preconditions (read before Task 0)

- **#84 phase 2 must be on `origin/master`** before Task 0 runs. Tasks 4,
  5 and 7 are written against the shapes #84's phase-2 plan introduces
  (`git show
  aidanb/84:docs/superpowers/plans/2026-10-03-column-trie-build-mode.md`,
  Tasks 5–9):
  - `DsChoices { hasher, pruning, config, build }` and
    `DsChoices::resolve(selector, &layout, &config, &build)`;
  - `SortedTrie::{TreeTrie, ColumnTrie { build }}`;
  - `SortedTrieRelation { type BuildMode; fn kind(build); fn
    build_with(header, build, tuples); fn build_mode_axes(build) }`;
  - `SortedTrieFamily::{new(build), default()}` and
    `TrieLftj::new(build, optimiser)`;
  - `load_query_runner(args, config, build)`;
  - `SCOPED_AXIS_DEFAULTS`, and the `_BUILD_ONLY_AXES` / `_BUILD_PHASES`
    guard in `presets.py`.

  Where the merged code differs from those snippets, adapt this plan's
  snippets minimally, keep their intent, and report the deviation.
- **The authoritative sweep comes first.** Iteration and space ran at
  62f722e; the insertion re-run is pending #84. #80 lands after the sweep
  (user, 2026-10-02). In practice, "after the sweep" means after that
  deferred insertion re-run. Making `TreeTrie<S>` generic changes how the
  build path monomorphises, so the insertion binary must not contain it.
  Task 10 lands nothing without the user's instruction.
- **Questions 1–3 in the spec are resolved.** At checkpoint 1
  (2026-10-03) the user chose the recommended answer to all three:
  stateless doubling galloping, `linear` kept, and the probe set for
  measurement 2. The plan stands as written.

## Ground rules for every task

- **Paths.** The worktree is
  `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/80_18dafb5a4bc72f12`.
  `SCRATCH` is the executing session's scratchpad directory. **Never `cd`**
  in a Bash call, because it moves the session; use absolute paths,
  `env -C <dir>`, `git -C $WT` or `uv --directory`.
- **Cargo.** Run it **in the foreground** through the flake with
  `CARGO_BUILD_JOBS=2`, e.g. `CARGO_BUILD_JOBS=2 nix develop $WT --command
  cargo test -p kermit-ds`. A background memory monitor kills
  `run_in_background` cargo jobs. Anything longer than 10 minutes runs
  fully detached (`setsid nohup … & disown`).
- **Formatting.** Format **only** with `nix develop $WT --command cargo fmt
  --all`; stable rustfmt rewrites dozens of files. If CI's fmt later
  disagrees, run `nix flake update rust-overlay` first (toolchain skew).
- **Doc comments.** clippy's `doc_markdown` lints them, so backtick every
  identifier.
- **Commits.** Plain conventional commits referencing `(#80)`; never amend,
  never push. Every commit message ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01FuWsPhYU2JAAEJ2vyChiYr
  ```
  (An executing session substitutes its own `Claude-Session` line.)
- **Mutation checks.**
  1. Commit first.
  2. Apply the mutant with an exact edit.
  3. Confirm the named test fails **and** that the mutant applied:
     `git -C $WT diff` shows it.
  4. Revert by reversing the exact edit, never with `git checkout`.
  5. Re-run the test and see it pass. `git -C $WT status` must then be
     clean.
- **Supervisor.** The supervising session is
  `issues-18daf754b5505502-3d` (SendMessage `to:
  "uds:/run/user/1000/cc-socks/2053498.sock"`). Its checkpoints:
  1. this plan, before any code;
  2. after Task 6 (P1 and P2 done, measurement 1 in hand);
  3. after Task 9;
  4. before any merge or push.
- **Scope (Priority 6).** Do **not** change `HashTrie`, any algorithm or
  any optimiser. The one exception is Task 3: it adds *type annotations
  only* inside `#[cfg(test)]` modules of `kermit-algos/src/sorted/*.rs`,
  needed because the tries gain a type parameter.

---

## Work packages

The ten tasks fall into packages. One implementer agent executes each
package end to end: its tasks in order, one commit per task, mutation
checks included. The package is then reviewed before the next starts. The
controller keeps Tasks 0, 6, 9 and 10 for itself: the merge, the gates, the
measurements, the supervisor checkpoints and landing.

| Package | Tasks | Scope | Depends on | Commits | Done when |
|---|---|---|---|---|---|
| *Controller* | 0 | Merge `origin/master` (with #84 phase 2); record `BASE`; build the baseline binary for measurement 1 | #84 phase 2 on master | — | `BASE` recorded; baseline release binary built; full `cargo test` green on the merge |
| **P1 — `kermit-ds`** | 1, 2, 3 | The `kermit-derive` rule; `seek.rs` and `test_support.rs`; generic tries and iterators; inference annotations; the DS-level suites | Task 0 | 3 | `cargo test -p kermit-derive -p kermit-ds -p kermit-algos` green; `cargo check --workspace --all-targets` clean; mutation checks recorded |
| **P2 — `kermit` binary** | 4, 5 | `--ds-layout-seek`, cells, families, dispatch, the `bench ds` rejection, the CLI test; the join, allocation and real-data suites | P1 | 2 | `cargo test -p kermit` green, including `cli_sorted_trie_layout_seek` (8 tests) and 192 sorted join tests; mutation checks recorded |
| *Controller* | 6 | Workspace gate and miri; measurement 1 (the default is unchanged); supervisor checkpoint 2 | P2 | — | — |
| **P3 — analysis and docs** | 7, 8 | kermit-lab back-fill and the axis → phases map; every doc in Task 8 | Task 6 | 2 | kermit-lab pytest green with `KERMIT_BIN`; `cargo doc` clean |
| *Controller* | 9, 10 | Final gate; measurement 2; supervisor checkpoint 3; landing only on the user's instruction | P3 | — | — |

Each implementer receives this plan, its package's task numbers, the
ground rules and a report budget of about 40 lines. The report covers
commit SHAs, test counts, mutation-check outcomes, and any deviation from
the plan with its reason. An implementer must not start the next package,
push or merge.

---

## Task 0 [controller]: Catch up and record the baseline

**Files:** none modified, apart from the merge commit.

**As executed (2026-10-03).**
- **Merge.** `origin/master` was `ec6f42b` (#84 phase 2 and #78 P2+P3).
  It merged cleanly into `0056fe1`, which is BASE.
- **Baseline binary.** The release build is deferred to Task 6, where
  measurement 1 needs it. BASE is pinned in `$SCRATCH/base.sha` and
  `git archive` reproduces it at any time. Building it now would compete
  with the P1 implementers' cargo runs on a memory-constrained host.
- **Green baseline (Step 4).** Satisfied by identity:
  `git diff --stat ec6f42b 0056fe1` shows only this spec and plan, and the
  supervisor ran the full gate (CI-scope miri included) on `ec6f42b`.

- [ ] **Step 1: Confirm the precondition**

```bash
git -C $WT fetch origin
git -C $WT log --oneline origin/master | grep -m1 -- '--ds-build selects ColumnTrie' \
  || echo 'STOP: #84 phase 2 is not on origin/master'
```

Expected: one line, #84's `feat(kermit): --ds-build selects ColumnTrie's build mode (#84)`. If you see `STOP` instead, stop and tell the supervisor.

- [ ] **Step 2: Merge `origin/master` into the branch (never rebase)**

```bash
git -C $WT merge origin/master
```

Expected: a merge, or a fast-forward. The branch so far holds only `docs/`
changes (this spec and plan), so conflicts are not expected.

- [ ] **Step 3: Record the baseline commit and build the baseline binary**

```bash
git -C $WT rev-parse HEAD | tee $SCRATCH/base.sha
mkdir -p $SCRATCH/kermit-base
git -C $WT archive "$(cat $SCRATCH/base.sha)" | tar -x -C $SCRATCH/kermit-base
CARGO_BUILD_JOBS=2 nix develop $WT --command \
  cargo build --release -p kermit --manifest-path $SCRATCH/kermit-base/Cargo.toml
ls -l $SCRATCH/kermit-base/target/release/kermit
```

Expected: the binary exists. It is measurement 1's `base` arm (Task 6).
Never `git worktree add`, because loom tracks worktrees.

- [ ] **Step 4: Green baseline**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace
```

Expected: all pass. Record the count of `test result:` lines and the totals
in the P1 brief, so later deltas can be explained.

---

# P1 — `kermit-ds`

## Task 1 [P1]: `#[derive(IntoTrieIter)]` accepts type parameters after `'a`

**Files:**
- Modify: `kermit-derive/src/lib.rs`
- Modify: `kermit-derive/tests/derive_into_trie_iter.rs`
- Delete: `kermit-derive/tests/ui/reject_type_param.rs`, `kermit-derive/tests/ui/reject_type_param.stderr`
- Create: `kermit-derive/tests/ui/reject_second_lifetime.rs` (+ `.stderr`), `kermit-derive/tests/ui/reject_const_param.rs` (+ `.stderr`)
- Modify: `kermit-derive/tests/ui/reject_no_lifetime.stderr`, `kermit-derive/tests/ui/reject_wrong_lifetime_name.stderr`

Task 3 makes the iterators `TreeTrieIter<'a, S>` and `ColumnTrieIter<'a,
S>`, which today's derive rejects with "requires exactly one generic
parameter". The #60 UI tests pinned the macro's existing shape; they did
not record a decision against generics. The new rule:

- the first parameter is the lifetime `'a`;
- further parameters must be type parameters;
- their bounds are carried into the impl by `split_for_impl`.

- [ ] **Step 1: Write the failing positive test**

Append to `kermit-derive/tests/derive_into_trie_iter.rs`:

```rust
// -- A type parameter after `'a`, as on `TreeTrieIter<'a, S>` --

/// Stands in for a Layout parameter such as a seek strategy.
trait Marker {}

struct Plain;

impl Marker for Plain {}

/// `MockTrieIter`, generic over a type parameter after `'a`. The derive must
/// carry `M: Marker` into the impl it generates.
#[derive(kermit_derive::IntoTrieIter)]
struct GenericMockTrieIter<'a, M: Marker> {
    inner: MockTrieIter<'a>,
    _marker: std::marker::PhantomData<M>,
}

impl<M: Marker> LinearIterator for GenericMockTrieIter<'_, M> {
    fn key(&self) -> Option<usize> { self.inner.key() }

    fn next(&mut self) -> Option<usize> { self.inner.next() }

    fn seek(&mut self, seek_key: usize) -> bool { self.inner.seek(seek_key) }

    fn at_end(&self) -> bool { self.inner.at_end() }
}

impl<M: Marker> TrieIterator for GenericMockTrieIter<'_, M> {
    fn open(&mut self) -> bool { self.inner.open() }

    fn up(&mut self) -> bool { self.inner.up() }
}

#[test]
fn derive_accepts_a_type_parameter_after_the_lifetime() {
    let trie = MockTrie {
        roots: vec![node(1, vec![leaf(2), leaf(3)]), node(4, vec![leaf(5)])],
    };
    let iter = GenericMockTrieIter::<Plain> {
        inner: MockTrieIter::new(&trie),
        _marker: std::marker::PhantomData,
    };
    let result: Vec<Vec<usize>> = iter.into_iter().collect();
    assert_eq!(result, vec![vec![1, 2], vec![1, 3], vec![4, 5]]);
}
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-derive --test derive_into_trie_iter`
Expected: compile error, `#[derive(IntoTrieIter)] requires exactly one generic parameter … found 2 parameter(s) on GenericMockTrieIter`.

- [ ] **Step 2: Generalise the derive**

In `kermit-derive/src/lib.rs`, replace the doc comment and body of
`derive_into_trie_iter` with:

```rust
/// Derives [`IntoIterator`] for a trie-iterator struct.
///
/// The annotated struct must:
/// - implement `kermit_iters::TrieIterator` (and therefore
///   `kermit_iters::LinearIterator`),
/// - take the lifetime `'a` as its first generic parameter, optionally
///   followed by type parameters (a Layout such as a seek strategy). Declare
///   their bounds on the struct: the generated impl carries them, which is
///   what lets it name `TrieIteratorWrapper<Self>`.
///
/// The expanded impl wraps `self` in a `kermit_iters::TrieIteratorWrapper`,
/// yielding each root-to-leaf path in the trie as a `Vec<usize>`.
#[proc_macro_derive(IntoTrieIter)]
pub fn derive_into_trie_iter(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = &input.ident;

    let params: Vec<_> = input.generics.params.iter().collect();
    let valid = match params.split_first() {
        | Some((GenericParam::Lifetime(lt), rest)) => {
            lt.lifetime.ident == "a" && rest.iter().all(|p| matches!(p, GenericParam::Type(_)))
        },
        | _ => false,
    };
    if !valid {
        let msg = format!(
            "#[derive(IntoTrieIter)] requires the lifetime `'a` as the first generic parameter, \
             optionally followed by type parameters; found {} parameter(s) on `{}`",
            params.len(),
            ident
        );
        return quote! { compile_error!(#msg); }.into();
    }

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let output = quote! {

        impl #impl_generics IntoIterator for #ident #ty_generics #where_clause {
            type Item = Vec<usize>;
            type IntoIter = TrieIteratorWrapper<Self>;

            fn into_iter(self) -> Self::IntoIter {
                TrieIteratorWrapper::new(self)
            }
        }

    };

    output.into()
}
```

In the crate's `//!` docs, after the `// The derive generates:` example
block, add the paragraph:

```rust
//! A struct generic over a type parameter after `'a`, such as
//! `TreeTrieIter<'a, S: SeekStrategy>`, gets
//! `impl<'a, S: SeekStrategy> IntoIterator for TreeTrieIter<'a, S>`.
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-derive --test derive_into_trie_iter`
Expected: PASS, including `derive_accepts_a_type_parameter_after_the_lifetime`.

- [ ] **Step 3: Update the UI cases**

```bash
git -C $WT rm -q kermit-derive/tests/ui/reject_type_param.rs kermit-derive/tests/ui/reject_type_param.stderr
```

Create `kermit-derive/tests/ui/reject_second_lifetime.rs`:

```rust
//! `#[derive(IntoTrieIter)]` must reject a second lifetime after `'a`.
use kermit_derive::IntoTrieIter;

#[derive(IntoTrieIter)]
struct TwoLifetimes<'a, 'b> {
    first: &'a [usize],
    second: &'b [usize],
}

fn main() {}
```

Create `kermit-derive/tests/ui/reject_const_param.rs`:

```rust
//! `#[derive(IntoTrieIter)]` must reject a const parameter after `'a`.
use kermit_derive::IntoTrieIter;

#[derive(IntoTrieIter)]
struct WithConst<'a, const N: usize> {
    data: &'a [usize; N],
}

fn main() {}
```

Regenerate the expected diagnostics, then read every `.stderr`:

```bash
TRYBUILD=overwrite CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-derive --test ui
for f in $WT/kermit-derive/tests/ui/*.stderr; do echo "== $f"; cat "$f"; done
```

Expected: four `.stderr` files. Each begins with `error: #[derive(IntoTrieIter)] requires the lifetime `'a` as the first generic parameter, optionally followed by type parameters; found N parameter(s) on `<Name>``. N is 0 for `NoLifetime`, 1 for `WrongName`, 2 for `TwoLifetimes` and 2 for `WithConst`. Each continues with the same ` --> tests/ui/<file>.rs:4:10` caret block as before. If any file shows anything else, the rule is wrong; fix the derive, don't the expectation.

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-derive`
Expected: all pass, with no `TRYBUILD` override set.

- [ ] **Step 4: Lint and commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-derive --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds   # the two existing derives still expand
git -C $WT add -A kermit-derive
git -C $WT commit -F - <<'EOF'
feat(kermit-derive): IntoTrieIter accepts type parameters after 'a (#80)

The derive required exactly one generic parameter, the lifetime 'a, so
it could not serve an iterator generic over a Layout, such as the sorted
tries' seek strategy (#80). The first parameter must still be 'a; type
parameters may follow, and split_for_impl carries their bounds into the
generated impl. A second lifetime or a const parameter is still
rejected, pinned by two new trybuild cases.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FuWsPhYU2JAAEJ2vyChiYr
EOF
```

---

## Task 2 [P1]: Linear, binary and galloping seek strategies

**Files:**
- Create: `kermit-ds/src/seek.rs`
- Create: `kermit-ds/src/test_support.rs`
- Modify: `kermit-ds/src/lib.rs`
- Modify: `kermit-ds/src/ds/column_trie/implementation.rs` (only the `#[cfg(test)]` module: the LCG moves out)

The strategies come first, with no consumer, so that their contract and
cost are pinned before any trie depends on them. Every strategy must return
exactly what `slice::partition_point` returns.

- [ ] **Step 1: Share the LCG**

Create `kermit-ds/src/test_support.rs`:

```rust
//! Helpers shared by the crate's inline tests.

/// Linear-congruential generator, so the randomised tests need no `rand`
/// dev-dependency. The constants are Knuth's MMIX ones.
pub(crate) struct Lcg(pub(crate) u64);

impl Lcg {
    /// The next pseudo-random value, below `2^31`.
    pub(crate) fn next_usize(&mut self) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize
    }
}
```

In `kermit-ds/src/lib.rs`, after `mod relation;`, add:

```rust
mod seek;
#[cfg(test)]
mod test_support;
```

In `kermit-ds/src/ds/column_trie/implementation.rs`, inside `#[cfg(test)] mod tests`, delete the `struct Lcg` definition, its doc comment and its `impl Lcg` block (added by #84). Add `test_support::Lcg` to the module's `crate::{…}` import. Every use (`Lcg(0x…)`, `rng.next_usize()`) stays as it is.

- [ ] **Step 2: Write the failing tests**

Create `kermit-ds/src/seek.rs` with only the test module, so it fails to
compile:

```rust
#[cfg(test)]
mod tests {
    use {
        super::{BinarySeek, GallopingSeek, LinearSeek, SeekStrategy},
        crate::test_support::Lcg,
        kermit_iters::LayoutOption,
        std::cell::Cell,
    };

    /// `S`'s offset for `target` in `remaining`, through the predicate a
    /// trie iterator's `seek` uses.
    fn seek_with<S: SeekStrategy>(remaining: &[usize], target: usize) -> usize {
        S::partition_point(remaining, |&k| k < target)
    }

    /// Each strategy by name, so a failure names the culprit.
    const STRATEGIES: [(&str, fn(&[usize], usize) -> usize); 3] = [
        ("linear", seek_with::<LinearSeek>),
        ("binary", seek_with::<BinarySeek>),
        ("galloping", seek_with::<GallopingSeek>),
    ];

    fn assert_agrees(slice: &[usize], start: usize, target: usize) {
        let remaining = &slice[start..];
        let want = remaining.partition_point(|&k| k < target);
        for (name, seek) in STRATEGIES {
            assert_eq!(
                seek(remaining, target),
                want,
                "{name}: {slice:?}[{start}..], target {target}"
            );
        }
    }

    /// Every strictly increasing slice over a small key range (each subset,
    /// in order: a sibling list has no duplicates), from every start offset,
    /// for every target from below the current key to past the last. This
    /// covers a target equal to the current key (offset 0), a backward
    /// target (offset 0) and one past the end (offset `len`).
    #[test]
    fn strategies_agree_with_partition_point_exhaustively() {
        const KEYS: usize = if cfg!(miri) { 6 } else { 10 };
        for subset in 0u32..(1 << KEYS) {
            let slice: Vec<usize> = (0..KEYS).filter(|k| subset & (1 << k) != 0).collect();
            for start in 0..=slice.len() {
                for target in 0..=KEYS {
                    assert_agrees(&slice, start, target);
                }
            }
        }
    }

    /// Long slices, with and without duplicates (the contract holds on any
    /// sorted slice), so a gallop crosses brackets of many sizes.
    #[test]
    fn strategies_agree_with_partition_point_on_random_sorted_slices() {
        let mut rng = Lcg(0x5EE4_5EE4_5EE4_5EE4);
        let cases = if cfg!(miri) { 20 } else { 500 };
        let max_len = if cfg!(miri) { 64 } else { 4096 };
        for case in 0..cases {
            let len = rng.next_usize() % max_len;
            // A spread of 1 makes duplicates common.
            let spread = 1 + rng.next_usize() % 8;
            let mut slice: Vec<usize> = (0..len)
                .map(|_| rng.next_usize() % (len * spread + 1))
                .collect();
            slice.sort_unstable();
            if case % 2 == 0 {
                slice.dedup();
            }
            let start = rng.next_usize() % (slice.len() + 1);
            let last = slice.last().copied().unwrap_or(0);
            let current = slice.get(start).copied().unwrap_or(0);
            for target in [rng.next_usize() % (last + 2), current, last + 1] {
                assert_agrees(&slice, start, target);
            }
        }
    }

    /// How many times `S` evaluates the predicate to place `target`.
    fn probes<S: SeekStrategy>(remaining: &[usize], target: usize) -> usize {
        let count = Cell::new(0);
        S::partition_point(remaining, |&k| {
            count.set(count.get() + 1);
            k < target
        });
        count.get()
    }

    /// Bits needed to write `n`, i.e. `⌊log₂ n⌋ + 1` (0 for 0): an upper
    /// bound on `⌈log₂ n⌉` that needs no float.
    fn bit_length(n: usize) -> usize { (usize::BITS - n.leading_zeros()) as usize }

    /// Pins each strategy's *cost*, which the agreement tests cannot see: a
    /// galloping search that degenerated into a scan would still return the
    /// right offsets. Probe counts are exact, so this also runs under miri.
    #[test]
    fn probe_counts_match_each_strategys_bound() {
        let n = if cfg!(miri) { 64 } else { 1024 };
        // Even keys: the least upper bound of `2 * d` sits `d` positions ahead.
        let slice: Vec<usize> = (0..n).map(|k| 2 * k).collect();
        for d in 0..=n {
            let target = 2 * d;
            assert_eq!(
                probes::<LinearSeek>(&slice, target),
                (d + 1).min(n),
                "linear, d = {d}"
            );
            let binary = probes::<BinarySeek>(&slice, target);
            assert!(
                binary <= bit_length(n) + 1,
                "binary, d = {d}: {binary} probes"
            );
            let galloping = probes::<GallopingSeek>(&slice, target);
            if d == 0 {
                assert_eq!(galloping, 1, "galloping must stop at the current key");
            } else {
                assert!(
                    galloping <= 2 * bit_length(d) + 2,
                    "galloping, d = {d}: {galloping} probes"
                );
            }
        }
        assert_eq!(probes::<LinearSeek>(&[], 1), 0);
        assert_eq!(probes::<BinarySeek>(&[], 1), 0);
        assert_eq!(probes::<GallopingSeek>(&[], 1), 0);
    }

    /// These strings are `ds_layout_seek` report values: renaming one splits
    /// every ablation across the rename.
    #[test]
    fn layout_names_are_pinned() {
        assert_eq!(<LinearSeek as LayoutOption>::NAME, "linear");
        assert_eq!(<BinarySeek as LayoutOption>::NAME, "binary");
        assert_eq!(<GallopingSeek as LayoutOption>::NAME, "galloping");
    }
}
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds seek::`
Expected: compile error, `unresolved imports super::BinarySeek …`.

- [ ] **Step 3: Implement the strategies**

Insert above the test module in `kermit-ds/src/seek.rs`:

```rust
//! Seek strategies: the Layout dimension of the sorted tries,
//! [`TreeTrie`](crate::TreeTrie) and [`ColumnTrie`](crate::ColumnTrie).
//!
//! A sorted-trie iterator's `seek(target)` moves to the least upper bound
//! of `target` among the siblings it has not yet passed. *Where* that bound
//! sits is a [`slice::partition_point`] question over the remaining
//! siblings. *How* to answer it is the strategy:
//!
//! | Strategy | Probes for a seek that moves `d` of `n` remaining siblings |
//! |---|---|
//! | [`LinearSeek`] | `min(d + 1, n)`: a scan, cheapest for one-step seeks |
//! | [`BinarySeek`] | `⌈log₂ n⌉ + 1`, whatever `d` is |
//! | [`GallopingSeek`] | 1 if `d = 0`, at most `2⌈log₂ d⌉ + 2` otherwise |
//!
//! Every strategy returns exactly what `partition_point` returns, so an
//! iterator's state after a seek is the same under all three; only the cost
//! differs. The strategy is a type parameter rather than a runtime value
//! because a runtime switch would put a branch in every seek. Under the
//! default, [`BinarySeek`], both tries compile to the `partition_point`
//! call they made before the parameter existed. Bench axis
//! `ds_layout_seek`; see `docs/data-structures/seek-strategies.md`.

use {kermit_iters::LayoutOption, std::fmt::Debug};

/// Compile-time seek strategy of a sorted trie: how its iterator's `seek`
/// finds the least upper bound among the siblings it has not yet passed.
///
/// Implementors are zero-sized markers, so a trie pays nothing for the
/// strategies it does not use.
pub trait SeekStrategy: LayoutOption + Copy + Debug + Default + 'static {
    /// The number of leading elements of `remaining` that satisfy `below`,
    /// exactly as [`slice::partition_point`] returns it.
    ///
    /// `below` must hold on a prefix of `remaining` and on nothing after it,
    /// as `|k| k < target` does on a sorted slice. The result is then the
    /// offset of the least upper bound: `remaining.len()` if there is none,
    /// and 0 if the first element is already not below. Strategies differ
    /// only in which elements they probe.
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], below: P) -> usize;
}

/// Scans forward one element at a time: O(d) for a seek that moves `d`
/// positions. The cheapest strategy when seeks move one or two siblings, and
/// the algorithm `TreeTrie` used before issue #67.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinearSeek;

/// Binary-searches everything not yet passed: O(log n) in the `n` remaining
/// siblings, however far the seek moves. The default, and what both sorted
/// tries did before the strategy became a parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BinarySeek;

/// Gallops from the current position, probing offsets 0, 1, 2, 4, … until
/// one is no longer below, then binary-searches the bracket the last two
/// probes found: O(log d) for a seek that moves `d` positions. This is the
/// cost Veldhuizen's LFTJ analysis assumes for its amortised
/// O(1 + log(N/m)) seek bound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GallopingSeek;

impl LayoutOption for LinearSeek {
    const NAME: &'static str = "linear";
}

impl LayoutOption for BinarySeek {
    const NAME: &'static str = "binary";
}

impl LayoutOption for GallopingSeek {
    const NAME: &'static str = "galloping";
}

impl SeekStrategy for LinearSeek {
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], mut below: P) -> usize {
        remaining
            .iter()
            .position(|element| !below(element))
            .unwrap_or(remaining.len())
    }
}

impl SeekStrategy for BinarySeek {
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], below: P) -> usize {
        remaining.partition_point(below)
    }
}

impl SeekStrategy for GallopingSeek {
    fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], mut below: P) -> usize {
        // A seek usually lands close by, so probe the current position
        // first: a seek that does not move costs one probe.
        if remaining.is_empty() || !below(&remaining[0]) {
            return 0;
        }
        // Gallop: double `bound` while it is still below. Invariant: `below`
        // holds at offset `bound / 2`. The loop doubles only while
        // `bound < len <= isize::MAX`, so the doubling cannot overflow.
        let mut bound = 1;
        while bound < remaining.len() && below(&remaining[bound]) {
            bound *= 2;
        }
        // The answer lies in (bound / 2, min(bound, len)]; binary-search the
        // elements strictly between. `lo <= hi`: the loop ran only while
        // `bound / 2 < len`.
        let lo = bound / 2 + 1;
        let hi = bound.min(remaining.len());
        lo + remaining[lo..hi].partition_point(below)
    }
}
```

In `kermit-ds/src/lib.rs`, extend the `pub use { … }` block with
`seek::{BinarySeek, GallopingSeek, LinearSeek, SeekStrategy},`. In the
crate's `//!` docs, after the paragraph listing the three structures, add:

```rust
//! The two sorted tries take their seek algorithm as a Layout type
//! parameter, [`SeekStrategy`]: [`LinearSeek`], [`BinarySeek`] (the
//! default) or [`GallopingSeek`].
```

- [ ] **Step 4: Run the tests**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds seek::`
Expected: the four tests PASS.

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds`
Expected: all pass, including #84's ColumnTrie equivalence tests now on the shared `Lcg`.

- [ ] **Step 5: Lint and commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-ds --all-targets -- -D warnings
git -C $WT add kermit-ds/src/seek.rs kermit-ds/src/test_support.rs kermit-ds/src/lib.rs kermit-ds/src/ds/column_trie/implementation.rs
git -C $WT commit -F - <<'EOF'
feat(kermit-ds): linear, binary and galloping seek strategies (#80)

SeekStrategy::partition_point is the one question a sorted-trie seek
asks: how many of the remaining siblings lie below the target. Every
strategy must answer exactly as slice::partition_point does; they
differ only in which elements they probe. LinearSeek scans, BinarySeek
is partition_point itself, and GallopingSeek probes offsets 0, 1, 2,
4, ... and then binary-searches the bracket, O(log d) in the distance
moved.

Tests: exhaustive agreement over every sibling list drawn from ten
keys, randomised agreement on long sorted slices with and without
duplicates, and exact probe counts that pin each strategy's cost (and
run under miri). The test LCG moves to a shared test_support module.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FuWsPhYU2JAAEJ2vyChiYr
EOF
```

- [ ] **Step 6: Mutation checks**

Run each mutant against `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds seek::`.

- **Mutant A: the bracket loses its top.** In `GallopingSeek`, change
  `let hi = bound.min(remaining.len());` to
  `let hi = (bound - 1).min(remaining.len());`. Expected:
  `strategies_agree_with_partition_point_exhaustively` FAILS (it panics on
  `lo > hi`, or gives a wrong offset when the answer is `bound`). Reverse
  the edit and re-run: PASS.
- **Mutant B: no current-position probe.** Replace
  `if remaining.is_empty() || !below(&remaining[0]) {` with
  `if remaining.is_empty() {`. Expected: the exhaustive test FAILS
  (target == current key returns 1, not 0). Reverse the edit and re-run:
  PASS.
- **Mutant C: linear is secretly binary.** Replace `LinearSeek`'s body
  with `remaining.partition_point(below)`, and rename its `mut below`
  parameter to `below` for the warning. Expected: both agreement tests
  still PASS, while `probe_counts_match_each_strategys_bound` FAILS
  (`linear, d = …`). This mutant is why that test exists. Reverse the edit
  and re-run: PASS.

---

## Task 3 [P1]: The sorted tries take their seek strategy as a Layout

**Files:**
- Modify: `kermit-ds/src/seek.rs` (`seek_axes`)
- Modify: `kermit-ds/src/ds/tree_trie/implementation.rs`, `tree_trie_iter.rs`, `tests.rs`
- Modify: `kermit-ds/src/ds/column_trie/implementation.rs`, `column_trie_iter.rs`, plus every other `impl … ColumnTrie` block #84 added (e.g. `BuildModeRelation`)
- Modify: `kermit-ds/src/relation.rs` (annotations in doctests/tests)
- Modify: `kermit-ds/tests/trie_tests.rs`, `kermit-ds/tests/parquet_tests.rs`, `kermit-ds/tests/common/macros.rs`
- Modify (annotations only, `#[cfg(test)]`): `kermit-algos/src/sorted/leapfrog_triejoin.rs`, `selection.rs`, `iter_kind.rs`; `kermit/src/db.rs`, `kermit/src/db/validation.rs`; `kermit/tests/*.rs` as the compiler demands

- [ ] **Step 1: Write the failing structure-level tests**

Append to `kermit-ds/src/ds/tree_trie/tests.rs`. Add
`crate::{BinarySeek, GallopingSeek, HeapSize, LinearSeek, SeekStrategy}`
and `kermit_iters::HasOptimizationAxes` to its `use` block.

```rust
/// The seek strategy is a type-level choice: it adds no field to the trie
/// or its iterator and no heap, so no instantiation pays for the others
/// (the non-user-tax test of `docs/specs/optimization-standard.md`).
#[test]
fn seek_strategy_adds_no_state() {
    use std::mem::{size_of, size_of_val};
    let tuples = vec![vec![1, 2], vec![1, 3], vec![2, 4]];
    let linear: TreeTrie<LinearSeek> = TreeTrie::from_tuples(2.into(), tuples.clone());
    let binary: TreeTrie<BinarySeek> = TreeTrie::from_tuples(2.into(), tuples.clone());
    let galloping: TreeTrie<GallopingSeek> = TreeTrie::from_tuples(2.into(), tuples);
    assert_eq!(size_of::<TreeTrie<LinearSeek>>(), size_of::<TreeTrie<BinarySeek>>());
    assert_eq!(size_of::<TreeTrie<GallopingSeek>>(), size_of::<TreeTrie<BinarySeek>>());
    assert_eq!(size_of_val(&linear.trie_iter()), size_of_val(&binary.trie_iter()));
    assert_eq!(size_of_val(&galloping.trie_iter()), size_of_val(&binary.trie_iter()));
    assert_eq!(linear.heap_size_bytes(), binary.heap_size_bytes());
    assert_eq!(galloping.heap_size_bytes(), binary.heap_size_bytes());
}

/// The relation reports its strategy as the `ds_layout_seek` axis, from its
/// type.
#[test]
fn optimization_axes_name_the_seek_strategy() {
    fn seek_axis<S: SeekStrategy>() -> serde_json::Value {
        TreeTrie::<S>::new(1.into()).optimization_axes()["ds_layout_seek"].clone()
    }
    assert_eq!(seek_axis::<LinearSeek>(), "linear");
    assert_eq!(seek_axis::<BinarySeek>(), "binary");
    assert_eq!(seek_axis::<GallopingSeek>(), "galloping");
}
```

Then replace the two existing tests `seek_backward_panics` and
`open_after_failed_seek_descends_from_last_positioned_node` with versions
that run under every strategy. These pin the code a strategy may not touch.

```rust
/// Seeks backward on a `TreeTrie<S>`: must panic whatever `S` is, because
/// the check precedes the strategy.
fn seek_backward<S: SeekStrategy>() {
    let trie: TreeTrie<S> = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![3], vec![5]]);
    let mut iter = trie.trie_iter();
    iter.open();
    iter.seek(3);
    iter.seek(1); // should panic — seeking backward
}

#[test]
#[should_panic(expected = "seek_key must be ≥ the key at the current position")]
fn seek_backward_panics_linear() { seek_backward::<LinearSeek>() }

#[test]
#[should_panic(expected = "seek_key must be ≥ the key at the current position")]
fn seek_backward_panics_binary() { seek_backward::<BinarySeek>() }

#[test]
#[should_panic(expected = "seek_key must be ≥ the key at the current position")]
fn seek_backward_panics_galloping() { seek_backward::<GallopingSeek>() }

/// A successful seek moves the stack top; one that runs off the end leaves
/// it on the last node the iterator was positioned on, so `open` still
/// descends into *that* node's children after `at_end`: the descent LFTJ
/// relies on (see `TreeTrieIter`'s position model). The strategy only
/// computes the offset, so this holds under each of them.
fn open_after_failed_seek<S: SeekStrategy>() {
    let trie: TreeTrie<S> =
        TreeTrie::from_tuples(2.into(), vec![vec![1, 5], vec![2, 6], vec![3, 7]]);
    let mut iter = trie.trie_iter();
    assert!(iter.open());
    assert!(iter.seek(2));
    assert!(!iter.seek(100));
    assert!(iter.at_end());
    assert!(iter.open());
    assert_eq!(iter.key(), Some(6));
}

#[test]
fn open_after_failed_seek_descends_from_last_positioned_node() {
    open_after_failed_seek::<LinearSeek>();
    open_after_failed_seek::<BinarySeek>();
    open_after_failed_seek::<GallopingSeek>();
}
```

Append the same two structure tests to the `#[cfg(test)] mod tests` of
`kermit-ds/src/ds/column_trie/implementation.rs`. Substitute `ColumnTrie`
for `TreeTrie`, and add `crate::{BinarySeek, GallopingSeek, LinearSeek,
SeekStrategy}` and `kermit_iters::HasOptimizationAxes` to its imports:

```rust
    /// See `TreeTrie`'s `seek_strategy_adds_no_state`.
    #[test]
    fn seek_strategy_adds_no_state() {
        use std::mem::{size_of, size_of_val};
        let tuples = vec![vec![1, 2], vec![1, 3], vec![2, 4]];
        let linear: ColumnTrie<LinearSeek> = ColumnTrie::from_tuples(2.into(), tuples.clone());
        let binary: ColumnTrie<BinarySeek> = ColumnTrie::from_tuples(2.into(), tuples.clone());
        let galloping: ColumnTrie<GallopingSeek> = ColumnTrie::from_tuples(2.into(), tuples);
        assert_eq!(size_of::<ColumnTrie<LinearSeek>>(), size_of::<ColumnTrie<BinarySeek>>());
        assert_eq!(size_of::<ColumnTrie<GallopingSeek>>(), size_of::<ColumnTrie<BinarySeek>>());
        assert_eq!(size_of_val(&linear.trie_iter()), size_of_val(&binary.trie_iter()));
        assert_eq!(size_of_val(&galloping.trie_iter()), size_of_val(&binary.trie_iter()));
        assert_eq!(linear.heap_size_bytes(), binary.heap_size_bytes());
        assert_eq!(galloping.heap_size_bytes(), binary.heap_size_bytes());
    }

    #[test]
    fn optimization_axes_name_the_seek_strategy() {
        fn seek_axis<S: SeekStrategy>() -> serde_json::Value {
            ColumnTrie::<S>::new(1.into()).optimization_axes()["ds_layout_seek"].clone()
        }
        assert_eq!(seek_axis::<LinearSeek>(), "linear");
        assert_eq!(seek_axis::<BinarySeek>(), "binary");
        assert_eq!(seek_axis::<GallopingSeek>(), "galloping");
    }
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib`
Expected: compile errors, `TreeTrie` / `ColumnTrie` take no type arguments.

- [ ] **Step 2: The axis helper**

Append to `kermit-ds/src/seek.rs`, above the test module, and extend its `use` to
`{kermit_iters::LayoutOption, serde_json::Value, std::{collections::BTreeMap, fmt::Debug}}`:

```rust
/// The `ds_layout_seek` axis of a sorted trie seeking with `S`, shared by
/// both tries' `HasOptimizationAxes` impls so the key is spelt once.
pub(crate) fn seek_axes<S: SeekStrategy>() -> BTreeMap<String, Value> {
    BTreeMap::from([(
        "ds_layout_seek".to_string(),
        Value::String(<S as LayoutOption>::NAME.to_string()),
    )])
}
```

- [ ] **Step 3: `TreeTrie<S>`**

In `kermit-ds/src/ds/tree_trie/implementation.rs`:

The `use` block becomes:

```rust
use {
    crate::{
        relation::{Relation, RelationHeader},
        seek::{seek_axes, BinarySeek, SeekStrategy},
    },
    kermit_iters::{HasOptimizationAxes, JoinIterable},
    serde_json::Value,
    std::{
        collections::BTreeMap,
        marker::PhantomData,
        ops::{Index, IndexMut},
    },
};
```

In the struct's doc comment, add a section before `# Example`:

```rust
/// # Seek strategy
///
/// `S` picks how the iterator's `seek` searches the siblings it has not yet
/// passed (see [`SeekStrategy`]); it changes no stored data. The default,
/// [`BinarySeek`], is a `partition_point` binary search, so plain
/// `TreeTrie` is the structure as it was before the parameter existed.
/// Bench axis `ds_layout_seek`.
///
```

In the doc example, change `let trie = TreeTrie::from_tuples(` to `let trie: TreeTrie = TreeTrie::from_tuples(`.

The struct becomes:

```rust
#[derive(Clone, Debug)]
pub struct TreeTrie<S: SeekStrategy = BinarySeek> {
    header: RelationHeader,
    children: Vec<TrieNode>,
    /// Number of distinct tuples stored; maintained by `insert`.
    tuple_count: usize,
    /// The seek strategy: a type-level choice, zero-sized.
    _seek: PhantomData<S>,
}
```

Make every impl block for `TreeTrie` generic: `impl<S: SeekStrategy>
TreeTrie<S>`, `impl<S: SeekStrategy> Relation for TreeTrie<S>`, `impl<S:
SeekStrategy> JoinIterable for TreeTrie<S> {}`, `impl<S: SeekStrategy>
crate::relation::Projectable for TreeTrie<S>`, `impl<S: SeekStrategy>
crate::heap_size::HeapSize for TreeTrie<S>` and `impl<S: SeekStrategy>
crate::cardinality::Cardinality for TreeTrie<S>`. In `Relation::new`, the
struct literal gains `_seek: PhantomData,`. Add:

```rust
impl<S: SeekStrategy> HasOptimizationAxes for TreeTrie<S> {
    /// The layout axis `ds_layout_seek`: the strategy's `LayoutOption::NAME`.
    fn optimization_axes(&self) -> BTreeMap<String, Value> { seek_axes::<S>() }
}
```

In `mod heap_size_tests`, annotate each binding the compiler flags: `let trie: TreeTrie = TreeTrie::new(…)`.

In `kermit-ds/src/ds/tree_trie/tree_trie_iter.rs`:

```rust
use {
    super::implementation::{TreeTrie, TrieNode},
    crate::seek::SeekStrategy,
    kermit_derive::IntoTrieIter,
    kermit_iters::{LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
};
```

Change the struct to `struct TreeTrieIter<'a, S: SeekStrategy>` with the
field `trie: &'a TreeTrie<S>,`. The other fields and every doc comment are
unchanged. Change the impls to `impl<'a, S: SeekStrategy> TreeTrieIter<'a,
S>` (`fn new(trie: &'a TreeTrie<S>) -> Self`), `impl<S: SeekStrategy>
LinearIterator for TreeTrieIter<'_, S>`, `impl<S: SeekStrategy> TrieIterator
for TreeTrieIter<'_, S>` and `impl<S: SeekStrategy> TrieIterable for
TreeTrie<S>`.

In `seek`, replace exactly this comment and statement:

```rust
                // Siblings are sorted by key (TreeTrie invariant), so
                // `partition_point(|n| n.key() < seek_key)` over the remaining
                // siblings is the offset of the first key ≥ `seek_key` — a
                // binary search, as in `ColumnTrieIter::seek`.
                self.sibling_idx +=
                    siblings[self.sibling_idx..].partition_point(|n| n.key() < seek_key);
```

with:

```rust
                // Siblings are sorted by key (TreeTrie invariant), so the
                // number of remaining siblings whose key is below `seek_key`
                // is the offset of the first key ≥ `seek_key`. `S` decides
                // how to count them (a scan, a binary search or a gallop);
                // every strategy returns the same offset, so nothing below
                // depends on `S`.
                self.sibling_idx +=
                    S::partition_point(&siblings[self.sibling_idx..], |n| n.key() < seek_key);
```

Nothing else in `seek` changes: not the `at_end` guard, the backward panic,
the success-only stack-top update, or the off-the-end comment.

- [ ] **Step 4: `ColumnTrie<S>`**

In `kermit-ds/src/ds/column_trie/implementation.rs`, apply the same
pattern:

- **Imports.** Extend them with `crate::seek::{seek_axes, BinarySeek,
  SeekStrategy}`, `kermit_iters::HasOptimizationAxes`, `serde_json::Value`
  and `std::{collections::BTreeMap, marker::PhantomData}`.
- **Struct docs.** Add the same `# Seek strategy` doc section, naming
  `ColumnTrie`.
- **Struct.** `pub struct ColumnTrie<S: SeekStrategy = BinarySeek>` gains
  `/// The seek strategy: a type-level choice, zero-sized.` and
  `_seek: PhantomData<S>,`.
- **Impls.** Every `impl … ColumnTrie` block becomes generic over `S`:
  inherent, `Display`, `JoinIterable`, `Projectable`, `Relation`,
  `HeapSize`, `Cardinality`, and #84's `BuildModeRelation` / `from_sorted`.
  Every struct literal `ColumnTrie { … }` / `Self { … }` gains `_seek:
  PhantomData`; the compiler lists them.
- **Axes.** Add `impl<S: SeekStrategy> HasOptimizationAxes for
  ColumnTrie<S>`, as for `TreeTrie`.
- **Doc example.** It becomes `let trie: ColumnTrie =
  ColumnTrie::from_tuples(`.

In `kermit-ds/src/ds/column_trie/column_trie_iter.rs`:

- Add `crate::seek::SeekStrategy` to the imports.
- `pub struct ColumnTrieIter<'a, S: SeekStrategy>` takes the field
  `trie: &'a ColumnTrie<S>,`.
- The impls become `impl<'a, S: SeekStrategy> ColumnTrieIter<'a, S>`
  (`pub fn new(trie: &'a ColumnTrie<S>) -> Self`), `impl<S: SeekStrategy>
  LinearIterator for ColumnTrieIter<'_, S>`, `impl<S: SeekStrategy>
  TrieIterator for ColumnTrieIter<'_, S>` and `impl<S: SeekStrategy>
  TrieIterable for ColumnTrie<S>`.

In `seek`, replace exactly:

```rust
            // `interval_slice` is sorted within each interval (ColumnTrie
            // invariant). For a sorted slice, `partition_point(|x| x <
            // target)` returns the index of the first element ≥ target —
            // exactly what `seek` needs.
            let remaining = &data[self.slice_offset..];
            let offset = remaining.partition_point(|&k| k < seek_key);
```

with:

```rust
            // `interval_slice` is sorted within each interval (ColumnTrie
            // invariant), so the number of remaining keys below `seek_key`
            // is the offset of the first key ≥ `seek_key`, exactly what
            // `seek` needs. `S` decides how to count them; every strategy
            // returns the same offset. A target at or below the current key
            // gives 0 under every strategy, so the iterator stays put, as
            // it always has.
            let remaining = &data[self.slice_offset..];
            let offset = S::partition_point(remaining, |&k| k < seek_key);
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib`
Expected: compiles, or fails only with E0283 at plain-`TreeTrie` / `ColumnTrie` constructor sites (Step 5). The new tests pass once Step 5 is done.

- [ ] **Step 5: Annotate the bindings inference no longer resolves**

Rust does not fall back to a defaulted type parameter in expression
position. So `let t = TreeTrie::from_tuples(…); t.trie_iter()` now fails
with **E0283** ("type annotations needed for `TreeTrie<_>`"), and so does a
binding that only ever reaches a generic function. A later use that names
the type (`Vec<&TreeTrie>`, a return type, a struct field) still pins `S`.

The fix is the HashTrie precedent (`let trie: HashTrie =
HashTrie::from_tuples(…)` in `hash_trie_iter.rs`'s tests): annotate the
binding with the plain alias, e.g. `let t: TreeTrie = TreeTrie::from_tuples(…)`,
or `let result: Result<TreeTrie, _> = TreeTrie::from_csv(&path);`. Change
nothing else.

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo check --workspace --all-targets 2>&1 | grep -E '^error|^ +--> ' | head -80
```

Fix every reported site and repeat until the check is clean. Then check the
doctests, which `cargo check` does not compile:

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --doc -p kermit-ds
```

Expected sites, by file:

- `kermit-ds/src/relation.rs`: the doctests and the `from_csv` /
  `from_parquet` tests;
- `kermit-ds/src/ds/tree_trie/tests.rs` and the column-trie tests;
- the `#[cfg(test)]` modules of
  `kermit-algos/src/sorted/{leapfrog_triejoin,selection,iter_kind}.rs`;
- `kermit/src/db.rs` and `kermit/src/db/validation.rs` (their tests);
- `kermit/tests/{watdiv_correctness,subject_position_constant,lubm_mini_oracle,lubm_cardinalities}.rs`.

Many of these sites are pinned already and need nothing.

Confirm that `kermit-algos` received annotations and nothing else
(Priority 6):

```bash
git -C $WT diff -- kermit-algos | grep -E '^[-+][^-+]' | grep -vE '(TreeTrie|ColumnTrie)' || echo 'annotations only'
git -C $WT diff -- kermit-algos/src | grep -E '^[-+][^-+]' | grep -cvE '(TreeTrie|ColumnTrie)'
```

Expected: `annotations only`, and a count of `0`. Quote both outputs in
the Task 3 commit body (supervisor requirement, checkpoint 1).

**Stop rule.** Count the annotated sites
(`git -C $WT diff | grep -cE '^\+.*: (TreeTrie|ColumnTrie)'`). If the count
grows well past ~100, stop and report it to the supervisor before
continuing. The fallback is a `pub type TreeTrie = …<BinarySeek>` alias
over a renamed generic struct, which avoids the churn but breaks the
HashTrie precedent. The supervisor decides on it; the implementer does
not.

- [ ] **Step 6: The DS-level suites take one alias per structure × strategy**

Replace the two plain invocations in `kermit-ds/tests/trie_tests.rs`. Keep
#84's `define_build_mode_provider!(Incremental, …)`, its
`type ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>;` and
`relation_trie_test_suite!(ColumnTrieIncremental);`. That alias names
`ColumnTrie` in type position, so it is the default `binary` strategy.
`BuiltWith` derefs to its inner relation, so the seek-cost test's
`relation.optimization_axes()` resolves through `Deref`. If it does not,
add a forwarding `impl<R: HasOptimizationAxes, P> HasOptimizationAxes for
BuiltWith<R, P>` in `kermit-ds/src/built_with.rs` and report the
deviation. The `use` line becomes `use kermit_ds::{define_build_mode_provider,
BinarySeek, BuiltWith, ColumnTrie, ColumnTrieBuildMode, GallopingSeek,
LinearSeek, TreeTrie};`, and the six aliases replace the two plain
invocations:

```rust
mod common;

// One alias per structure × seek strategy: the Layout test obligation of
// `docs/specs/optimization-standard.md`. `TreeTrieBinary` is plain
// `TreeTrie`.
type TreeTrieLinear = TreeTrie<LinearSeek>;
type TreeTrieBinary = TreeTrie<BinarySeek>;
type TreeTrieGalloping = TreeTrie<GallopingSeek>;
type ColumnTrieLinear = ColumnTrie<LinearSeek>;
type ColumnTrieBinary = ColumnTrie<BinarySeek>;
type ColumnTrieGalloping = ColumnTrie<GallopingSeek>;

relation_trie_test_suite!(TreeTrieLinear, TreeTrieBinary, TreeTrieGalloping);

relation_trie_test_suite!(ColumnTrieLinear, ColumnTrieBinary, ColumnTrieGalloping);
```

In `kermit-ds/tests/parquet_tests.rs`, add `BinarySeek, GallopingSeek,
LinearSeek` to the `kermit_ds::{…}` import. Replace
`parquet_test_suite!(TreeTrie);` and `parquet_test_suite!(ColumnTrie);`
with:

```rust
// One alias per structure × seek strategy, matching `trie_tests.rs`.
type TreeTrieLinear = TreeTrie<LinearSeek>;
type TreeTrieBinary = TreeTrie<BinarySeek>;
type TreeTrieGalloping = TreeTrie<GallopingSeek>;
type ColumnTrieLinear = ColumnTrie<LinearSeek>;
type ColumnTrieBinary = ColumnTrie<BinarySeek>;
type ColumnTrieGalloping = ColumnTrie<GallopingSeek>;

parquet_test_suite!(TreeTrieLinear);

parquet_test_suite!(TreeTrieBinary);

parquet_test_suite!(TreeTrieGalloping);

parquet_test_suite!(ColumnTrieLinear);

parquet_test_suite!(ColumnTrieBinary);

parquet_test_suite!(ColumnTrieGalloping);
```

- [ ] **Step 7: A varied-distance fixture and the seek-cost gate**

In `kermit-ds/tests/common/macros.rs`, inside `trie_seek_tests!`'s
`mod trie_seek`, add after `seek_high_fan_out_lands_on_least_upper_bound`:

```rust
            /// Seeks at Fibonacci distances (1, 2, 3, 5, 8, …) through one
            /// wide sibling list, so a galloping seek crosses brackets of
            /// every size and lands on their edges, shapes the 3-5-key
            /// fixtures above never reach (issue #80). Keys are even, so the
            /// odd target `2(i + d) - 1` lands on index `i + d`.
            #[test]
            fn seek_varied_distances_land_on_least_upper_bound() {
                use {
                    kermit_ds::Relation,
                    kermit_iters::{LinearIterator, TrieIterable, TrieIterator},
                };
                const FAN_OUT: usize = 1000;
                let tuples = (0..FAN_OUT).map(|i| vec![1, 2 * i]).collect();
                let relation = $relation_type::from_tuples(2_usize.into(), tuples);
                let mut iter = relation.trie_iter();
                assert!(iter.open());
                assert!(iter.open());
                let (mut index, mut distance, mut next_distance) = (0, 1, 2);
                while index + distance < FAN_OUT {
                    let target = 2 * (index + distance) - 1;
                    assert!(iter.seek(target), "seek({target}) fell off the end");
                    assert_eq!(
                        iter.key(),
                        Some(target + 1),
                        "seek({target}) from index {index}"
                    );
                    index += distance;
                    (distance, next_distance) = (next_distance, distance + next_distance);
                }
                assert!(!iter.seek(2 * FAN_OUT));
                assert!(iter.at_end());
                // The failed seek left the parent level intact.
                assert!(iter.up());
                assert_eq!(iter.key(), Some(1));
            }
```

Then replace the doc comment, name and closing assertion of
`seek_cost_is_independent_of_distance`. The setup, the `time_batch` helper
and the constants stay as they are:

```rust
            /// Pins `seek`'s *complexity* through the real iterator, which
            /// the contract tests above cannot see, in the direction the
            /// relation's seek strategy promises. The strategy is read from
            /// the `ds_layout_seek` axis, which comes from the type, so an
            /// alias cannot be mislabelled.
            ///
            /// - `binary` and `galloping` are sublinear: seeking across a
            ///   whole high-fan-out sibling list costs about as much as
            ///   seeking one step. LFTJ's worst-case-optimality bound assumes
            ///   this (issue #67).
            /// - `linear` is O(distance) by design (issue #80): the far seek
            ///   costs ~fan-out times the near one. If it ever met the
            ///   sublinear bound, the linear arm of every seek ablation would
            ///   measure the wrong thing.
            ///
            /// Wall-clock is the only observable: keys are plain `usize`, so
            /// comparisons cannot be counted without instrumenting the trie
            /// (`seek.rs` counts them for the strategies themselves). The
            /// ratio calibrates itself (machine speed and debug/release
            /// cancel); the near and far batches interleave and each keeps
            /// its fastest run, so scheduler noise cannot single one side
            /// out; and `MAX_RATIO` sits far from both outcomes (~1x for a
            /// sublinear search, hundreds of x for a scan).
            #[test]
            #[cfg_attr(
                miri,
                ignore = "wall-clock complexity check; too slow under miri"
            )]
            fn seek_cost_matches_the_strategy() {
                use {
                    kermit_ds::Relation,
                    kermit_iters::{HasOptimizationAxes, LinearIterator, TrieIterable, TrieIterator},
                    std::{
                        hint::black_box,
                        time::{Duration, Instant},
                    },
                };
```

…and replace the final `assert!(ratio < MAX_RATIO, …);` with:

```rust
                let strategy = relation.optimization_axes()["ds_layout_seek"].clone();
                if strategy == "linear" {
                    assert!(
                        ratio >= MAX_RATIO,
                        "a linear seek across {FAN_OUT} siblings took only {ratio:.1}x a \
                         one-step seek ({far:?} vs {near:?}); `linear` must scan"
                    );
                } else {
                    assert!(
                        ratio < MAX_RATIO,
                        "a {strategy} seek across {FAN_OUT} siblings took {ratio:.1}x a \
                         one-step seek ({far:?} vs {near:?}); it must be sublinear in the \
                         fan-out"
                    );
                }
```

Update the `relation_trie_test_suite!` doc comment (if it has one) or the
comment above `trie_seek_tests!` to say that the suite needs the relation
to implement `HasOptimizationAxes` with a `ds_layout_seek` key, as both
sorted tries do.

- [ ] **Step 8: Run the tests**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo check --workspace --all-targets
```

Expected: all pass, and the check is clean. `cargo test -p kermit-ds --test
trie_tests` now runs six `relation_trie_test_suite` modules
(`treetrielinear` … `columntriegalloping`). The two linear
`seek_cost_matches_the_strategy` tests each take a few seconds in a debug
build: their far batches scan 4096 siblings per seek.

- [ ] **Step 9: Lint and commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --workspace --all-targets -- -D warnings
git -C $WT add -A kermit-ds kermit-algos kermit
git -C $WT commit -F - <<'EOF'
feat(kermit-ds): sorted tries take their seek strategy as a Layout (#80)

TreeTrie<S> and ColumnTrie<S> gain an S: SeekStrategy parameter,
defaulting to BinarySeek, so plain TreeTrie / ColumnTrie still compile
to today's partition_point call. Each iterator's seek now asks
S::partition_point for the offset and nothing else: the at_end guard,
TreeTrie's backward-seek panic, its success-only stack-top update and
the off-the-end rule LFTJ relies on are unchanged. Both tries report
the strategy as ds_layout_seek through HasOptimizationAxes.

A defaulted parameter is no inference fallback in expression position,
so bindings used only through a method or a generic function are
annotated (let t: TreeTrie = ...), the HashTrie precedent; in
kermit-algos this touches test modules only, and only by annotation.

Tests: one alias per structure x strategy through the DS-level suites;
a Fibonacci-distance seek fixture; the seek-cost test now asserts the
cost its strategy promises (sublinear for binary and galloping, linear
for linear); zero-tax size and heap checks; TreeTrie's backward-panic
and open-after-failed-seek tests under every strategy.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FuWsPhYU2JAAEJ2vyChiYr
EOF
```

- [ ] **Step 10: Mutation checks**

- **Mutant A: TreeTrie ignores its strategy.** In `tree_trie_iter.rs`,
  change `S::partition_point(&siblings[self.sibling_idx..],` to
  `crate::seek::BinarySeek::partition_point(&siblings[self.sibling_idx..],`
  and add `use crate::seek::SeekStrategy as _;` if the trait method needs
  it in scope.
  Run `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --test trie_tests treetrielinear`.
  Expected: `treetrielinear::trie_seek::seek_cost_matches_the_strategy`
  FAILS (`a linear seek … took only …x`). Reverse the edit and re-run:
  PASS.
- **Mutant B: ColumnTrie ignores its strategy.** Make the same edit in
  `column_trie_iter.rs` (`S::partition_point(remaining,` becomes
  `crate::seek::BinarySeek::partition_point(remaining,`).
  Run `… cargo test -p kermit-ds --test trie_tests columntrielinear`.
  Expected: `columntrielinear::trie_seek::seek_cost_matches_the_strategy`
  FAILS. Reverse the edit and re-run: PASS. `git -C $WT status` is clean.

---

# P2 — the `kermit` binary

## Task 4 [P2]: `--ds-layout-seek` selects the sorted tries' seek strategy

**Files:**
- Modify: `kermit/src/options.rs`, `kermit/src/execution.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/src/main.rs`
- Create: `kermit/tests/cli_sorted_trie_layout_seek.rs`

Code below assumes #84 phase 2's shapes (see Preconditions).

- [ ] **Step 1: Write the failing CLI test**

Create `kermit/tests/cli_sorted_trie_layout_seek.rs`:

```rust
//! CLI smoke test for the sorted tries' seek-strategy Layout (#80).
//! `--ds-layout-seek` reaches both sorted cells and is recorded as
//! `ds_layout_seek`, and the default records `"binary"`. The flag is
//! rejected where it cannot act: on `hash-trie`, and on `bench ds`, none
//! of whose metrics seeks. Mirrors `cli_hash_trie_layout_pruning.rs`.

mod common;

use {
    common::cli::{axes_of, bench_ds, bench_join, bench_run, kermit_bin, reports_of},
    std::{
        path::PathBuf,
        process::{Command, Output},
    },
};

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_rejected(output: &Output, hint: &str) {
    assert!(
        !output.status.success(),
        "accepted: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-layout-seek"), "{stderr}");
    assert!(stderr.contains(hint), "{stderr}");
}

/// Runs `kermit join` over `first.csv` / `second.csv` with
/// `intersect_query.dl`, appending `extra_args`.
fn kermit_join(indexstructure: &str, algorithm: &str, extra_args: &[&str]) -> Output {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    Command::new(kermit_bin())
        .arg("join")
        .arg("--relations")
        .arg(fixtures.join("first.csv"))
        .arg(fixtures.join("second.csv"))
        .arg("--query")
        .arg(fixtures.join("intersect_query.dl"))
        .arg("--indexstructure")
        .arg(indexstructure)
        .arg("--algorithm")
        .arg(algorithm)
        .args(extra_args)
        .output()
        .expect("failed to execute kermit binary")
}

#[test]
fn cli_bench_run_default_seek_is_binary_on_both_sorted_tries() {
    for ds in ["tree-trie", "column-trie"] {
        let (output, report) =
            bench_run("triangle", &["-i", ds, "-a", "leapfrog-triejoin", "-m", "space"]);
        assert_success(&output);
        assert_eq!(axes_of(&report)["ds_layout_seek"], "binary", "{ds}");
    }
}

#[test]
fn cli_bench_run_with_galloping_records_axis() {
    for ds in ["tree-trie", "column-trie"] {
        let (output, report) = bench_run("triangle", &[
            "-i",
            ds,
            "-a",
            "leapfrog-triejoin",
            "-m",
            "space",
            "--ds-layout-seek",
            "galloping",
        ]);
        assert_success(&output);
        assert_eq!(axes_of(&report)["ds_layout_seek"], "galloping", "{ds}");
    }
}

#[test]
fn cli_bench_run_sweep_carries_seek_only_to_sorted_trie_cells() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-layout-seek",
        "linear",
    ]);
    assert_success(&output);
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert!(axes.get("ds_layout_seek").is_none(), "{axes}");
        } else {
            assert_eq!(axes["ds_layout_seek"], "linear", "{axes}");
        }
    }
}

#[test]
fn cli_rejects_seek_on_hash_trie() {
    let (output, _) = bench_run("triangle", &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "-m",
        "space",
        "--ds-layout-seek",
        "linear",
    ]);
    assert_rejected(&output, "tree-trie or column-trie");
    let (output, _) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--ds-layout-seek",
        "linear",
    ]);
    assert_rejected(&output, "tree-trie or column-trie");
    let output = kermit_join("hash-trie", "hash-triejoin", &["--ds-layout-seek", "linear"]);
    assert_rejected(&output, "tree-trie or column-trie");
}

/// None of `bench ds`'s metrics calls `seek`, so a seek "ablation" there
/// would time identical code under two labels.
#[test]
fn cli_bench_ds_rejects_seek_on_every_structure() {
    for ds in ["tree-trie", "column-trie", "hash-trie", "all"] {
        let (output, _) = bench_ds(ds, &["--ds-layout-seek", "galloping"]);
        assert_rejected(&output, "no effect on bench ds");
    }
}

#[test]
fn cli_bench_ds_reports_the_default_seek() {
    let (output, report) = bench_ds("tree-trie", &[]);
    assert_success(&output);
    assert_eq!(axes_of(&report)["ds_layout_seek"], "binary");
}

#[test]
fn cli_bench_join_records_the_seek_strategy() {
    let (output, report) = bench_join("column-trie", "leapfrog-triejoin", &[
        "-m",
        "space",
        "--ds-layout-seek",
        "linear",
    ]);
    assert_success(&output);
    assert_eq!(axes_of(&report)["ds_layout_seek"], "linear");
}

#[test]
fn cli_kermit_join_answers_are_identical_under_every_strategy() {
    for ds in ["tree-trie", "column-trie"] {
        let outputs: Vec<String> = ["linear", "binary", "galloping"]
            .iter()
            .map(|seek| {
                let output = kermit_join(ds, "leapfrog-triejoin", &["--ds-layout-seek", seek]);
                assert_success(&output);
                String::from_utf8(output.stdout).expect("utf-8 CSV")
            })
            .collect();
        assert!(
            outputs[0].lines().count() > 1,
            "{ds}: expected a header and rows, got {:?}",
            outputs[0]
        );
        assert_eq!(outputs[0], outputs[1], "{ds}: linear vs binary");
        assert_eq!(outputs[1], outputs[2], "{ds}: binary vs galloping");
    }
}
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_sorted_trie_layout_seek`
Expected: the tests FAIL (clap: unexpected argument `--ds-layout-seek`;
the default-seek tests fail on the missing axis).

- [ ] **Step 2: Write the failing unit tests**

Append to `mod tests` in `kermit/src/options.rs`, and add `kermit_ds::{BinarySeek, GallopingSeek, LinearSeek}` to its imports:

```rust
    /// The same round trip for `--ds-layout-seek` and `SeekStrategy`.
    #[test]
    fn seek_choices_round_trip_through_layout_names() {
        const TABLE: &[(SeekChoice, &str)] = &[
            (SeekChoice::Linear, <LinearSeek as LayoutOption>::NAME),
            (SeekChoice::Binary, <BinarySeek as LayoutOption>::NAME),
            (SeekChoice::Galloping, <GallopingSeek as LayoutOption>::NAME),
        ];
        for choice in SeekChoice::value_variants() {
            let (_, name) = TABLE
                .iter()
                .find(|(listed, _)| listed == choice)
                .unwrap_or_else(|| panic!("{choice:?} is missing from the round-trip table"));
            assert_eq!(SeekChoice::from_layout_name(name), Some(*choice));
        }
        assert_eq!(seek_of::<LinearSeek>(), SeekChoice::Linear);
        assert_eq!(seek_of::<BinarySeek>(), SeekChoice::Binary);
        assert_eq!(seek_of::<GallopingSeek>(), SeekChoice::Galloping);
    }

    /// Every `with_sorted_trie_layout!` arm binds the marker its
    /// `SeekChoice` names, so a transposed arm fails here rather than
    /// mislabelling a bench report.
    #[test]
    fn sorted_layout_macro_binds_the_marker_its_arm_names() {
        for &seek in SeekChoice::value_variants() {
            assert_eq!(with_sorted_trie_layout!(seek, |S| seek_of::<S>()), seek);
        }
    }

    #[test]
    fn validate_layout_choices_accepts_seek_on_sorted_tries_or_all_only() {
        let layout = LayoutChoices {
            sorted_trie_seek: Some(SeekChoice::Galloping),
            ..LayoutChoices::default()
        };
        for sel in [
            IndexStructureSelector::TreeTrie,
            IndexStructureSelector::ColumnTrie,
            IndexStructureSelector::All,
        ] {
            assert!(validate_layout_choices(sel, &layout).is_ok(), "{sel:?}");
        }
        let msg = validate_layout_choices(IndexStructureSelector::HashTrie, &layout)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("--ds-layout-seek"), "{msg}");
        assert!(msg.contains("tree-trie or column-trie"), "{msg}");
    }

    #[test]
    fn ds_choices_resolve_carries_the_seek_strategy() {
        let layout = LayoutChoices {
            sorted_trie_seek: Some(SeekChoice::Linear),
            ..LayoutChoices::default()
        };
        let choices = DsChoices::resolve(
            IndexStructureSelector::TreeTrie,
            &layout,
            &ConfigChoices::default(),
            &BuildChoices::default(),
        )
        .unwrap();
        assert_eq!(choices.seek, SeekChoice::Linear);
        assert_eq!(DsChoices::default().seek, SeekChoice::Binary);
        assert!(DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &layout,
            &ConfigChoices::default(),
            &BuildChoices::default(),
        )
        .is_err());
    }
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit seek`
Expected: compile errors (`SeekChoice`, `seek_of`, `with_sorted_trie_layout` and `sorted_trie_seek` are not found).

- [ ] **Step 3: `SeekChoice`, `seek_of`, the flag and the validator**

In `kermit/src/options.rs`, add `SeekStrategy` to the `kermit_ds::{…}` import. Change the module doc's first line to:

```rust
//! CLI option groups for the optimisation axes (`--ds-layout-*`,
//! `--ds-config`, `--ds-build`) and the places the Layout products are
//! monomorphised: `with_hash_trie_layout!` and `with_sorted_trie_layout!`.
```

After `pruning_of`, add:

```rust
/// CLI-side selector for `--ds-layout-seek`: the [`SeekStrategy`]
/// monomorphised into `TreeTrie<S>` / `ColumnTrie<S>`. `Binary`, the
/// default, is the `partition_point` search both sorted tries used before
/// the parameter existed.
///
/// Only valid when the selected index structure is `tree-trie` or
/// `column-trie` (or `all`): `validate_layout_choices` rejects it on
/// `hash-trie`, and `bench ds` rejects it outright, since none of its
/// metrics seeks.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SeekChoice {
    /// A scan (`LinearSeek`).
    Linear,
    /// A binary search (`BinarySeek`), the default.
    #[default]
    Binary,
    /// A galloping search (`GallopingSeek`).
    Galloping,
}

impl SeekChoice {
    /// The choice that monomorphises to the [`SeekStrategy`] marker whose
    /// [`LayoutOption::NAME`] is `name`, or `None` if no CLI choice does.
    /// The counterpart of [`HasherChoice::from_layout_name`].
    pub(crate) fn from_layout_name(name: &str) -> Option<Self> {
        match name {
            | "linear" => Some(Self::Linear),
            | "binary" => Some(Self::Binary),
            | "galloping" => Some(Self::Galloping),
            | _ => None,
        }
    }
}

/// The `--ds-layout-seek` label of the [`SeekStrategy`] a code path was
/// monomorphised over. The counterpart of [`hasher_of`].
///
/// # Panics
///
/// Panics if `S`'s layout name has no [`SeekChoice`], with the same caveat
/// about what the round-trip test covers; see [`hasher_of`].
pub(crate) fn seek_of<S: SeekStrategy>() -> SeekChoice {
    let name = <S as LayoutOption>::NAME;
    SeekChoice::from_layout_name(name).unwrap_or_else(|| {
        panic!(
            "no --ds-layout-seek choice for seek strategy {name:?} ({})",
            std::any::type_name::<S>()
        )
    })
}
```

In `LayoutChoices`, change the first paragraph of its doc to "Layout-axis
CLI choices flattened into every subcommand whose dispatch monomorphises
over a Layout-parameterised structure (`HashTrie<H, P>`, `TreeTrie<S>`,
`ColumnTrie<S>`)". Add the field:

```rust
    /// Seek strategy of `TreeTrie<S>` / `ColumnTrie<S>` (default:
    /// `binary`). Only valid when `--indexstructure tree-trie` or
    /// `column-trie` (or `all`) is selected, and not on `bench ds`, none of
    /// whose metrics seeks.
    #[arg(long = "ds-layout-seek", value_name = "SEEK", value_enum)]
    sorted_trie_seek: Option<SeekChoice>,
```

and the accessors:

```rust
    /// Returns the `SeekChoice` to monomorphise on, applying
    /// `SeekChoice::default()` when none was supplied on the command line.
    /// Use this at dispatch sites.
    pub(crate) fn sorted_trie_seek_resolved(&self) -> SeekChoice {
        self.sorted_trie_seek.unwrap_or_default()
    }

    /// Returns whether the user explicitly passed `--ds-layout-seek`.
    pub(crate) fn sorted_trie_seek_explicit(&self) -> bool { self.sorted_trie_seek.is_some() }
```

Replace `validate_layout_choices` (doc and body):

```rust
/// Rejects `LayoutChoices` flags that are incompatible with the chosen
/// `IndexStructureSelector`. Each flag names a Layout of particular
/// structures: `--ds-layout-hasher` and `--ds-layout-pruning` belong to
/// `hash-trie`, and `--ds-layout-seek` to `tree-trie` and `column-trie`.
/// `all` accepts every flag, because its sweep has a cell for each. A flag
/// on a structure without its Layout is a usage error: it would be silently
/// ignored, producing a benchmark report whose `ds_layout_*` axis disagrees
/// with the actual structure used.
pub(crate) fn validate_layout_choices(
    indexstructure: IndexStructureSelector, layout: &LayoutChoices,
) -> anyhow::Result<()> {
    let hash_trie = matches!(
        indexstructure,
        IndexStructureSelector::HashTrie | IndexStructureSelector::All
    );
    let sorted_trie = matches!(
        indexstructure,
        IndexStructureSelector::TreeTrie
            | IndexStructureSelector::ColumnTrie
            | IndexStructureSelector::All
    );
    // (flag, given, applies to the selection, the structures it applies to)
    let flags: &[(&str, bool, bool, &str)] = &[
        (
            "--ds-layout-hasher",
            layout.hash_trie_hasher_explicit(),
            hash_trie,
            "hash-trie",
        ),
        (
            "--ds-layout-pruning",
            layout.hash_trie_pruning_explicit(),
            hash_trie,
            "hash-trie",
        ),
        (
            "--ds-layout-seek",
            layout.sorted_trie_seek_explicit(),
            sorted_trie,
            "tree-trie or column-trie",
        ),
    ];
    for (flag, given, applies, structures) in flags {
        if *given && !applies {
            anyhow::bail!(
                "{flag} is only valid with --indexstructure {structures} (or all); got \
                 --indexstructure {indexstructure:?}"
            );
        }
    }
    Ok(())
}
```

The hash flags' message is unchanged, so their existing tests keep passing.

Add a field to `DsChoices`, after `pruning`:

```rust
    /// `--ds-layout-seek`; reaches the two sorted-trie cells.
    pub seek: SeekChoice,
```

and in `DsChoices::resolve`'s `Ok(Self { … })`, add `seek: layout.sorted_trie_seek_resolved(),`.

After `pub(crate) use with_hash_trie_layout;`, add:

```rust
/// Monomorphises `$body` over the sorted-trie seek strategy selected at
/// runtime. It is the sorted counterpart of `with_hash_trie_layout!`, and
/// the one place a `--ds-layout-seek` choice becomes a type. The two
/// products share no dimension (a sorted cell has no hasher, and a hash
/// cell no seek), so they are separate macros. Called by the sorted arms of
/// `dispatch_run_bench`, `dispatch_ds_bench` and `load_query_runner`.
///
/// Hygiene contract as for `with_hash_trie_layout!`: `$S` becomes a type
/// alias scoped to the arm.
macro_rules! with_sorted_trie_layout {
    ($seek:expr, | $S:ident | $body:expr) => {
        match $seek {
            | $crate::options::SeekChoice::Linear => {
                type $S = ::kermit_ds::LinearSeek;
                $body
            },
            | $crate::options::SeekChoice::Binary => {
                type $S = ::kermit_ds::BinarySeek;
                $body
            },
            | $crate::options::SeekChoice::Galloping => {
                type $S = ::kermit_ds::GallopingSeek;
                $body
            },
        }
    };
}

pub(crate) use with_sorted_trie_layout;
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit options::`
Expected: compile errors only in `execution.rs` and its callers (Step 4).
The options tests run once the crate builds.

- [ ] **Step 4: The cells carry the seek strategy, and the families report it**

In `kermit/src/execution.rs`, make these import changes:

- `crate::options::{…}` gains `seek_of, SeekChoice`;
- `kermit_ds::{…}` gains `SeekStrategy`.

Replace `enum SortedTrie` and `SortedTrie::index_structure`:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SortedTrie {
    /// Pointer-based trie (`-i tree-trie`), seeking with `seek`.
    TreeTrie {
        /// The `--ds-layout-seek` choice, in the two roles of
        /// `Execution::HashHtj`'s Layout fields: the request that selects
        /// `S`, and the label re-derived from `S`.
        seek: SeekChoice,
    },
    /// Column-oriented trie (`-i column-trie`), seeking with `seek` and
    /// built by the `--ds-build` mode `build`.
    ColumnTrie {
        /// As on [`SortedTrie::TreeTrie`].
        seek: SeekChoice,
        /// The `--ds-build` mode every relation is built with.
        build: ColumnTrieBuildMode,
    },
}

impl SortedTrie {
    /// The `IndexStructure` this sorted trie corresponds to.
    pub fn index_structure(self) -> IndexStructure {
        match self {
            | Self::TreeTrie {
                ..
            } => IndexStructure::TreeTrie,
            | Self::ColumnTrie {
                ..
            } => IndexStructure::ColumnTrie,
        }
    }
}
```

On `SortedTrieRelation`, add `HasOptimizationAxes` to the supertraits
(`Relation + RelationFileExt + TrieIterable + Cardinality + HeapSize +
HasOptimizationAxes`). Add this sentence to its doc: "Its Layout axes come
from the relation itself (`HasOptimizationAxes`), its build mode from the
family." Replace both impls:

```rust
impl<S: SeekStrategy> SortedTrieRelation for TreeTrie<S> {
    type BuildMode = ();

    fn kind(_: ()) -> SortedTrie {
        SortedTrie::TreeTrie {
            seek: seek_of::<S>(),
        }
    }

    fn build_with(header: RelationHeader, _: (), tuples: Vec<Vec<usize>>) -> Self {
        Self::from_tuples(header, tuples)
    }

    fn build_mode_axes(_: ()) -> BTreeMap<String, serde_json::Value> { BTreeMap::new() }
}

impl<S: SeekStrategy> SortedTrieRelation for ColumnTrie<S> {
    type BuildMode = ColumnTrieBuildMode;

    fn kind(build: ColumnTrieBuildMode) -> SortedTrie {
        SortedTrie::ColumnTrie {
            seek: seek_of::<S>(),
            build,
        }
    }

    fn build_with(
        header: RelationHeader, build: ColumnTrieBuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::from_tuples_with_build_mode(header, build, tuples)
    }

    /// Every ColumnTrie report says which build made it, so kermit-lab can
    /// read a ColumnTrie row *without* the axis as the pre-#84 incremental
    /// build.
    fn build_mode_axes(build: ColumnTrieBuildMode) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([(
            "ds_build_mode".to_string(),
            serde_json::Value::from(build.axis_value()),
        )])
    }
}
```

In `Execution::for_pair` and `Execution::for_structure`, destructure `seek`
too (`let DsChoices { hasher, pruning, seek, config, build } = choices;`),
and make the sorted arms:

```rust
            | (IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::TreeTrie {
                    seek,
                }))
            },
            | (IndexStructure::ColumnTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::ColumnTrie {
                    seek,
                    build,
                }))
            },
```

and, in `for_structure`, `IndexStructure::TreeTrie =>
Execution::TrieLftj(SortedTrie::TreeTrie { seek })` and
`IndexStructure::ColumnTrie =>
Execution::TrieLftj(SortedTrie::ColumnTrie { seek, build })`. Change the
last sentence of `for_pair`'s doc to: "`choices` reach the cells that have
each axis: the hash-trie cell's hasher, pruning and config, both sorted
cells' seek strategy, and the column-trie cell's build mode."

In `impl RelationFamily for SortedTrieFamily<R>`, replace
`optimization_axes`:

```rust
    /// The relation's own Layout axes (`ds_layout_seek`), read from the type
    /// it was monomorphised over.
    fn optimization_axes(rel: &R) -> BTreeMap<String, serde_json::Value> {
        rel.optimization_axes()
    }
```

Update the `mod tests` of `execution.rs`:

- In `families_report_their_own_execution`, the two sorted assertions
  become `Execution::TrieLftj(SortedTrie::TreeTrie { seek:
  SeekChoice::Binary })` and `Execution::TrieLftj(SortedTrie::ColumnTrie {
  seek: SeekChoice::Binary, build: ColumnTrieBuildMode::Incremental })`
  (the family there is built with `Incremental`).
- In `for_structure_agrees_with_for_pair`, add an innermost loop
  `for &seek in SeekChoice::value_variants()` and build `DsChoices {
  hasher, pruning, seek, config, build }`.
- In #84's `sweep_attaches_the_build_mode_to_the_column_trie_cell_only`,
  write the expected cells with `seek: SeekChoice::Binary`.
- Append:

```rust
    /// Each sorted family reports the seek strategy its type parameter
    /// implies, so the label cannot disagree with the code that ran; and a
    /// sweep attaches the `--ds-layout-seek` choice to both sorted cells.
    #[test]
    fn sorted_families_label_their_seek_strategy_from_the_type() {
        use kermit_ds::{BinarySeek, GallopingSeek, LinearSeek};
        assert_eq!(
            TrieLftj::<TreeTrie<LinearSeek>>::new((), Optimiser::Lexicographic).execution(),
            Execution::TrieLftj(SortedTrie::TreeTrie {
                seek: SeekChoice::Linear
            })
        );
        assert_eq!(
            TrieLftj::<ColumnTrie<GallopingSeek>>::new(
                ColumnTrieBuildMode::Incremental,
                Optimiser::Lexicographic
            )
            .execution(),
            Execution::TrieLftj(SortedTrie::ColumnTrie {
                seek: SeekChoice::Galloping,
                build: ColumnTrieBuildMode::Incremental
            })
        );
        assert_eq!(
            SortedTrieFamily::<TreeTrie<BinarySeek>>::default().execution(),
            Execution::TrieLftj(SortedTrie::TreeTrie {
                seek: SeekChoice::Binary
            })
        );
        let choices = DsChoices {
            seek: SeekChoice::Linear,
            ..DsChoices::default()
        };
        let sweep = Sweep::expand(&all_structures(), &all_algorithms(), choices);
        assert!(sweep.cells.contains(&Execution::TrieLftj(SortedTrie::TreeTrie {
            seek: SeekChoice::Linear
        })));
        assert!(sweep.cells.contains(&Execution::TrieLftj(SortedTrie::ColumnTrie {
            seek: SeekChoice::Linear,
            build: ColumnTrieBuildMode::default()
        })));
    }

    /// A sorted family's report axes are the relation's own.
    #[test]
    fn sorted_families_report_the_relations_seek_axis() {
        use kermit_ds::GallopingSeek;
        let family = TrieLftj::<TreeTrie<GallopingSeek>>::new((), Optimiser::Lexicographic);
        let rel = family.build_relation(header(), tuples());
        assert_eq!(
            TrieLftj::<TreeTrie<GallopingSeek>>::optimization_axes(&rel)["ds_layout_seek"],
            "galloping"
        );
    }
```

(`header()` and `tuples()` are the module's existing fixture helpers. If
they are named differently after #84, use those.)

- [ ] **Step 5: Dispatch through `with_sorted_trie_layout!`**

In `kermit/src/bench/run.rs`, add `with_sorted_trie_layout` to the `options::{…}` import, and make `dispatch_run_bench`'s two sorted arms:

```rust
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &TrieLftj::<kermit_ds::TreeTrie<S>>::new((), optimiser),
            workload,
            settings,
        )),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &TrieLftj::<kermit_ds::ColumnTrie<S>>::new(build, optimiser),
            workload,
            settings,
        )),
```

In `kermit/src/bench/ds.rs`, make the same import change, and make
`dispatch_ds_bench`'s sorted arms:

```rust
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
        }) => with_sorted_trie_layout!(seek, |S| run_ds_bench(
            &SortedTrieFamily::<kermit_ds::TreeTrie<S>>::default(),
            relation,
            metrics,
            queries_per_build,
            group_name,
            bench_args,
        )),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| run_ds_bench(
            &SortedTrieFamily::<kermit_ds::ColumnTrie<S>>::new(build),
            relation,
            metrics,
            queries_per_build,
            group_name,
            bench_args,
        )),
```

`bench ds` only ever passes `Binary` (Step 6 rejects the flag). The macro
is still used here because that rejection is CLI policy, not a type
invariant, and one dispatch shape reads more easily than a special case.

In `kermit/src/main.rs`, add `with_sorted_trie_layout` to the
`options::{…}` import.

**As landed (adapted at Task 0).** `load_query_runner(args: &QueryArgs,
choices: DsChoices)` takes its `DsChoices` from its two callers:

- `bench join --output` passes the result of `DsChoices::resolve`.
- `run_join` (`kermit join`) builds a literal,
  `DsChoices { hasher: …, pruning: …, ..DsChoices::default() }`.

A new `seek` field would silently take the default there, and no test
could notice: `kermit join` writes no report axis, and every strategy
gives the same answers. So `kermit join` resolves through
`DsChoices::resolve` like every other command, which also makes the
hand-duplicated check in `load_query_runner` dead.

1. In `run_join`, replace the `let choices = DsChoices { … };` literal with:

```rust
    // `kermit join` has no `--ds-config` / `--ds-build` (neither can change
    // an answer), so it resolves its layout flags against their defaults,
    // through the same validator as every bench command.
    let choices = DsChoices::resolve(
        IndexStructureSelector::of(query_args.indexstructure),
        &query_args.layout,
        &ConfigChoices::default(),
        &BuildChoices::default(),
    )?;
```

2. In `load_query_runner`, delete the hand-duplicated layout check: the
   comment beginning "Deliberately duplicates `validate_layout_choices`"
   and the `if args.indexstructure != IndexStructure::HashTrie { … }` block.
   Both callers now pass a `DsChoices` that `resolve` validated. Add one
   sentence to its doc comment: "Both callers resolve `choices` through
   `DsChoices::resolve`, which has already validated every `--ds-*`
   flag."
   `cli_join_tests.rs` asserts the substring `--ds-layout-pruning is only
   valid with --indexstructure hash-trie`, and `resolve`'s message still
   contains it.

3. Make its two sorted arms:

```rust
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
        }) => with_sorted_trie_layout!(seek, |S| build_join_runner(
            TrieLftj::<kermit_ds::TreeTrie<S>>::new((), optimiser),
            &args.relations,
        )),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| build_join_runner(
            TrieLftj::<kermit_ds::ColumnTrie<S>>::new(build, optimiser),
            &args.relations,
        )),
```

Drop any import the compiler reports unused, e.g. `IndexStructure` if the removed check was its last user.

**Known limitation, inherited (#86).** Like the hasher, pruning, config
and build validators, the seek validator checks `-i` only and ignores `-a`.
So `-i all -a hash-triejoin --ds-layout-seek galloping` validates, then
runs only the hash cell, which has no seek field. Keep the seek rule
consistent with the existing validators; #86 tracks the fix for all of
them, and #80 does not fix it (Priority 6).

- [ ] **Step 6: `bench ds` rejects the flag**

In `run_ds_bench_command` (`kermit/src/main.rs`), insert before
`DsChoices::resolve(…)`:

```rust
    // None of `bench ds`'s metrics calls `seek`: insertion builds the
    // relation, iteration and end_to_end scan it with open/next/up
    // (`TrieIteratorWrapper::advance`), and space measures it. Accepting
    // the flag would label identical measurements as a seek ablation.
    if layout.sorted_trie_seek_explicit() {
        anyhow::bail!(
            "--ds-layout-seek has no effect on bench ds: none of its metrics calls seek \
             (insertion builds the relation; iteration and end_to_end scan it with \
             open/next/up; space measures it). Time a join with bench run or bench join."
        );
    }
```

- [ ] **Step 7: Run the tests**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit`
Expected: all pass, including:

- the 8 `cli_sorted_trie_layout_seek` tests;
- the new `options::` and `execution::` tests;
- every existing `cli_*` test, including `cli_hash_trie_*` and
  `cli_column_trie_build_mode`, with unchanged messages.

- [ ] **Step 8: Lint and commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit --all-targets -- -D warnings
git -C $WT add kermit
git -C $WT commit -F - <<'EOF'
feat(kermit): --ds-layout-seek selects the sorted tries' seek strategy (#80)

bench run, bench join and kermit join take --ds-layout-seek
linear|binary|galloping on tree-trie, column-trie or all, and reject it
on hash-trie. bench ds rejects it outright: none of its metrics calls
seek, so it would label identical measurements as an ablation.

Both sorted cells carry the choice (SortedTrie::TreeTrie { seek },
ColumnTrie { seek, build }), with_sorted_trie_layout! is the one place
it becomes a type, and SortedTrieRelation::kind re-derives the label
from S, as the hash cell does with hasher_of. Reports carry
ds_layout_seek from the relation's own HasOptimizationAxes.
validate_layout_choices now checks each flag against the structures
its Layout belongs to, and kermit join uses it instead of a
hand-duplicated hash-only check.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FuWsPhYU2JAAEJ2vyChiYr
EOF
```

- [ ] **Step 9: Mutation checks**

Run each mutant against `CARGO_BUILD_JOBS=2 nix develop $WT --command
cargo test -p kermit --test cli_sorted_trie_layout_seek` and `… cargo test
-p kermit --bin kermit seek`.

- **Mutant A: a transposed macro arm.** In `with_sorted_trie_layout!`,
  change the `Galloping` arm's `type $S = ::kermit_ds::GallopingSeek;` to
  `type $S = ::kermit_ds::BinarySeek;`. Expected: both
  `sorted_layout_macro_binds_the_marker_its_arm_names` and
  `cli_bench_run_with_galloping_records_axis` FAIL. The axis comes from the
  type, so the report says `binary`. Reverse the edit and re-run: PASS.
- **Mutant B: the cell drops the choice.** In `Execution::for_pair`, change
  the TreeTrie arm to `SortedTrie::TreeTrie { seek: SeekChoice::Binary }`.
  Expected: `cli_bench_run_with_galloping_records_axis` FAILS for
  `tree-trie`. Reverse the edit and re-run: PASS.
- **Mutant C: `bench ds` accepts the flag.** Change `if
  layout.sorted_trie_seek_explicit() {` to `if false &&
  layout.sorted_trie_seek_explicit() {`. Expected:
  `cli_bench_ds_rejects_seek_on_every_structure` FAILS. Reverse the edit
  and re-run: PASS. `git -C $WT status` is clean.

---

## Task 5 [P2]: Every seek strategy through the join, allocation and real-data suites

**Files:**
- Modify: `kermit/tests/join_tests.rs`
- Modify: `kermit/tests/result_allocation.rs`
- Modify: `kermit/tests/lubm_mini_oracle.rs`, `kermit/tests/watdiv_correctness.rs`, `kermit/tests/lubm_cardinalities.rs`

These suites pass from the start: they extend coverage rather than drive
code. The mutation check at the end proves they bite.

- [ ] **Step 1: The standard join suite, per alias**

In `kermit/tests/join_tests.rs`, add `BinarySeek, GallopingSeek,
LinearSeek` to the `kermit_ds::{…}` import. Replace the four plain sorted
invocations (`define_multiway_join_test_suite!(TreeTrie, …)` ×2 and
`(ColumnTrie, …)` ×2) with:

```rust
// ── Layout aliases: seek strategy (sorted tries) ────────────────────────
type TreeTrieLinear = TreeTrie<LinearSeek>;
type TreeTrieBinary = TreeTrie<BinarySeek>;
type TreeTrieGalloping = TreeTrie<GallopingSeek>;
type ColumnTrieLinear = ColumnTrie<LinearSeek>;
type ColumnTrieBinary = ColumnTrie<BinarySeek>;
type ColumnTrieGalloping = ColumnTrie<GallopingSeek>;

define_multiway_join_test_suite!(TreeTrieLinear, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrieLinear, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(TreeTrieBinary, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrieBinary, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(TreeTrieGalloping, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrieGalloping, LeapfrogTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(ColumnTrieLinear, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(ColumnTrieLinear, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(ColumnTrieBinary, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(ColumnTrieBinary, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(ColumnTrieGalloping, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(ColumnTrieGalloping, LeapfrogTriejoin, CardinalityOptimiser);
```

Leave #84's `define_multiway_join_test_suite_for_build_mode!(ColumnTrie, …)` invocations alone. A build mode yields identical arrays under every strategy, so the build-mode alternate runs under the default seek only (spec, "Out of scope").

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests trie`
Expected: PASS. Sorted-trie tests now come to 12 invocations × 16 patterns = 192, plus #84's build-mode ones (`cargo test … -- --list | grep -c trie` to confirm).

- [ ] **Step 2: Allocation cells for the two new strategies**

In `kermit/tests/result_allocation.rs`, add `GallopingSeek, LinearSeek` to
the `kermit_ds::{…}` import. The existing sorted cells use plain `TreeTrie`
/ `ColumnTrie`, which is `binary`. At the end of the module doc, add:

```rust
//!
//! Issue #80 adds the seek strategies: the existing sorted cells run the
//! default (`binary`), and the `linear` / `galloping` cells below hold the
//! other two to the same standard: a strategy that allocated per seek would
//! show up in both checks. The scan cells are not multiplied, because the
//! scan never seeks.
```

After `column_trie_lftj_allocates_independently_of_result_size`, add:

```rust
#[test]
fn tree_trie_linear_lftj_allocates_independently_of_result_size() {
    lftj_allocations::<TreeTrie<LinearSeek>>(SMALL);
    assert_flat(
        "TreeTrie<LinearSeek>/LFTJ",
        lftj_allocations::<TreeTrie<LinearSeek>>(SMALL),
        lftj_allocations::<TreeTrie<LinearSeek>>(LARGE),
    );
}

#[test]
fn tree_trie_galloping_lftj_allocates_independently_of_result_size() {
    lftj_allocations::<TreeTrie<GallopingSeek>>(SMALL);
    assert_flat(
        "TreeTrie<GallopingSeek>/LFTJ",
        lftj_allocations::<TreeTrie<GallopingSeek>>(SMALL),
        lftj_allocations::<TreeTrie<GallopingSeek>>(LARGE),
    );
}

#[test]
fn column_trie_linear_lftj_allocates_independently_of_result_size() {
    lftj_allocations::<ColumnTrie<LinearSeek>>(SMALL);
    assert_flat(
        "ColumnTrie<LinearSeek>/LFTJ",
        lftj_allocations::<ColumnTrie<LinearSeek>>(SMALL),
        lftj_allocations::<ColumnTrie<LinearSeek>>(LARGE),
    );
}

#[test]
fn column_trie_galloping_lftj_allocates_independently_of_result_size() {
    lftj_allocations::<ColumnTrie<GallopingSeek>>(SMALL);
    assert_flat(
        "ColumnTrie<GallopingSeek>/LFTJ",
        lftj_allocations::<ColumnTrie<GallopingSeek>>(SMALL),
        lftj_allocations::<ColumnTrie<GallopingSeek>>(LARGE),
    );
}
```

After `column_trie_lftj_allocates_independently_of_descent_count`, add:

```rust
#[test]
fn tree_trie_linear_lftj_allocates_independently_of_descent_count() {
    lftj_descent_allocations::<TreeTrie<LinearSeek>>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "TreeTrie<LinearSeek>/LFTJ",
        lftj_descent_allocations::<TreeTrie<LinearSeek>>(FEW_DEAD_ENDS),
        lftj_descent_allocations::<TreeTrie<LinearSeek>>(MANY_DEAD_ENDS),
    );
}

#[test]
fn tree_trie_galloping_lftj_allocates_independently_of_descent_count() {
    lftj_descent_allocations::<TreeTrie<GallopingSeek>>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "TreeTrie<GallopingSeek>/LFTJ",
        lftj_descent_allocations::<TreeTrie<GallopingSeek>>(FEW_DEAD_ENDS),
        lftj_descent_allocations::<TreeTrie<GallopingSeek>>(MANY_DEAD_ENDS),
    );
}

#[test]
fn column_trie_linear_lftj_allocates_independently_of_descent_count() {
    lftj_descent_allocations::<ColumnTrie<LinearSeek>>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "ColumnTrie<LinearSeek>/LFTJ",
        lftj_descent_allocations::<ColumnTrie<LinearSeek>>(FEW_DEAD_ENDS),
        lftj_descent_allocations::<ColumnTrie<LinearSeek>>(MANY_DEAD_ENDS),
    );
}

#[test]
fn column_trie_galloping_lftj_allocates_independently_of_descent_count() {
    lftj_descent_allocations::<ColumnTrie<GallopingSeek>>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "ColumnTrie<GallopingSeek>/LFTJ",
        lftj_descent_allocations::<ColumnTrie<GallopingSeek>>(FEW_DEAD_ENDS),
        lftj_descent_allocations::<ColumnTrie<GallopingSeek>>(MANY_DEAD_ENDS),
    );
}
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test result_allocation`
Expected: PASS, 8 more tests than before.

- [ ] **Step 3: Real data under every strategy**

The 3–5-tuple fixtures never exercise a long seek. The real-data tests are
where a strategy bug that needs depth would surface (CLAUDE.md's
failed-descent lesson).

`kermit/tests/lubm_mini_oracle.rs`, which runs in CI without Java:
- **Imports.** `kermit_ds::{RelationFileExt, TreeTrie}` becomes
  `kermit_ds::{BinarySeek, Cardinality, GallopingSeek, LinearSeek,
  Relation, RelationFileExt, TreeTrie}`, and `kermit_iters::TrieIterable`
  is added.
- **`cardinality_mismatches`** becomes generic. It takes the relation
  type, loads through it, and names it in each mismatch:

```rust
/// Loads the generated relations as `R` into a fresh engine planned by
/// `optimiser`, runs every query, and returns one line per query whose
/// result count differs from the hand-counted cardinality.
fn cardinality_mismatches<R: TrieIterable + Relation + Cardinality>(
    bench: &BenchmarkDefinition, dir: &Path, optimiser: Optimiser, expected: &HashMap<&str, u64>,
) -> Vec<String> {
    let planner = optimiser.instantiate();
    let mut relations: BTreeMap<String, R> = BTreeMap::new();
    for rel in &bench.relations {
        let path = dir.join(format!("{}.parquet", rel.name));
        let trie = R::from_parquet(&path)
            .unwrap_or_else(|e| panic!("failed to load relation {path:?}: {e}"));
        relations.insert(rel.name.clone(), trie);
    }

    let mut mismatches = Vec::new();
    for q in &bench.queries {
        let want = expected[q.name.as_str()];
        let parsed: JoinQuery = q.query.parse().expect("datalog parse failure");
        let got = lftj_join::<R, LeapfrogTriejoin>(&relations, parsed, planner.as_ref())
            .unwrap_or_else(|e| panic!("query {}: {e}", q.name))
            .len() as u64;
        if got != want {
            mismatches.push(format!(
                "  [{} / {}] {}: got {got}, expected {want}\n    query: {}",
                optimiser.axis_value(),
                std::any::type_name::<R>(),
                q.name,
                q.query
            ));
        }
    }
    mismatches
}
```

and in `mini_lubm_abox_query_cardinalities_match_hand_derivation`, the
loop body becomes:

```rust
    for &optimiser in optimisers {
        // Every seek strategy must give every answer: the strategy changes
        // how far a seek looks, never where it lands (issue #80).
        mismatches.extend(cardinality_mismatches::<TreeTrie<LinearSeek>>(
            &bench,
            out.path(),
            optimiser,
            &expected,
        ));
        mismatches.extend(cardinality_mismatches::<TreeTrie<BinarySeek>>(
            &bench,
            out.path(),
            optimiser,
            &expected,
        ));
        mismatches.extend(cardinality_mismatches::<TreeTrie<GallopingSeek>>(
            &bench,
            out.path(),
            optimiser,
            &expected,
        ));
    }
```

and its assertion message reads
`"mini LUBM cardinality mismatches ({} across {} queries x {} optimisers x 3 seek strategies):\n{}"`.

`kermit/tests/watdiv_correctness.rs`:
- Import `BinarySeek, Cardinality, GallopingSeek, LinearSeek, Relation`
  and `kermit_iters::TrieIterable`.
- Move the body of `watdiv_mini_cardinalities_match` into `fn
  check_cardinalities<R: TrieIterable + Relation + Cardinality>()`. In it,
  the map is `BTreeMap<String, R>`, loading uses `R::from_parquet`, the
  join is `lftj_join::<R, LeapfrogTriejoin>`, and the mismatch message
  adds `std::any::type_name::<R>()`.
- The test then becomes:

```rust
#[test]
fn watdiv_mini_cardinalities_match() {
    check_cardinalities::<TreeTrie<LinearSeek>>();
    check_cardinalities::<TreeTrie<BinarySeek>>();
    check_cardinalities::<TreeTrie<GallopingSeek>>();
}
```

`kermit/tests/lubm_cardinalities.rs` needs Java, so run it inside `nix
develop`; it skips silently without Java.
- Make `cardinality_mismatches` generic over `R: TrieIterable + Relation +
  Cardinality` in the same way. It loads through `R::from_parquet` and
  joins with `lftj_join::<R, LeapfrogTriejoin>`, and its message prefix
  becomes `"  [{optimiser_name} / {}] …"` with `std::any::type_name::<R>()`.
- It currently takes a `Box<dyn QueryOptimiser>` by value, which cannot be
  reused across three calls, so change that parameter to `optimiser: &dyn
  QueryOptimiser`.
- The loop becomes:

```rust
    for (name, optimiser) in &optimisers {
        mismatches.extend(cardinality_mismatches::<TreeTrie<LinearSeek>>(
            &bench,
            out.path(),
            name,
            optimiser.as_ref(),
            &expected,
        ));
        mismatches.extend(cardinality_mismatches::<TreeTrie<BinarySeek>>(
            &bench,
            out.path(),
            name,
            optimiser.as_ref(),
            &expected,
        ));
        mismatches.extend(cardinality_mismatches::<TreeTrie<GallopingSeek>>(
            &bench,
            out.path(),
            name,
            optimiser.as_ref(),
            &expected,
        ));
    }
```

(`let optimiser_count = optimisers.len();` stays above the loop.) Inside
`cardinality_mismatches`, the join call passes `optimiser` directly. The
assertion message gains `x 3 seek strategies`.

Run:

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test lubm_mini_oracle --test watdiv_correctness
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test lubm_cardinalities -- --nocapture
```

Expected: PASS. `lubm_cardinalities` runs (it does not print `skipping`) because the flake provides JDK 8. It takes about 3x its former time.

- [ ] **Step 4: Lint and commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit --all-targets -- -D warnings
git -C $WT add kermit/tests
git -C $WT commit -F - <<'EOF'
test(kermit): every seek strategy through the join, allocation and real-data suites (#80)

The standard join suite runs one alias per sorted trie x seek strategy
under both optimisers (12 invocations, 192 tests, replacing 4 / 64).
result_allocation gains linear and galloping cells for the result-size
and descent-count checks. The LUBM mini oracle, the WatDiv mini fixture
and the LUBM(1, 0) reference test load TreeTrie under all three
strategies: the small fixtures never seek far, so real data is where a
strategy bug that needs depth would surface.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FuWsPhYU2JAAEJ2vyChiYr
EOF
```

- [ ] **Step 5: Mutation check: which suites see a bug only depth reaches**

This check is evidence for the spec's claim that the small join fixtures
cannot see a strategy bug that needs depth. The mutant breaks galloping
only on brackets of 64 or more, i.e. seeks across more than 32 siblings.
In `kermit-ds/src/seek.rs`, `GallopingSeek`, change
`let hi = bound.min(remaining.len());` to:

```rust
        let hi = if bound >= 64 { bound.min(remaining.len()) - 1 } else { bound.min(remaining.len()) };
```

Run each command and record PASS or FAIL:

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds seek::
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --test trie_tests galloping
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests galloping
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test lubm_mini_oracle --test watdiv_correctness
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test lubm_cardinalities
```

Expected:
- `seek::` FAILS: the randomised agreement test, whose past-the-end
  targets on slices longer than 32 always reach the mutant.
- `join_tests galloping` PASSES. Its fixtures never seek that far, which
  is the reason for the strategy-level and real-data tests.
- Record the others either way. If `lubm_cardinalities` (real LUBM(1, 0)
  data) also passes, say so in the report: then only the strategy-level
  tests guard long gallops.

Reverse the edit, re-run `seek::` and see it pass. `git -C $WT status` is clean.

---

## Task 6 [controller]: Gate, measurement 1, checkpoint 2

- [ ] **Step 1: The workspace gate**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace
CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings nix develop $WT --command cargo clippy --workspace --all-targets
CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings nix develop $WT --command cargo doc --workspace --no-deps
nix develop $WT --command cargo fmt --all --check
CARGO_BUILD_JOBS=2 MIRIFLAGS=-Zmiri-disable-isolation nix develop $WT --command cargo miri test -p kermit-ds -p kermit-algos -p kermit-derive
```

Expected: all green. Under miri, the trybuild and wall-clock tests are
ignored, and the strategy tests run at their reduced sizes.

- [ ] **Step 2: Build the new binary**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build --release -p kermit
```

- [ ] **Step 3: Measurement 1: the default is unchanged**

All runs happen outside the repo, one at a time, and never alongside
cargo. Use the sweep's Criterion settings for iteration.

```bash
RUN=/tb/Source/Academia/kermit-bench-runs/seek-ab-$(date +%F)
mkdir -p $RUN/reports
BASE_BIN=$SCRATCH/kermit-base/target/release/kermit
NEW_BIN=$WT/target/release/kermit
CRIT="--sample-size 20 --measurement-time 2 --warm-up-time 1"
for r in 1 2 3 4 5; do
  if [ $((r % 2)) = 1 ]; then order="base new"; else order="new base"; fi
  for arm in $order; do
    if [ $arm = base ]; then bin=$BASE_BIN; else bin=$NEW_BIN; fi
    for q in q0236 q0167 q0056; do
      env -C $RUN KERMIT_WORKSPACE=$WT $bin bench $CRIT --name ab-$arm-r$r \
        --report-json $RUN/reports/$arm-r$r-$q.json \
        run watdiv-stress-100-test-1-prelim -q $q -i all -a all -m iteration --verify
    done
    env -C $RUN KERMIT_WORKSPACE=$WT $bin bench $CRIT --name ab-$arm-r$r \
      --report-json $RUN/reports/$arm-r$r-lubm.json \
      run lubm-reference -i all -a all -m iteration --verify
  done
done
```

If the loop will exceed 10 minutes (it likely will), write it to `$RUN/ab.sh`
and run it fully detached with `setsid nohup bash $RUN/ab.sh >$RUN/ab.log
2>&1 & disown`. Then poll `$RUN/ab.log`. Write `$RUN/README.md` with both
SHAs (`cat $SCRATCH/base.sha` and `git -C $WT rev-parse HEAD`), the
command, and the date.

Analyse it with kermit-lab:

```bash
uv --directory $WT/python/kermit-lab run python - <<EOF
import pathlib, re
import pandas as pd
import kermit_lab as kl
run = pathlib.Path("$RUN")
frames = []
for path in sorted((run / "reports").glob("*.json")):
    arm, replicate = re.match(r"(base|new)-r(\d)-", path.name).groups()
    df = kl.load(path, criterion_root=run / "target" / "criterion")
    frames.append(df.assign(arm=arm, replicate=int(replicate)))
df = pd.concat(frames)
it = df[df["phase"] == "iteration"]
stats = it.groupby(["query", "data_structure", "arm"])["mean_ns"].agg(["median", "min", "max"])
wide = stats.unstack("arm")
wide["ratio_new_over_base"] = wide[("median", "new")] / wide[("median", "base")]
wide["base_spread_lo"] = wide[("min", "base")] / wide[("median", "base")]
wide["base_spread_hi"] = wide[("max", "base")] / wide[("median", "base")]
print(wide.to_string())
EOF
```

Each report file is loaded alone and tagged by its name, so the arm and
replicate never depend on a frame column.

Pass criteria:

- every `--verify` check passed in both arms;
- for each query and structure, `ratio_new_over_base` lies inside the
  baseline's own replicate spread, `[min/median, max/median]` of the
  `base` arm;
- HashTrie (the control) shows the same ratio band as the sorted tries.

If both sorted tries shift beyond that band in the same direction while
the control does not, stop. Report it to the supervisor; do not land.

- [ ] **Step 4: Supervisor checkpoint 2**

Send the supervisor:

- the P1 and P2 commit SHAs;
- the gate results;
- each mutation-check outcome;
- the measurement-1 table (path to `$RUN`);
- any deviation from this plan.

Wait for the go-ahead before dispatching P3.

---

# P3 — analysis and docs

## Task 7 [P3]: kermit-lab back-fill and per-axis applicable phases

**Files:**
- Modify: `python/kermit-lab/kermit_lab/defaults.py`
- Modify: `python/kermit-lab/kermit_lab/presets.py`
- Modify: `python/kermit-lab/tests/test_defaults.py`, `tests/conftest.py`, `tests/test_presets.py`, `tests/test_render_all.py`, `tests/test_contract.py`

- [ ] **Step 1: Write the failing tests**

In `tests/test_defaults.py` (whose import #84 made `from kermit_lab.defaults import AXIS_DEFAULTS, SCOPED_AXIS_DEFAULTS, apply_axis_defaults`), append:

```python
def test_seek_backfills_column_trie_rows_only() -> None:
    df = pd.DataFrame({
        "data_structure": ["ColumnTrie", "ColumnTrie", "TreeTrie", "HashTrie"],
        "ds_layout_seek": [pd.NA, "galloping", pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["ds_layout_seek"].iloc[0] == "binary"
    assert out["ds_layout_seek"].iloc[1] == "galloping"
    # TreeTrie's seek was linear before #67 and binary after it, and a
    # report cannot tell which; HashTrie has no seek. Both stay unlabelled.
    assert out["ds_layout_seek"].iloc[2:].isna().all()
    assert SCOPED_AXIS_DEFAULTS[("ds_layout_seek", "ColumnTrie")] == "binary"
    assert ("ds_layout_seek", "TreeTrie") not in SCOPED_AXIS_DEFAULTS
    assert "ds_layout_seek" not in AXIS_DEFAULTS
```

Append to `tests/conftest.py`. It reuses the module's `_FunctionSpec`,
`_write_function_dir` and `_write_report`, as #84's
`fixture_build_mode_tree` does:

```python
@pytest.fixture
def fixture_seek_tree(tmp_path: Path) -> dict:
    """TreeTrie reports under two seek strategies, for the seek ablation
    guard (#80).

    Each report times insertion and iteration (plus space), so the seek
    axis has two values. It applies to iteration but not insertion: both
    strategies build the same trie, so the insertion points are equal.
    """
    criterion_root = tmp_path / "target" / "criterion"
    reports_dir = tmp_path / "reports"
    criterion_root.mkdir(parents=True)
    reports_dir.mkdir()

    paths: list[Path] = []
    for seek, iteration_point in (("binary", 100.0), ("galloping", 80.0)):
        groups: list[tuple[str, str, str]] = []
        for phase, point in (("insertion", 300.0), ("iteration", iteration_point)):
            function = f"TreeTrie/{seek}/{phase}"
            samples = [(i + 1, point * (i + 1)) for i in range(10)]
            _write_function_dir(
                criterion_root, _FunctionSpec("run", function, "time", point, samples)
            )
            groups.append(("run", function, "time"))
        space_function = f"TreeTrie/{seek}/space"
        space_samples = [(i + 1, 6400.0 * (i + 1)) for i in range(10)]
        _write_function_dir(
            criterion_root,
            _FunctionSpec("run", space_function, "space", 6400.0, space_samples),
        )
        groups.append(("run", space_function, "space"))
        axes = {
            "benchmark": "triangle",
            "query": "triangle",
            "data_structure": "TreeTrie",
            "algorithm": "LeapfrogTriejoin",
            "tuples": 100,
            "ds_layout_seek": seek,
        }
        paths.append(
            _write_report(
                reports_dir, f"run-TreeTrie-{seek}", kind="run", axes=axes,
                metadata=[], groups=groups,
            )
        )
    return {
        "criterion_root": criterion_root,
        "reports_dir": reports_dir,
        "paths": sorted(paths),
    }
```

Append to `tests/test_render_all.py` (its imports gained `pytest`, `kl`,
`presets` and `InsufficientAxesError` with #84):

```python
def test_seek_ablation_is_drawn_only_for_search_phases(
    fixture_seek_tree, tmp_path: Path
) -> None:
    """Both seek strategies build the same trie, so an insertion-time "seek
    ablation" would chart noise; iteration is where the strategy acts."""
    reports = load_reports(fixture_seek_tree["paths"])
    for phase, drawn in (("iteration", True), ("insertion", False)):
        out = tmp_path / phase
        out.mkdir()
        render_all(reports, out, fixture_seek_tree["criterion_root"], "pdf", phase=phase)
        names = {p.name for p in out.iterdir()}
        assert ("ablation-ds_layout_seek.pdf" in names) is drawn, (phase, names)
```

Append to `tests/test_presets.py` (add `import pytest`, `import kermit_lab
as kl` and `from kermit_lab.plots_errors import InsufficientAxesError` if
absent):

```python
def test_ablation_refuses_seek_outside_search_phases(fixture_seek_tree) -> None:
    df = kl.load(
        fixture_seek_tree["paths"], criterion_root=fixture_seek_tree["criterion_root"]
    )
    assert set(df["ds_layout_seek"].dropna()) == {"binary", "galloping"}
    with pytest.raises(InsufficientAxesError, match="searched"):
        presets.ablation(df, axis="ds_layout_seek", phase="insertion")
    assert isinstance(
        presets.ablation(df, axis="ds_layout_seek", phase="iteration"), Figure
    )
```

Append to `tests/test_contract.py`:

```python
def test_bench_join_reports_its_seek_strategy(tmp_path: Path) -> None:
    report = tmp_path / "join.json"
    _run(
        tmp_path, report,
        "join", "--relations", str(FIXTURES / "first.csv"), str(FIXTURES / "second.csv"),
        "--query", str(FIXTURES / "intersect_query.dl"),
        "-i", "tree-trie", "-a", "leapfrog-triejoin", "-m", "space",
        "--ds-layout-seek", "galloping",
    )
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion")
    assert len(df) >= 1
    assert set(df["ds_layout_seek"]) == {"galloping"}
```

On a cold environment, first run `uv --directory $WT/python/kermit-lab sync --group test`.

Run: `systemd-run --user --scope -p MemoryMax=3G -p MemorySwapMax=0 uv --directory $WT/python/kermit-lab run pytest tests/test_defaults.py tests/test_presets.py tests/test_render_all.py`
Expected:
- `test_seek_backfills_column_trie_rows_only` FAILS: `KeyError ('ds_layout_seek', 'ColumnTrie')`.
- `test_seek_ablation_is_drawn_only_for_search_phases` FAILS: insertion draws it.
- `test_ablation_refuses_seek_outside_search_phases` FAILS: nothing raised.

- [ ] **Step 2: The scoped back-fill**

In `kermit_lab/defaults.py`, add to `SCOPED_AXIS_DEFAULTS`:

```python
    # ColumnTrie's seek has been a binary search (`partition_point`) since
    # 525c99f (2026-03-02), before the first JSON report writer (f76344d,
    # 2026-04-26), so every ColumnTrie report without the axis ran `binary`.
    # TreeTrie has no entry: its seek was linear until 9604293 (#67) and
    # binary after, and a report cannot tell which side it came from (#80).
    ("ds_layout_seek", "ColumnTrie"): "binary",
```

- [ ] **Step 3: One map from axis to applicable phases**

In `kermit_lab/presets.py`, replace #84's `_BUILD_ONLY_AXES` /
`_BUILD_PHASES` block and its comment. Add `from dataclasses import
dataclass` to the imports if it is absent.

```python
@dataclass(frozen=True)
class AxisScope:
    """The time phases an optimisation axis can affect, and why."""

    phases: frozenset[str]
    reason: str


# The time phases an optimisation axis can affect; an axis absent from this
# map can affect every phase. `ablation` refuses an axis on a phase outside
# its scope: two values there time the same code, so any difference between
# them is noise or binary drift, not the optimisation. Give a new axis an
# entry when its effect is confined to some phases.
AXIS_PHASES: dict[str, AxisScope] = {
    # A BuildMode changes how a structure is built, never the structure (#84).
    "ds_build_mode": AxisScope(
        frozenset({"insertion", "end_to_end"}),
        "it changes only how a structure is built",
    ),
    # A seek strategy changes how a built trie is searched; no build calls
    # seek (#80).
    "ds_layout_seek": AxisScope(
        frozenset({"iteration", "end_to_end"}),
        "it changes only how a built trie is searched",
    ),
}
```

and replace the guard at the top of `ablation` (keep the rest of the body)
with:

```python
    """Ablation: time vs an optimization axis, coloured by DS, faceted by query when >1.

    Raises :class:`InsufficientAxesError` for an axis on a phase outside its
    :data:`AXIS_PHASES` scope.
    """
    scope = AXIS_PHASES.get(axis)
    if scope is not None and phase not in scope.phases:
        allowed = " or ".join(repr(p) for p in sorted(scope.phases))
        raise InsufficientAxesError(
            f"{axis} cannot affect phase {phase!r}: {scope.reason}; plot it on {allowed}"
        )
```

#84's message test (`match="built"`) still passes: the build-mode reason
contains "built".

- [ ] **Step 4: Run the suite, contract test included**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit
systemd-run --user --scope -p MemoryMax=3G -p MemorySwapMax=0 env KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest
```

Expected: all pass, the four new tests and #84's build-mode tests among them. With `KERMIT_BIN` set, no test is skipped.

- [ ] **Step 5: Commit**

```bash
git -C $WT add python/kermit-lab
git -C $WT commit -F - <<'EOF'
feat(kermit-lab): ds_layout_seek back-fill and per-axis applicable phases (#80)

ColumnTrie rows without ds_layout_seek are back-filled "binary": its
seek has been partition_point since before the first JSON report.
TreeTrie rows stay unlabelled, because its seek was linear before #67
and nothing in a report says which side of #67 it came from.

The ablation guard becomes one map from axis to the phases it can
affect, replacing the build-mode special case: ds_build_mode explains
insertion and end_to_end, ds_layout_seek explains iteration and
end_to_end. A seek ablation is therefore never drawn on insertion,
where both strategies build the same trie.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FuWsPhYU2JAAEJ2vyChiYr
EOF
```

---

## Task 8 [P3]: Document the seek-strategy Layout

**Files:**
- Create: `docs/data-structures/seek-strategies.md`
- Modify: `docs/data-structures/tree-trie.md`, `docs/data-structures/column-trie.md`
- Modify: `docs/specs/optimization-standard.md`, `docs/specs/bench-report-schema.md`
- Modify: `CLAUDE.md`, `ARCHITECTURE.md`, `BENCHMARKING.md`, `USAGE.md`

#84 also edited several of these files. Where an anchor below has moved,
put the text at the equivalent place and report the deviation.

- [ ] **Step 1: `docs/data-structures/seek-strategies.md`**

Create:

````markdown
# Seek strategies (`TreeTrie<S>`, `ColumnTrie<S>`)

Both sorted tries take their seek algorithm as a Layout type parameter,
`S: SeekStrategy` (`kermit-ds/src/seek.rs`). Each strategy has one
implementation, shared by both tries, so the same strategy on both tries
isolates the layout, and one trie under three strategies isolates the
seek (issue #80).

| Strategy | `--ds-layout-seek` | `ds_layout_seek` | Default |
|---|---|---|---|
| `LinearSeek` | `linear` | `"linear"` | |
| `BinarySeek` | `binary` | `"binary"` | ✓ |
| `GallopingSeek` | `galloping` | `"galloping"` | |

## Contract

`seek(target)` moves the iterator to the least upper bound of `target`
among the siblings it has not yet passed. Its offset is the number of
remaining siblings whose key is below `target`, which is exactly what
`slice::partition_point` returns. A strategy is one function:

```rust
fn partition_point<T, P: FnMut(&T) -> bool>(remaining: &[T], below: P) -> usize;
```

Every strategy must return what `remaining.partition_point(below)`
returns, and the strategies differ only in which elements they probe. An
iterator's state after a seek is therefore identical under every strategy.

## What a strategy may not touch

The strategy computes the offset and nothing else. Around it, each
iterator keeps the logic LFTJ depends on:

- `TreeTrieIter` keeps its `at_end` guard and backward-seek panic, and
  moves the stack top only when the seek lands on a key. Off the end, the
  stack top stays on the last positioned node, so `open` after `at_end`
  still descends from it (see `tree-trie.md`).
- `ColumnTrieIter` keeps its `at_end` guard and advances `slice_offset`
  by the offset.

## The three strategies

```text
linear(remaining, below):
    i = 0
    while i < len and below(remaining[i]): i += 1
    return i

binary(remaining, below):
    return partition_point(remaining, below)      // std's binary search

galloping(remaining, below):
    if len == 0 or not below(remaining[0]): return 0
    bound = 1
    while bound < len and below(remaining[bound]): bound *= 2
    lo = bound / 2 + 1;  hi = min(bound, len)      // answer in (bound/2, hi]
    return lo + partition_point(remaining[lo..hi], below)
```

| Strategy | Probes for a seek that moves `d` of `n` remaining | Best when |
|---|---|---|
| linear | `min(d + 1, n)` | seeks move one or two siblings |
| binary | `⌈log₂ n⌉ + 1` | seeks jump far, uniformly |
| galloping | 1 if `d = 0`, at most `2⌈log₂ d⌉ + 2` | both: O(1) short seeks, O(log d) long ones |

Veldhuizen's LFTJ analysis assumes a seek costs O(log N), amortised
O(1 + log(N/m)) over m visited keys. Binary meets the first bound;
galloping meets both. `seek.rs` pins every bound in this table by
counting probes (`probe_counts_match_each_strategys_bound`).

## Worked micro-example

Take sixteen siblings with keys `2, 4, 6, …, 32`, and the iterator on the
first (key 2).

| Seek | Lands on | `d` | linear probes | binary probes | galloping probes |
|---|---|---|---|---|---|
| `seek(4)` | 4 (offset 1) | 1 | 2: keys 2, 4 | 5 | 2: offsets 0, 1 |
| `seek(27)` | 28 (offset 13) | 13 | 14: keys 2 … 28 | 5 | 9: offsets 0, 1, 2, 4, 8, then 4 inside offsets 9–15 |

With a thousand siblings, binary costs 11 probes for both seeks, while
linear and galloping cost what they cost here. That is the trade-off issue
#67 exposed. Binary removed TreeTrie's 10–197x losses on high-fan-out
WatDiv queries, but it made queries dominated by one-step seeks slower
(q0236 ~1.9x, q0167 ~1.35x; indicative).

## Why a Layout

A runtime switch would add a branch to every seek, a tax on runs that
never change strategy (the shape/value rule in
[`optimization-standard.md`](../specs/optimization-standard.md)). As a
type parameter, the strategy is monomorphised: `TreeTrie` and
`ColumnTrie` default to `BinarySeek` and compile to the `partition_point`
call they made before the parameter existed. The trie gains only a
zero-sized `PhantomData<S>`, which `seek_strategy_adds_no_state` pins.

## Measuring

- **Compare within one binary.** Compare strategies inside one binary
  (`--ds-layout-seek`), with a distinct `--name` per run. Reports carry no
  binary identity, so a cross-build comparison cannot be recovered later.
- **`bench ds` rejects the flag.** None of its metrics calls `seek`.
- **kermit-lab.** kermit-lab back-fills a missing `ds_layout_seek` on
  ColumnTrie rows as `binary`, which is true of every report ever written.
  It leaves TreeTrie rows unlabelled, because TreeTrie's seek was linear
  before 9604293 (#67).
- **Ablation phases.** `kl.ablation(df, axis="ds_layout_seek")` draws only
  for `iteration` and `end_to_end`.
````

- [ ] **Step 2: `tree-trie.md` and `column-trie.md`**

In `docs/data-structures/tree-trie.md`, replace the complexity row
`| TrieIterator::seek(target) | O(log b) | | partition_point binary search over the remaining siblings |` with:

```markdown
| `TrieIterator::seek(target)` | `S`-dependent: linear O(d), binary O(log r), galloping O(log d) | | `S::partition_point` over the `r` remaining siblings; `d` is the distance moved. See [seek strategies](seek-strategies.md) |
```

and replace the two paragraphs after the table (the one beginning "`seek` binary-searches the siblings it has not yet passed" and the one beginning "Until issue #67") with:

```markdown
`seek` asks its seek strategy `S` how many of the remaining siblings lie below the target, and moves that far. The strategy is a Layout shared with `ColumnTrie`, so the two sorted tries differ only in layout under any one strategy. The default, `binary`, is the `partition_point` search both tries used before the parameter existed. See [seek strategies](seek-strategies.md) for the three strategies, their probe bounds and the LFTJ bound they relate to.

Until issue #67, `seek` was a linear scan. On high-fan-out WatDiv queries it made `TreeTrie` 10–197x slower than `ColumnTrie`, and `TreeTrie` numbers from before that fix are not comparable with later ones. `--ds-layout-seek linear` runs the same algorithm in today's code, not the pre-#67 code. `seek_cost_matches_the_strategy` in `trie_seek_tests!` ([`kermit-ds/tests/common/macros.rs`](../../kermit-ds/tests/common/macros.rs)) pins each strategy's complexity through the real iterator.
```

Before "## See also", add:

```markdown
## Optimizations

| Dimension | Category | Axis | Flag | Default | Test aliases |
|---|---|---|---|---|---|
| Seek strategy | Layout (`S: SeekStrategy`) | `ds_layout_seek` | `--ds-layout-seek linear\|binary\|galloping` | `binary` | `TreeTrieLinear`, `TreeTrieBinary`, `TreeTrieGalloping` |

The strategy changes only how `seek` searches; it changes no stored data, no build and no `heap_size_bytes`. Details: [seek strategies](seek-strategies.md).
```

In `docs/data-structures/column-trie.md`:
- Change the invariant bullet "**Forward-only `seek`.** … it uses
  `partition_point` on the remaining slice." to end "… it asks its seek
  strategy `S` for the offset within the remaining slice (see [seek
  strategies](seek-strategies.md))."
- Replace the `seek` complexity row with the same `S`-dependent row as
  `tree-trie.md`, but say "remaining keys of the interval slice".
- In its "Optimizations" section (#84 added one for the build mode), add
  the row:

```markdown
| Seek strategy | Layout (`S: SeekStrategy`) | `ds_layout_seek` | `--ds-layout-seek linear\|binary\|galloping` | `binary` | `ColumnTrieLinear`, `ColumnTrieBinary`, `ColumnTrieGalloping` |
```

If #84's section is prose rather than a table, add a paragraph with the
same facts.

- [ ] **Step 3: `optimization-standard.md` and `bench-report-schema.md`**

`docs/specs/optimization-standard.md`:
- **Layout section.** After the "Second concrete example" block quote,
  add:

```markdown
> **Third concrete example.** The sorted tries' seek strategy is a Layout of two structures at once: `TreeTrie<S>` and `ColumnTrie<S>` share one implementation per `S: SeekStrategy` (`linear`, `binary`, `galloping`), so the same strategy on both tries isolates the layout. Every strategy returns what `slice::partition_point` returns; only the probes differ.
```

- **Layout table.** The "Examples (potential)" cell becomes `Hasher choice ✓, singleton pruning ✓, seek strategy ✓, pointer encoding, lazy expansion`.
- **Walkthrough step 3.** After "**Adding a third Layout dimension means
  adding arms to that macro and nowhere else**", add: "That rule is per
  structure family. The sorted tries' seek strategy has its own
  `with_sorted_trie_layout!` beside it, because the two products share no
  dimension."
- **Walkthrough step 4.** "the full 11-pattern suite" becomes "the full
  16-pattern suite".
- **"Where to look".** Add the rows `| Third Layout consumer (seek
  strategy, both sorted tries) |
  [`kermit-ds/src/seek.rs`](../../kermit-ds/src/seek.rs) |` and `| Sorted
  Layout dispatch |
  [`kermit/src/options.rs`](../../kermit/src/options.rs)
  (`with_sorted_trie_layout!`) |`.
- **"What's implemented today".** Add the row `| Seek strategy (linear /
  binary / galloping) | Layout | `ds_layout_seek` | (kermit-specific; LFTJ
  §3) |`, and fix the count sentence above the table to match.
- **FAQ "Does adding a new axis break existing bench reports?".** Add:
  "Some back-fills are true only for one structure. `ds_layout_seek` is
  back-filled `binary` on ColumnTrie rows only, through kermit-lab's
  structure-scoped registry."

`docs/specs/bench-report-schema.md`, "Standard axis prefixes":
- The `ds_layout_<dim>` example list gains `ds_layout_seek`.
- Append this bullet to the "When pivoting" list:

```markdown
- `ds_layout_seek` (sorted tries only) is back-filled `"binary"` on
  `ColumnTrie` rows only. `TreeTrie` rows stay missing: its seek was
  linear before 9604293 (#67) and binary after, and a report cannot tell
  which. `HashTrie` has no seek.
```

- [ ] **Step 4: `CLAUDE.md`, `ARCHITECTURE.md`, `BENCHMARKING.md`, `USAGE.md`**

`CLAUDE.md`:
- **Priorities item 1.** After the HashTrie aliases sentence, add: "For
  each sorted trie it is the seek strategy: `TreeTrieLinear` /
  `TreeTrieBinary` / `TreeTrieGalloping` and the three `ColumnTrie…`
  aliases, each through the 16 patterns under LFTJ and both optimisers."
- **Workspace Architecture, `kermit-ds` entry.** Append "TreeTrie and
  ColumnTrie are generic over a `SeekStrategy` Layout (`seek.rs`)."
- **Key Trait Hierarchy.** Add the bullet "**SeekStrategy**: the sorted
  tries' seek Layout (`kermit-ds/src/seek.rs`): `partition_point(remaining,
  below)`, which every strategy must answer exactly as
  `slice::partition_point` does. `LinearSeek`, `BinarySeek` (default) and
  `GallopingSeek`, axis `ds_layout_seek`."
- **Index-structure recipe step 3.** Append "The derive accepts type
  parameters after `'a` (e.g. a Layout such as `S: SeekStrategy`); declare
  their bounds on the struct."
- **Optimisation recipe step 4.** "the `with_hash_trie_layout!` product
  macro" becomes "the `with_hash_trie_layout!` / `with_sorted_trie_layout!`
  product macros".
- **Gotcha "`bench run` sweeps are cells, not pairs".** The variant list
  becomes `TrieLftj(SortedTrie::TreeTrie { seek } | ColumnTrie { seek,
  build })`.
- **New gotcha**, after "Singleton pruning is a Layout, not a Config":

```markdown
- **Defaulted Layout parameters need annotated bindings**: `TreeTrie<S = BinarySeek>` (like `HashTrie<H, P>`) is no inference fallback in expression position, so `let t = TreeTrie::from_tuples(…); t.trie_iter()` fails with E0283. Annotate the binding (`let t: TreeTrie = …`) or use an alias; test macros take aliases. `bench ds` rejects `--ds-layout-seek` (no metric of it seeks), and kermit-lab draws a seek ablation only for `iteration` / `end_to_end` (`AXIS_PHASES` in `presets.py`).
```

`ARCHITECTURE.md`: in the HashTrie paragraph on optimisation axes (#84 may
already have reworded it), state that `TreeTrie<S>` and `ColumnTrie<S>`
implement `HasOptimizationAxes` too, reporting `ds_layout_seek`. Wherever
the `Execution` variants are listed, update them to the shape above.

`BENCHMARKING.md`, in "Ablation: measuring an optimization", after the
hasher example and its Python block, add:

````markdown
The seek strategy of the sorted tries works the same way. Compare
strategies **within one binary**, and give each run a distinct `--name`,
because Criterion group names do not encode axes:

```sh
for seek in linear binary galloping; do
  kermit bench --name seek-$seek --report-json bench-runs/seek-$seek.json \
    run watdiv-stress-100-test-1-prelim -q q0236 -i all -a leapfrog-triejoin \
    --ds-layout-seek $seek --metrics iteration
done
```

`kl.ablation(df, axis="ds_layout_seek")` draws only for `iteration` and
`end_to_end`: both strategies build the same trie.
````

`USAGE.md`:
- In the `bench join` paragraph, "accepts `--ds-config` alongside the
  `--ds-layout-*` flags" becomes "accepts `--ds-config` and `--ds-build`
  alongside the `--ds-layout-*` flags (including `--ds-layout-seek
  linear|binary|galloping` for the sorted tries)".
- In the `bench ds` section, add: "`--ds-layout-seek` is rejected here:
  none of `bench ds`'s metrics calls `seek`."
- In the `kermit join` section, if it lists the layout flags, add
  `--ds-layout-seek`.

- [ ] **Step 5: Verify and commit**

```bash
CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings nix develop $WT --command cargo doc --workspace --no-deps
git -C $WT add docs CLAUDE.md ARCHITECTURE.md BENCHMARKING.md USAGE.md
git -C $WT commit -F - <<'EOF'
docs: the sorted tries' seek-strategy Layout (#80)

A shared seek-strategies.md (contract, pseudocode, probe bounds, a
worked example, why a Layout), Optimizations sections and seek rows in
tree-trie.md and column-trie.md, the third Layout consumer in the
optimisation standard, the ds_layout_seek axis and its scoped back-fill
in the report schema, and the matching CLAUDE.md, ARCHITECTURE.md,
BENCHMARKING.md and USAGE.md updates.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FuWsPhYU2JAAEJ2vyChiYr
EOF
```

---

## Task 9 [controller]: Final gate, measurement 2, checkpoint 3

- [ ] **Step 1: The full gate**

Re-run Task 6 Step 1's five commands, then the kermit-lab suite with
`KERMIT_BIN` set (Task 7 Step 4). Expected: all green.

- [ ] **Step 2: Derive the probe set**

The 31 queries are #67's: those where TreeTrie took ≥10x ColumnTrie's
iteration time in the 2026-09-10 prelim.

```bash
PRELIM=/tb/Source/Academia/kermit-bench-runs/watdiv-stress-100-prelim-2026-09-10
uv --directory $WT/python/kermit-lab run python - <<EOF
import pathlib, kermit_lab as kl
prelim = pathlib.Path("$PRELIM")
df = kl.load(sorted((prelim / "reports").glob("*.json")), criterion_root=prelim / "criterion")
it = df[(df["phase"] == "iteration") & (df["algorithm"] == "LeapfrogTriejoin")]
pivot = it.pivot_table(index="query", columns="data_structure", values="mean_ns")
slow = sorted(pivot.index[pivot["TreeTrie"] >= 10 * pivot["ColumnTrie"]])
print(len(slow))
probes = sorted(set(slow) | {"q0236", "q0167", "q0056"})
pathlib.Path("$SCRATCH/probe-queries.txt").write_text(" ".join(probes) + "\n")
EOF
cat $SCRATCH/probe-queries.txt
```

Expected: the script prints 31, and the file lists 34 queries (the 31
plus q0236, q0167 and q0056). If the count differs, say so; don't adjust
the threshold to force 31.

- [ ] **Step 3: Measurement 2: the strategy comparison**

The scope assumes open question 3 was answered with "probe set".

```bash
RUN=/tb/Source/Academia/kermit-bench-runs/seek-strategies-$(date +%F)
mkdir -p $RUN/reports
BIN=$WT/target/release/kermit
CRIT="--sample-size 20 --measurement-time 2 --warm-up-time 1"
QUERIES="$(cat $SCRATCH/probe-queries.txt)"
cp $SCRATCH/probe-queries.txt $RUN/
ORDERS=("linear binary galloping" "binary galloping linear" "galloping linear binary" "linear binary galloping" "binary galloping linear")
for r in 1 2 3 4 5; do
  for seek in ${ORDERS[$((r-1))]}; do
    for q in $QUERIES; do
      env -C $RUN KERMIT_WORKSPACE=$WT $BIN bench $CRIT --name ss-$seek-r$r \
        --report-json $RUN/reports/$seek-r$r-$q.json \
        run watdiv-stress-100-test-1-prelim -q $q -i tree-trie -a leapfrog-triejoin \
        --ds-layout-seek $seek -m iteration --verify
      env -C $RUN KERMIT_WORKSPACE=$WT $BIN bench $CRIT --name ss-$seek-r$r \
        --report-json $RUN/reports/$seek-r$r-$q-col.json \
        run watdiv-stress-100-test-1-prelim -q $q -i column-trie -a leapfrog-triejoin \
        --ds-layout-seek $seek -m iteration --verify
    done
    env -C $RUN KERMIT_WORKSPACE=$WT $BIN bench $CRIT --name ss-$seek-r$r \
      --report-json $RUN/reports/$seek-r$r-lubm.json \
      run lubm-reference -i tree-trie -a leapfrog-triejoin --ds-layout-seek $seek -m iteration --verify
    env -C $RUN KERMIT_WORKSPACE=$WT $BIN bench $CRIT --name ss-$seek-r$r \
      --report-json $RUN/reports/$seek-r$r-lubm-col.json \
      run lubm-reference -i column-trie -a leapfrog-triejoin --ds-layout-seek $seek -m iteration --verify
  done
done
```

This runs for hours. Write it to `$RUN/run.sh` and run it fully detached
(`setsid nohup bash $RUN/run.sh >$RUN/run.log 2>&1 & disown`). Then
monitor the log; don't busy-poll. Write `$RUN/README.md` with the commit
SHA, the probe list, the command and the date.

Analyse with kermit-lab:
- the median and spread per (query, structure, strategy) across the 5
  replicates;
- per structure, the geomean of galloping/binary and of linear/binary;
- the queries where galloping loses to binary by more than the replicate
  spread.

Publish the results as the run's report artifact, per the sweep
precedent.

- [ ] **Step 4: Supervisor checkpoint 3**

Send:

- the P3 SHAs and gate results;
- the measurement-2 summary, with the per-structure geomeans, the probe
  queries where galloping wins or loses, and the run path;
- every deviation from the plan.

The supervisor owns any issue comment. #80 does not change the default:
whether a later sweep adopts galloping is the user's call, informed by
these numbers.

---

## Task 10 [controller]: Hand-off (only on the user's instruction)

Follow `project_loom_landing_to_master`:

1. Fetch, and merge `origin/master` into the branch (never rebase).
2. Re-run the gate on the merge.
3. `git -C $WT push origin HEAD:master` only when the user says so.

The sweep must have finished first. Before pushing, confirm with the
supervisor that the insertion re-run (#84) is done.
