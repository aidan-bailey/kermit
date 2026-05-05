# Regenerating `lubm-uba.jar`

This jar is the canonical Lehigh University Benchmark data generator (UBA), built from
Rob Vesse's modernised fork. The upstream generator preserves bit-identical output
relative to the original SWAT Lab code — this property is load-bearing for benchmark
reproducibility, so do not regenerate casually.

## Source

- **Upstream**: <https://github.com/rvesse/lubm-uba>
- **Local path used to build the vendored copy**: `/tb/Source/Academia/lubm-uba-rs`
- **Branch**: `improved`
- **Commit**: `32f83e3b8d88550af77fa563e94039ebf4229d16` (`Add Nix flake providing dev shell with JDK 8, Maven, and Jena`)

## Build environment

The lubm-uba-rs flake provides JDK 8 + Maven + Apache Jena. Use it for reproducibility:

```bash
cd /tb/Source/Academia/lubm-uba-rs
nix develop --command mvn -v
# Apache Maven 3.9.12
# Java version: 1.8.0_472, vendor: Oracle Corporation
```

The Maven enforcer plugin pins source/target to Java 1.7 (`<jdk.source>1.7</jdk.source>`
in `pom.xml`). Building under JDK 8 satisfies the enforcer's `requireJavaVersion ≥ 1.7`
rule. Do **not** modernise the source/target level — bit-identical output depends on
`java.util.Random` semantics that are stable across JDK versions but become noisy if
anyone "improves" the code.

## Procedure

```bash
cd /tb/Source/Academia/lubm-uba-rs
nix develop --command mvn -q clean package
sha256sum target/lubm-uba.jar
cp target/lubm-uba.jar /tb/Source/Academia/kermit/.../kermit-rdf/vendor/lubm-uba/lubm-uba.jar
```

The shaded jar (`target/lubm-uba.jar`, ~2.9 MB) is what we vendor; the unshaded
`target/original-lubm-uba.jar` (~76 KB) requires the maven classpath at runtime and is
not what we want.

## Provenance of the currently-checked-in jar

| Field | Value |
|-------|-------|
| Built | 2026-05-05 |
| Source commit | `32f83e3b8d88550af77fa563e94039ebf4229d16` |
| Source branch | `improved` |
| JDK | OpenJDK 1.8.0_472 (Oracle, NixOS) |
| Maven | Apache Maven 3.9.12 |
| Source/target level | Java 1.7 (per `pom.xml`) |
| Size | 2,915,542 bytes |
| SHA-256 | `948de481c9acdb75c5f0153fc3cb548af13a149f1366b484e42d86d27805835b` |

The pipeline records the SHA-256 of whichever jar it actually invokes into each
generated benchmark's `meta.json`, so a snapshot regenerated against a different
jar is distinguishable post-hoc. There is no runtime hash check that *rejects* a
mismatched jar — adding one is listed as future work in
`kermit-rdf/src/lubm/README.md`.

## Verification

After regenerating, smoke-test single-file consolidation:

```bash
mkdir -p /tmp/lubm-smoke
nix develop --command java -jar target/lubm-uba.jar \
    -u 1 -s 0 -f NTRIPLES --consolidate Maximal --compress \
    -o /tmp/lubm-smoke --quiet
ls /tmp/lubm-smoke/Universities.nt.gz   # must exist; multiple files = bug
zcat /tmp/lubm-smoke/Universities.nt.gz | wc -l   # ~103,076 for LUBM(1, 0)
```

If the triple count drifts substantially from 103,076 the jar is no longer bit-
compatible with the canonical generator and the rebuild should be rejected.

## License

Original UBA: SWAT Lab, Lehigh University. Modified code: Rob Vesse. The fork's
README.md retains the original copyright notice and adds Rob Vesse as maintainer of
modifications. Verify upstream license terms permit redistribution of the built jar
inside this repository before merging changes that cross repository boundaries.
