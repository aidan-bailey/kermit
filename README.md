# Kermit &emsp; [![Build Status]][actions] [![dependency status](https://deps.rs/repo/github/aidan-bailey/kermit/status.svg)](https://deps.rs/repo/github/aidan-bailey/kermit) [![Latest Version]][crates.io]

[Build Status]: https://img.shields.io/github/actions/workflow/status/aidan-bailey/kermit/build.yml?branch=master
[actions]: https://github.com/aidan-bailey/kermit/actions?query=branch%3Amaster
[Latest Version]: https://img.shields.io/crates/v/kermit.svg
[crates.io]: https://crates.io/crates/kermit

*Kermit* is a library containing data structures, iterators and algorithms related to [relational algebra](https://en.wikipedia.org/wiki/Relational_algebra), primarily for the purpose of research and benchmarking. It is currently in early stages of development and as such all builds and releases should be considered unstable.

It is being written primarily to provide a platform for my Masters thesis.
The scope of which (preliminarily) encompassing benchmarking the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481) over a variety of data structures.
I intend to design Kermit in an easily-extensible way, allowing for the possibility of benchmarking other algorithms and datastructures in the future.

Rust was chosen as the project language for two main reasons:
1. The [Knowledge-Based Systems group](https://iccl.inf.tu-dresden.de/web/Wissensbasierte_Systeme/en) at [TU Dresden](https://tu-dresden.de/) is developing a new Rust-based rule engine [Nemo](https://github.com/knowsys/nemo), which I'm hoping the knowledge and implementions developed during this Masters will prove useful for. I strongly recommend checking Nemo out. Not only is it a very promising project, it is one of most beautiful, pedantically managed repositories I've come across.
2. I wanted an excuse to write Rust with actual purpose.

My objective is to write entirely safe, stable, and hopefully idiomatic Rust the whole way through. I am very interested in how much one can maintain readibility (and sanity) while striving to achieve this.

## Usage

Given a relation stored as a CSV file (`edge.csv`):

```csv
src,dst
1,2
2,3
3,4
1,3
```

And a Datalog query file (`query.dl`):

```prolog
path(X, Y, Z) :- edge(X, Y), edge(Y, Z).
```

Run a join with the `kermit` CLI:

```sh
kermit join \
  --relations edge.csv \
  --query query.dl \
  --algorithm leapfrog-triejoin \
  --indexstructure tree-trie
```

Output (CSV to stdout):

```
1,2,3
1,3,4
2,3,4
```

Use `--output results.csv` to write to a file instead. Multiple relation files can be provided by repeating the `--relations` flag. Both `tree-trie` and `column-trie` index structures are supported.

Add `--bench` (or `-b`) to print timing statistics to stderr:

```sh
kermit join \
  --relations edge.csv \
  --query query.dl \
  --algorithm leapfrog-triejoin \
  --indexstructure tree-trie \
  --bench
```

```
--- join statistics ---
  data structure:  TreeTrie
  algorithm:       LeapfrogTriejoin
  relations:       1
  output tuples:   3
  load time:       0.000412s
  join time:       0.000076s
  write time:      0.000003s
  total time:      0.000521s
```

## Contributing

Thanks for taking an interest! Perhaps after I've finished my thesis.

## License

This repository, as is customary with Rust projects, is duel-licensed under the [MIT](https://github.com/aidan-bailey/kermit/blob/master/LICENSE-MIT.txt) and [Apache-V2](https://github.com/aidan-bailey/kermit/blob/master/LICENSE-APACHE.txt) licenses.

