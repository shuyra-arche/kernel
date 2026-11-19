# Kernel tests

Test source belongs to the responsibility it verifies. Host tests follow the
production owners under `basis/`, `entry/` and `metal/`; common fixtures only
exist where there are actual shared consumers.

Host tests:

```sh
cargo +nightly test --locked --workspace --target x86_64-unknown-linux-gnu
cargo +nightly test --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu
```

Each target project owns its `suite.json` alongside its Cargo manifest,
Guest and any source probe patch. The common `Target` workflow discovers these
files, selects owners whose source inputs changed, and runs an independent job
per project/profile. No project name is declared in the workflow or runner.

To discover available jobs locally:

```sh
bash tools/tests/discover.sh "$PWD" "" 1
```

To run one project:

```sh
cargo +nightly build --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu
server/tests/runner/target/x86_64-unknown-linux-gnu/debug/verify run \
  --suite server/tests/target/basis/nest/completion \
  --kernel "$PWD" --output /tmp/nest --harts 1
```

Set `QEMU_RISCV64` and `QEMU_OPENSBI` for CPU suites (QEMU 10.0.x and
OpenSBI DBCN). Normal CI uses one Hart. Multi-Hart runs are manually selected
and only admitted by projects that declare support for them. An IOMMU target
project may instead own a Cargo test invoking QEMU directly.

To add a project, create its directory and `suite.json` with an ID,
profile/Hart support, owned source patterns and one execution declaration.
A project can be deleted by deleting its directory. Neither action requires
editing CI, a central case enum or another project's tests. No historical
ABI fixtures or compatibility runners are kept.

Results report actual build/exit/Guest evidence. A green Host test cannot
prove target CPU, IOMMU or physical hardware effects.
