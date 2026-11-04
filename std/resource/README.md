> Historical upstream README, retained for provenance. The source import is
> byte-for-byte under `std/resource/src`; see `../README.md` for the current
> ownership contract.

# resource

`resource` 是 shuyra 的 `no_std` 基础资源机制包：孔通信、页映射、门铃、多路等待，
以及能力与映射的静态内核缝。它是基础机制包，不是 Unit，也不需要 server。

## 独立构建与测试

本目录是独立 Cargo 工作区，根包 `resource` 为 `0.1.0`、edition 2024，
`[workspace]` 使用 `resolver = "3"`。工具链由 `rust-toolchain.toml` 固定为
`nightly-2026-09-04`。

RISC-V 库构建：

```sh
cd resource
cargo build --locked --target riscv64gc-unknown-none-elf
cargo build --locked --release --target riscv64gc-unknown-none-elf
```

宿主检查：

```sh
cargo check --locked --target x86_64-unknown-linux-gnu
```

`src/` 是来源 `crates/resource/src` 的逐字节快照。根包 `[lib]` 保持
`test = false`、`bench = false`、`doctest = false`，因此这里的 `cargo test`
没有 owned 断言，不代替能力测试。

## 能力测试

`tests/capability` 是独立的工作区，继续用 `#[path = "../../src/..."]`
投影本目录来源 `src/` 中的同一组模块，并保留原来的 fake Env。它有自己的
`Cargo.lock`，从同一固定 manifest 生成，不改 provider 版本。

```sh
cd tests/capability
cargo test --locked --target x86_64-unknown-linux-gnu
cargo test --locked --offline --target x86_64-unknown-linux-gnu
```

两次运行都应执行完整的 22 个测试。该工作区不涉及 QEMU。

## 依赖

根包只保留原来的两项依赖：

- `env`：公开 Git 仓库，固定 rev
  `d3000253cd5d13d466e91fafd4dbb27a5ec5aaae`；
- `erra 0.2.0`：registry，`default-features = false`。

`env` 的传递依赖 `wire` 与 `mold` 仍由 `env` 的 manifest 固定为
`0ff77dc4573fa2620d2fe995a18c01949d9c7e57` 与
`f52501f4c91b686eded8b4bcd743b4e96084b7f9`。本目录没有
`[patch]`、`[replace]`、本地 Git 或 cache 投影。

来源与锁的生成过程见 `PROVENANCE.md`。
