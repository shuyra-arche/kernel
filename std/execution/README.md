> Historical upstream README, retained for provenance. The prototype in this
> repository moved the boot/panic/global-allocator registrations to
> `std/rt`; see `../README.md` for the current ownership contract.

# execution

Shuyra 的用户执行原语库：任务与线程局部存储、内存来源与映射、程序启动参数和锁。
包是 `no_std` 库，不含服务、镜像构建或完整运行时。

本目录从公开冻结基线 `66bb8b5c342c50b141eb935a85efdfcecb91451b`
（分支 `codex/shuyra-execution-extraction`）的 `crates/execution` 提取，
是独立的单包 Cargo 工作区。根包 `execution` 为 `0.1.0`、edition 2024，
`[workspace]` 使用 `resolver = "3"`。逐文件映射和 SHA-256 见
[MIGRATION-MAP.md](MIGRATION-MAP.md)，来源与作者见
[PROVENANCE.md](PROVENANCE.md)。

## 目录

- `src/`：从来源逐字节复制的 23 个库源文件；
- `build.rs`、`link.ld`：原样保留；`build.rs` 只发布真实 `link.ld` 路径元数据；
- `tests/`：独立 owner 工作区，14 个测试（5 个分配器、1 个启动参数、8 个机制），
  继续用 `#[path = "../src/..."]` 投影本目录的同一份来源；
- `tests/unit/`：独立 owner 工作区，1 个 Unit 任务生命周期测试；
- `Cargo.lock`、`tests/Cargo.lock`、`tests/unit/Cargo.lock`：三个都必须入库的锁。

## 工具链与构建

工具链由 `rust-toolchain.toml` 固定为 `nightly-2026-09-04`，含
`riscv64gc-unknown-none-elf` 目标。建议显式共享构建目录，避免重复构建依赖：

```sh
export CARGO_TARGET_DIR=/home/anreonyr/Develop/shuyra/target/shared-build/pr12-provider

cargo build --lib --target x86_64-unknown-linux-gnu --locked
cargo build --lib --target riscv64gc-unknown-none-elf --locked
cargo build --lib --release --target riscv64gc-unknown-none-elf --locked
```

依赖已缓存后，对同一组命令追加 `--offline` 复验。三个库构建只产生
`libexecution.rlib`，不产生完整 ELF，因此本阶段不验证 `_start`、TLS 装配或
最终链接布局；`link.ld` 只作为构建元数据发布。

## Owner 测试

两个 owner 工作区各自有 manifest 和锁，必须用显式 `--manifest-path` 运行：

```sh
cargo test --manifest-path tests/Cargo.toml \
  --target x86_64-unknown-linux-gnu --locked
cargo test --manifest-path tests/unit/Cargo.toml \
  --target x86_64-unknown-linux-gnu --locked
```

缓存就绪后，同一命令追加 `--offline` 复验。实际结果为 14/14 和 1/1 通过。

根包的 `[lib] test = false` 只关闭根库自身的单元测试入口；Cargo 仍会自动把
`tests/lib.rs` 发现为名为 `lib` 的 integration test target。故根目录的
`cargo test` 不是 owner 测试入口，不能据此宣称“没有测试”。本快照没有修改
`autotests`、没有新增宿主成功 stub，也没有改任何原有测试入口。

## 验证范围

宿主侧只验证了通用夹具：`FixtureKernel` 与 `fixture_state` 驱动 8 个机制测试，
分配器测试和 Unit 测试使用各自的 mock。`Native` 后端、真实 `env` 内存调用和
任务运行时没有被执行；宿主 `env::ecall::trap` 仍是既有的 `unimplemented!`。
因此本快照不宣称宿主任务运行、真实内存映射或完整启动路径可用。本阶段也不运行
QEMU，不迁移消费者。

## 来源、作者与许可

来源基线由 `anreonyr <anreonyr@gmail.com>` 提交，历史中另有一个
`Serial Device <anreonyr@gmail.com>` 提交。原仓历史保留在来源仓库，不在本目录
重写或压缩。来源树没有 `LICENSE`、`COPYING`、`NOTICE`、license 字段或 SPDX
声明；本仓不添加许可证，也不授予任何许可证。
