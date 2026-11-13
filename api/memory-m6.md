# M6：原生镜像集成与性能验收

本轮在 Vanllia `tests/memory` 组合真实 Kernel、Client、初始 Task、第二个 Team、
CPU MMU、RISC-V IOMMU 和 virtio-blk。测试程序只调用公开 ABI；不把私有 Server
源码编入 guest，也不以 host fixture 的结果替代硬件运行。

## 镜像版本与兼容边界

API、Client、Server 的 `memory-v11` feature 必须由同一个 composition 选择。
选择后全局调用版本、Ledger、Capsule 都是 11；v10/未来版本调用、旧 Cell 以及
旧 Flat/Nest register adapter 不进入旧分派器。Share Open/Shut 在此镜像中拒绝。
默认 feature 仍是 v10，供尚未迁移的消费者使用；两个版本不是同一镜像中的 fallback。
这不是所有 Vanllia 产品组件已经完成 ABI 切换的声明。

三项 native host 检查验证调用版本、权限位以及 Ledger/Capsule 混用拒绝。
QEMU 另以正常 v11 guest 执行旧版本调用，并将相同 ELF 包成 v10 Capsule，
确认在初始 Task 执行前启动失败。

## 接通的运行路径

| 场景 | 实际检查 |
| --- | --- |
| 跨 Team 共享 IPC | 两个独立 CPU Nest，128 次消息；其中 64 次保持发布，64 次逐次撤回/发布；只撤回一方不影响另一方 |
| 持有转交 | 子 Team 创建并写入 Page，通过实际 Accord 委托后由父 Task Pull；PageId 不变，源 Slot 变 Missing，内容保持 |
| 原生缺页 | 真实 Task load 挂起；事件重复读取不消费；仅 Occupy 不恢复，Publish 后恢复；路由丢失和超时结束等待者 |
| 换出/换入与稳定复制 | Block 后拒绝写者；维护只读复制；维护发布未退休时 Vacate 拒绝；Missing 不补零；暂存 Page 恢复后 Push/Allow/Publish，逐字验证 4096 bytes |
| Virtqueue | 实际 PCI virtio-blk，CPU/External 同时持有可写 RAM；64 次写/读，校验 16 KiB，IO 期间无 Publish/Retract |
| 设备退休 | used-ring 完成不作退休证据；设备运行时保留 Pending 和两项责任；reset/drain 后仅结清 External，CPU 继续读写 |
| 丢失回复与异步结果 | Publish 回写地址无效，InspectResult 恢复最终提交；Retract 精确重放不重复退休；host 检查超过 journal 容量的连续完成、失败和 Task 消亡 |

被动存储 Task 只执行保存/恢复指令，不决定换页策略。此处是内存后备的服务组合，
没有承诺磁盘持久化、完整 Swap Service、多页原子快照或通用 COW。

## 本轮修复与可信条件

- Fault continuation 已终结 Task 时不再第二次执行 `Task::rise`。
- External 发布/退休保留 Pending；由原 Memo 的精确完成回调更新该 Task 的 journal。
  回调仅持有弱 Task 引用和原 Accepted，不拥有 Page、不授予权限、不代替硬件退休。
  取消或硬件失败记录终态失败，仍保留实际资源责任。
- `Topology::with_local_instructions` 是显式 unsafe 平台保证：只支持已准入的一个 Hart，
  现有 FENCE.I 在可执行 Publish/Protect 完成前运行。完整 Page 组仍禁止 W/X，
  有执行者的组不接纳 External。没有提供此保证的平台继续拒绝 EXECUTE。
- `boot::map_entry_mmio` 仅为初始 Team 安装明确的静态 RW 设备窗口，排除 RAM、
  受保护控制器与路由配置，不提供执行许可。映射属于启动布局，持续整个 Team
  生命周期；不是可撤销 Pend，不继承给新 Team，不参加普通 RAM fabric 证明。
  平台不得在原 Team 仍能访问该窗口时重分配设备。
- QEMU transport 使用固定现代 virtio-blk、单 Hart TCG、单 requester，无 passthrough、
  ATS/PRI、IOThread 或 ioeventfd。配置空间和 IOMMU 保留在 Kernel；所有 DMA 均过 IOMMU。
  reset 证据依据 QEMU 10.0.x `virtio_blk_reset` 的 `blk_drain` 与排队请求清理，
  并在后续 MMIO 读中验证 status/queue_enable 为零。此契约不外推到物理设备。

## 验证分层

| 层次 | 本轮证据 |
| --- | --- |
| Host | 默认 workspace 437 项通过；另有 native ABI 3 项；没有 CPU/IOMMU 硬件结论 |
| RV64 编译 | 默认 v10 与原生 v11 provider/client/image，debug/release 分开验证 |
| QEMU v11 | 上述全部场景；保存串口、stderr、输入哈希、QEMU 命令与版本、耗时、退出状态；debug/release 分开记录 |
| QEMU v10 | 保留既有 14 项环境测试，作为兼容回归 |
| 实机 | 用户确认无实机；未验证，不用 QEMU 替代，不扩展平台支持清单 |

性能只报告同一 QEMU invocation 内的工作量与观测时间。保留映射与逐条重新映射
各交换 64 条 IPC；Virtqueue 的 64 次请求始终保持两侧映射。具体数值由
Vanllia runner 的 `PERF` 行保留，不作为真实设备吞吐、延迟上限或硬件普遍加速承诺。

先合并 Kernel 依赖链（包括 #154），再更新 Vanllia 的 API/Server 精确版本与锁文件。
M6 composition 单独选择 native feature；默认产品消费者的整体迁移、Share 创建新契约、
运行期 MMIO 发布和 SMP/未知硬件仍不在本轮已验证边界内。
