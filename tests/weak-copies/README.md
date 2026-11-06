# Concurrent weak-copy bookkeeping

From the repository root:

```sh
cargo test --manifest-path tests/weak-copies/Cargo.toml --locked --target x86_64-unknown-linux-gnu
```

The fixture supplies hart identity and compiles the production tracker. It
exercises eight concurrent simulated harts, held-copy failures and
stored-to-temporary slot reuse. Host concurrency does not establish native SMP
or DMA behavior.
