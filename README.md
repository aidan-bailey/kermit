# Kermit

*Kermit* is a library for data structures, iterators and algorithms relating to [relational algebra](https://en.wikipedia.org/wiki/Relational_algebra), primarily for the purpose of education and benchmarking.

It is being written primarily to provide a platform for my Masters thesis.
The scope of which (preliminarily) encompassing benchmarking the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481) over a variety of data structures.
I intend to design Kermit in an easily-extensible way, allowing for the possibility of benchmarking other algorithms and datastructures in the future.

Rust was chosen as the project language for two reasons:
1. The [Knowledge-Based Systems group](https://iccl.inf.tu-dresden.de/web/Wissensbasierte_Systeme/en) at [TU Dresden](https://tu-dresden.de/), the place I'm currently attending a research exchange, is developing a new Rust-based rule engine [Nemo](https://github.com/knowsys/nemo), which I'm hoping the knowledge and implementions developed during this Masters will prove useful for.
2. I wanted an excuse to write Rust with actual purpose.

My objective is to write entirely safe, stable, and hopefully idiomatic Rust the whole way through. I am very interested in how much one can maintain readibility (and sanity) while striving to achieve this.

## Contributing

Perhaps after I'm finished my thesis!

## License

*Kermit* is license under [LGPL-2.1](https://www.gnu.org/licenses/old-licenses/lgpl-2.1.en.html).
