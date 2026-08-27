# SizeTrail 开发规格书（Development Spec）

> 目标读者：在 Codex 中执行本项目的编码 Agent，以及项目维护者。
>
> **本文由 `decisions.md` 派生。** 两者冲突时 **`decisions.md` 优先** —— 本文是那些决策的规格化表达，不是独立来源。
> 实现与本文冲突时：先改 `decisions.md`（若属决策变更）或本文（若属表达错误），再改代码。

| 项 | 值 |
|---|---|
| 名称 | SizeTrail |
| 命令 | `sizetrail`（唯一二进制，无短别名 — Q23） |
| 定位 | 面向开发者的**只读**、审计级 macOS 存储归因解释器 |
| **写操作** | **永久禁止。见 §8 —— 这是产品的定义性属性** |
| API 基线 | macOS 13（deployment target）；验证矩阵见 §5.3 |
| 语言 | Rust stable（MSRV 锁定并写入 CI） |
| 许可 | MIT OR Apache-2.0（**不得**复制 mole 代码，见 §14.2） |
| v0.1 范围 | Xcode/CoreSimulator + Homebrew 两个 adapter，schema 明确不稳定 |
| v1.0 范围 | 追加 Docker Desktop adapter，schema 稳定 |

---

## 1. 产品契约

### 1.1 权威表述（Q1 + Q2）

> SizeTrail 是面向开发者发布的开源、审计级 macOS 存储解释器。v1 深入 Xcode、模拟器、容器与构建工具；通用的是归因引擎和安全策略，规则集限定开发者场景。它不声称复现 Apple 的 System Data 总数。计量契约与口径文档是一等交付物。

四条不可让渡的原则：

1. **公开每个数字的口径。** 没有 measurement basis、scope、coverage 与 uncertainty 的数字不得出现在任何输出中。
2. **不声称对账 Apple 的 System Data。** Apple 通过私有 API 计算分类，我们读不到；共享块、快照与 purgeable 重叠下也不存在守恒分解。
3. **只读。** 见 §8。
4. **诚实是功能。** unknown 与 coverage gap 是一等输出，不得为了截图或推广隐藏。

### 1.2 差异化（诚实版本）

缺口**不是**「无人解释」，而是「解释被实现为特例集合而非模型」。

mole 已有 allocated-size、硬链接去重、purgeable 标量与若干 insight，但分散于 Status / Analyze / Clean 三处，没有共同的 attribution 实体、口径、覆盖范围与 unknown residual。其快照处理仅为打印数量 + 提示用户自行运行 `tmutil`；所谓 hidden-space insights 实为针对单个 issue 的硬编码特例。

**SizeTrail 的差异是「一等归因模型」，不是「mole 没有任何相关能力」。** 若 mole 将来完整实现同一契约，SizeTrail 没有不可复制的技术护城河 —— 只能靠更聚焦、更可信的执行质量存在。这条判断是每次功能决策的试纸：**这个功能是让存储占用更可解释，还是只是多测了一个目录？**

### 1.3 非目标

- **任何形式的文件系统写操作**（§8）
- 清理、删除、驱逐、prune、thin、undo、history
- TUI（Q19 —— 显式删除，不得在实现期以遗漏为由重新加入）
- 通用磁盘空间可视化浏览器、应用卸载、系统监控、性能「优化」
- 全卷分类 / 逼近 Apple 的 System Data 数字
- 自定义规则表 / `rules.d`（Q16）
- 持久缓存（Q17）
- Windows / Linux / GUI
- 遥测、联网上报、自动更新检查

---

## 2. 计量模型（本项目的技术核心）

### 2.1 三平面模型（Q6）

**不跨 measurement basis 求和。** 这是硬约束，不是风格偏好。

| Plane | 名称 | 内容 |
|---|---|---|
| **1** | 容量事实 | container allocated、各 volume used、普通/重要/机会性用途可用容量。**逐个数字声明口径** |
| **2** | 工具链归因 | 每个 store 输出自己的度量向量、来源、范围、重叠与 unknown |
| **3** | 处置结果 | 操作 footprint、预计边际回收（区间，§2.3）、实际 free-space delta |

**禁止事项：**

- 禁止全局 `unattributed: u64` 字段 → 改为结构化 `coverage_gaps`。
- 禁止跨平面求和或相减得出「未归因空间」。
- 只有**同口径且已证互斥**时才允许算 remainder。

**为什么 plane 1 必须逐个数字声明口径：** Apple 自己的 `df` 就有多口径。`df.c` 的 `usedblks()` 以 `getattrlist(ATTR_VOL_SPACEUSED)` 为主，`f_blocks - f_bfree` 仅为失败回退；且 `availblks = f_bavail + used`，连容量百分比的分母都不是容器总量。

### 2.2 三个必须正确处理的计量陷阱

**(1) `st_blocks` 是 allocated footprint，不是物理占用。** 它解决稀疏文件但**不解决 clone**。实测：20MB 文件 `cp -c` 后两文件各报 `blocks=40960`，`du -sk` 对约 20MB 物理占用报 40MB。

**(2) inode 去重必须用 `(fsid, fileid)`，不是 `(dev, ino)`。** APFS firmlink 与 System/Data 卷拆分会让路径看起来在一个卷里而实际不在。去重集合按扫描会话隔离。

**(3) purgeable 不可回收也不可配平。** `EF_IS_PURGEABLE` 是逐对象删除策略标志，**不提供该对象对系统 purgeable capacity 的独占字节**，不能与 plane 1 的容量标量配平。

### 2.3 回收估算区间（Q10）

区间为 `[private floor, reference-counted ceiling]`，即 `[Σ privatesize, Σ allocated]`。

**该区间仅在下列条件全部满足时成立：**

- APFS 上的非目录 file forks；
- 按真实 `(fsid, fileid)` 去重；
- 目标集包含该 inode 的**全部 hardlink**；
- 数值成功返回且扫描后无 link/clone/snapshot 并发变化；
- 动作为永久 unlink 并最终关闭所有 open reference。

**边界规则（逐条实现）：**

| 情况 | 规则 |
|---|---|
| 未完整覆盖 hardlink link set | **floor = 0** |
| `allocated` 缺失 | **ceiling = unknown** |
| 目录、xattr、文件系统 metadata | **不进入区间** |
| Trash 动作 | 立即释放**恒为 0**；区间只表示清空 Trash 后的 deferred potential |
| 扫描后可能新建 snapshot | 令**未来保证的下界失效为 0**，**不得**把上界归零 |

**术语纪律：** 区间宽度**不能称「共享字节」**（多个 clone 会让上界重复膨胀），只能叫 `allocation uncertainty width`。

**实际容器 free delta 必须单列**，不得宣称落在区间内 —— 它受 open fd、目录 metadata 与并发写盘影响。

**已被反例否决、不得重新引入的收敛规则：**

```text
EF_MAY_SHARE_BLOCKS == 0 && volume_snapshots == 0  ⟹  allocated == private   ❌
```

反例见 `decisions.md` 附录 B。根因是 `ATTR_FILE_ALLOCSIZE` 覆盖所有 forks，而 `PRIVATESIZE` 并非它在所有存储形态下的完整独占对应量。普通唯一文件常见相等，**但不是恒等式**。

「存在 snapshot 就把上界压到 0」同样是错的：snapshot 创建后才生成的文件删除仍可完全释放，且 `PRIVATESIZE` 已内生排除被 clone/snapshot 困住的 extent。

### 2.4 类型化观测信号（Q13）

区间宽度用**信号**解释，不用**成因**解释。

```rust
enum Observation { Direct, Derived }

enum Relation {
    PossibleWidthExplanation,   // 可能解释宽度，非因果证明
    TestedWidthCorrelate,       // 当前 fixture 上验证的相关性
    LogicalAllocationGap,       // 只解释 logical − allocated
    ReclaimPolicy,              // 回收策略，不解释 private gap
    DeletionScope,              // 删除范围，不解释 private gap
}

enum Scope { Object, Inode, Volume }
```

| 观测 | 能解释什么 | relation |
|---|---|---|
| `EF_MAY_SHARE_BLOCKS` | clone sharing 的可能性 | `PossibleWidthExplanation` |
| 卷存在快照 | snapshot retention 的可能性，**不能定位到对象** | `PossibleWidthExplanation` |
| `rsrcallocsize > 0` | fixture 中与 private gap 相关的计量域差异 | `TestedWidthCorrelate` |
| `UF_COMPRESSED` | fixture 中与 private gap 相关的计量盲点 | `TestedWidthCorrelate` |
| `EF_IS_SPARSE` | `logical − allocated`，**不解释** `allocated − private` | `LogicalAllocationGap` |
| `linkcount > 1` | 删除范围与 inode 去重 | `DeletionScope` |
| `EF_IS_PURGEABLE` | 文件系统回收策略 | `ReclaimPolicy` |

**硬约束：**

- **永久保留 `unexplained_private_gap`。** 即使所有信号都未命中，仍可能存在无法解释的 gap。
- **禁止把信号字节相加**，禁止声称它们分解了区间。**任何负信号组合都不能令区间收敛。**
- `unmeasurable` 与 `unexplained_private_gap` 是**不同类型**，不得混同。
- resource fork 与压缩行为是**受支持系统上的 fixture 事实，不是公开 APFS 恒等式**。
- 所有信号可能并发。
- `filesystem_compressed` 高优先级展示为「private floor 不提供信息」，同时保留完整信号。
- 不得指出某个文件被哪个具体快照保留 —— 公开 API 没有 file extent → snapshot 映射。只能表述为「共享块或快照导致的不确定性」，并可并列显示该卷的快照事实。

### 2.5 属性能力门控（Q12）

所有 `getattrlist` 属性使用前必须检查三项：**volume capability、capability 的 `valid` 位、`ATTR_CMN_RETURNED_ATTRS` 返回掩码**。

| 能力 | 最低版本 | 用途 |
|---|---|---|
| `PRIVATESIZE`、`CLONEID`、`SEALED`、`EF_MAY_SHARE_BLOCKS`、`EF_IS_PURGEABLE`、`SF_DATALESS` | 13 | 基础计量路径 |
| `CLONE_REFCNT`、`ATTRIBUTION_TAG`、`EF_SHARES_ALL_BLOCKS` | 14.0 | capability-gated 补充诊断 |
| `CLONE_MAPPING`、卷级 attribution capability | 14.4 | 可选增强 |

已收窄的用法：

- `ATTR_CMNEXT_ATTRIBUTION_TAG` 是 bundle name 的 **64 位 hash**，无公开反向映射契约。可用于分组或辅助验证，**不能替代路径/adapter 归因**，不得当作 bundle-id 字符串展示。
- 完整 pure-clone family **不得无条件**令区间收敛到 `allocated`。clone ID 描述数据 stream 而 `ALLOCSIZE` 涵盖所有 forks；未经 fixture 证明不得视为同一集合。仅为后续可验证的 tightening optimization，**不进入首版正确性契约**。
- `EF_MAY_SHARE_BLOCKS` 清零只排除「可能与另一文件共享块」，不排除 snapshot trapping。

---

## 3. 平台约束

### 3.1 TCC / Full Disk Access

授权对象是**调用 SizeTrail 的终端 App**（Terminal / iTerm2 / Ghostty…），不是二进制本身。

**硬性要求：必须区分「这里是 0 字节」和「我看不见这里」。**

- 对每个受 TCC 保护的 region 做探针读取，捕获 `EPERM`。
- 被拒绝时标记为 typed `permission_denied` 状态，**不得静默报 0**。
- 映射到退出码 3（§11.3），属**信息性**而非错误。
- `sizetrail doctor` 输出当前终端 App 名称、授权状态与具体授权路径指引。

### 3.2 iCloud / File Provider（严格约束）

**读取 `SF_DATALESS` 前的元数据调用本身就可能 materialize 文件** —— 这会导致真实的网络下载与磁盘写入，直接违反只读契约。

- 扫描前**必须**设置并**验证** `IOPOL_MATERIALIZE_DATALESS_FILES_OFF`。
- 设置或验证失败 → 整个相关 root 记为 `unknown`，不继续探测。
- `EDEADLK` 同样记为 `unknown`。
- **Cloud / File Provider roots 永久排除**，不进入任何外部工具调用。

### 3.3 SIP

SIP 默认启用。把它当免费安全网，但不得依赖它作为唯一防线（用户可能关闭）。永久只读后本项影响已大幅下降。

### 3.4 依赖的系统工具

已确认可用：`tmutil`、`brctl`、`mdutil`、`diskutil`、`xcrun`。`fd` 不存在 → 不得依赖，自行实现并行遍历。

**所有外部命令必须：** 调用前 `which` 探测 → 缺失时记 `not_present`（退出码 0，非失败）→ 版本落在已验证范围外时显式降级 → 绝不 panic。

只允许**闭集**的只读命令。见 §8.2。

---

## 4. 技术选型

| 关注点 | 选择 | 理由 |
|---|---|---|
| 语言 | Rust stable | 单二进制无运行时；类型系统可静态排除写路径 |
| CLI | `clap` (derive) | 标准选择，自带 completion 生成 |
| 并行遍历 | `jwalk` 或 `ignore::WalkBuilder` | 并行 walk，显著快于 `walkdir` |
| 序列化 | `serde` + `serde_json` + `toml` | 规则表 TOML，输出 JSON |
| 错误 | `anyhow`（应用层）+ `thiserror`（库层） | — |
| 日志 | `tracing` + `tracing-subscriber` | `--debug` 开启结构化日志，写 stderr |
| 测试 | `cargo test` + `insta` + `assert_cmd` + `tempfile` + `proptest` | 见 §10 |

**不要引入：** `ratatui` / `crossterm`（Q19 已删 TUI）、`trash`（Q11 已删写操作）、async 运行时、ORM、任何网络库、任何 GUI 框架。

新增依赖前先按 ponytail 的阶梯检查 std 是否已覆盖（§13）。

---

## 5. 架构

### 5.1 分层

```
┌────────────────────────────────────────────────────┐
│  cli        clap 命令定义与分派                      │
├────────────────────────────────────────────────────┤
│  render     人类文本（流式）/ JSON（确定性）          │
├────────────────────────────────────────────────────┤
│  model      finding / attribution / signal / advice │
├────────────────────────────────────────────────────┤
│  adapters   typed toolchain adapters                │
├────────────────────────────────────────────────────┤
│  rules      内置 TOML 加载与求值                     │
├────────────────────────────────────────────────────┤
│  capacity   plane 1 容量事实                        │
├────────────────────────────────────────────────────┤
│  fsx        getattrlist 计量、(fsid,fileid) 去重     │
├────────────────────────────────────────────────────┤
│  policy ★   副作用闸门：外部命令与受控读的唯一出口     │
└────────────────────────────────────────────────────┘
```

**★ 关键不变量（取代原 guard 层的地位）：**

1. **整个 crate 不存在文件系统写操作。** 唯一允许的输出是 stdout / stderr。
2. **`policy` 是发起外部命令的唯一出口。** 其他层不得直接调用 `std::process::Command`。
3. 两条均由 CI 静态检查强制（§10.3）。

### 5.2 Adapter 契约（Q8）

```rust
trait ToolchainAdapter {
    fn id(&self) -> AdapterId;

    /// 探测工具链是否存在、版本是否在已验证范围内。
    /// 不存在 → NotPresent（非失败）；未知版本 → Degraded。
    fn probe(&self, ctx: &PolicyCtx) -> AdapterState;

    /// 枚举 store 对象。只读。
    fn inventory(&self, ctx: &PolicyCtx) -> Inventory;

    /// 归类：action/mechanism、recoverability、sensitivity（Q4）。
    fn classify(&self, inv: &Inventory) -> Vec<Finding>;

    /// 生成建议。永不执行。
    fn advise(&self, f: &Finding) -> Vec<Advice>;
}
```

**没有 `execute`。** 契约就是 `probe → inventory → classify → advise`（Q7 + Q11）。

**adapter 纪律（Q8）：**

- v1 只上 **3 个全深度** adapter：Xcode/CoreSimulator、Homebrew、Docker Desktop。**浅 adapter 比没有更差** —— 它把 mole 的 feature-local 问题搬进我们自己的架构。
- 每个 adapter **必须钉住已验证的第三方 CLI 版本范围**，未知版本显式降级。这是最大的长期维护风险：adapter 包装的是第三方 CLI，输出格式会变。
- 规则**只能引用已编译的 adapter id**，不能提供任意命令。**这是安全属性，不是架构品味** —— 允许 TOML 携带任意命令等于开命令注入面。

### 5.3 验证矩阵（Q12）

**API/deployment target 与验证矩阵是分离的两件事。**

| 目标 | 状态 |
|---|---|
| deployment / API target | macOS 13 |
| 当前 GA 验证 | macOS 15、26（Apple Silicon 与 Intel） |
| macOS 14 arm64 | 仅在 `macos-14` runner 存续期间验证（2026-11-02 下线），之后自动移出矩阵。Intel 需 `macos-14-large` |
| `xcode-27` lane | **只证明 macOS 26 上的工具链/SDK 兼容性。** 它是运行在 macOS 26 arm64 上的 SDK preview，不是 macOS 27 runtime；GitHub 当前无 `macos-27` runner |
| macOS 13 | best-effort，未经 hosted runtime CI 验证 |

- CI 检查两种架构产物的 **Mach-O minimum OS version**。
- **支持表由实际 CI matrix 生成**，不静态声称永久版本列表。
- 结构性约束：最多两个 GA + 一个 beta，自动轮换。

### 5.4 目录布局

```
src/
  main.rs
  cli/           命令定义与分派
  render/        text.rs（流式）json.rs（确定性）
  model/         finding.rs signal.rs advice.rs interval.rs
  adapters/      mod.rs（契约）xcode.rs homebrew.rs docker.rs
  rules/         loader.rs schema.rs builtin/*.toml
  capacity/      plane1.rs（容量事实与口径标注）
  fsx/           attrs.rs（getattrlist）dedupe.rs root.rs
  policy/    ★   sideeffect.rs（外部命令闸门 + registry）
tests/
  fixtures/      伪造 HOME 树的构造器 + APFS 反例构造器
  apfs_*.rs      §2 计量正确性测试（含 decisions.md 附录 B 全部反例）
  policy_*.rs    ★ 零写与副作用上限测试
  cli_*.rs       端到端（全部走 --root）
  snapshots/     insta 快照 + payload 逐字节 fixture
```

**`--root` 沙箱保留。** 永久只读后它不再是删除防护，而是**测试注入机制** —— 引擎全程只通过可注入的 `Root` 抽象访问文件系统，使 fixture 测试无需真实 HOME。这条能力是 §10 测试策略成立的前提。

---

## 6. 规则表：数据而非代码

**所有静态路径目标是 TOML 数据，引擎是代码。** macOS 每个大版本都会挪动路径；路径变化应该只改数据文件。

**两条贡献路径（Q16）：**

1. **静态内置规则 = TOML。** 新增规则只需规则、evidence 和 fixture，**不要求写 Rust**。
2. **动态工具链能力 = typed adapter 代码。**

内置 TOML 用 `include_str!` 编入二进制，使规则、coverage 与版本绑定。**不读取任何外置规则**（无 `rules.d`）。

### 6.1 Schema

正交字段取代原单轴 T0–T4（Q4）：

```toml
[[rule]]
id          = "xcode.derived_data"
adapter     = "xcode"                  # 必须是已编译的 adapter id
title       = "Xcode DerivedData"
description = "Xcode 编译中间产物。"
paths       = ["~/Library/Developer/Xcode/DerivedData/*"]
os          = ">=13.0"                 # 不匹配则规则休眠

# ── 正交分类字段（存事实）──
mechanism     = "generated"            # generated | user_adjacent | user_owned | vendor_managed
recoverability = "rebuild_time_cost"   # trash_restore | rebuild_time_cost | redownload_bandwidth
                                       # | requires_external_device | unrecoverable
sensitivity    = "low"                 # low | medium | high

evidence = "下次 build 自动重建，代价是一次全量编译时间。"

[rule.preconditions]
process_not_running = ["Xcode"]        # owner 进程在跑则标注状态
```

### 6.2 硬性约束

- **`selection policy` 必须派生，不得存储（Q4）。** 任何「默认呈现强度」都是 `(recoverability, sensitivity, precondition)` 的**纯函数**；需要例外时必须填 `override_reason`，使 schema 本身可校验。存成字段则规则作者可在高 sensitivity 项上直接置真 —— 那正是 T0–T4 想防的错误换地方复发。
- **白名单语义。** 只有匹配规则的路径才被测量。**禁止**「X 下除 Y 以外全部」的黑名单写法。
- 每条规则必须有**非空 `evidence`**。写不出「删了会怎样」的规则不允许合入。
- 每条规则必须有至少一个对应 fixture 测试。
- 新增规则的 PR 必须说明量级来源（实测数据，不是猜测）。
- **规则不得携带任何命令。** 只能引用已编译 adapter id（§5.2）。

---

## 7. 报告结构

### 7.1 主分类（Q15）

**一级按开发者心智 / adapter，二级 finding 按对象用途。**

一级桶：`Xcode & Simulators`、`Homebrew`、`Docker Desktop`、**`未归属到任何工具链`**。

**「未归属」桶表示「已测量但 ownership 未归属」。** 它与 `unmeasurable`、`coverage_gaps` 是三个不同类型，**不得混同**。

技术信号通过固定优先级归约成一条摘要（如「clone sharing detected；回收估算不确定」）。

**归约规则版本化并受 truth contract 管辖。确定性总序：**

```text
relation priority → signal ID → scope → finding ID
```

fixture 必须覆盖 tie-break。`--explain` 展开完整 observation/relation/scope 集合，**必须无损**。

### 7.2 Finding ID（Q24）

```text
f1:<adapter_id>:<digest>
```

- `digest` 由**版本化算法**根据 `adapter_id + rule_id + normalized_path` 派生。
- `normalized_path` 是 **HOME 相对**的规范化路径。
- **绝不使用发现序号。**

### 7.3 Advice（Q11）

两种**完全分离**的类型：

```rust
enum Advice {
    /// 该工具链官方的确切命令 + 解释。SizeTrail 永不执行。
    Command(CommandAdvice),
    /// 仅打印路径。永不启动 Finder（Q14）。
    Reveal(RevealAdvice),
}
```

**advice contract（逐条实现）：**

- 命令**必须**来自版本门控的 adapter 数据，**绝不拼接用户输入**。
- 必须标记 `inspect` / `reversible` / `destructive`。
- **永不**附加 `--force`、`--yes` 或 shell 管道。
- 没有 dry-run 的命令必须明确写「厂商未提供可靠预览」。
- `destructive` advice **只能渲染，类型上不能进入 probe runner**（用类型系统强制，不靠约定）。
- `docker system prune --volumes` 可精确展示，但**必须**同时说明它会删除 stopped containers、未使用对象及 anonymous volumes；**不得包装成推荐的一键下一步**。
- 前后差异由用户再次运行 `scan` 获得。**永不自动运行建议命令。**

**Reveal 只打印路径（Q14）。** Finder reveal 可能启动 File Provider、枚举目录、生成缩略图并写 Finder 自身状态，不能混入零副作用契约。路径是一等机器输出，用户自行组合 `open -R`。

---

## 8. 只读契约（本项目最重要的一节）

**SizeTrail 永不删除、移动、驱逐、thin、prune 或以任何形式修改用户/系统数据。**

这是**永久产品边界**，不是发布切片（Q11）。**本节任何要求都不适用 ponytail 的 YAGNI 简化。**

### 8.1 零写不变量

1. 整个 crate 不存在 `fs::write`、`fs::remove_*`、`fs::rename`、`File::create`、`OpenOptions::write/append/create`。唯一输出是 stdout / stderr。
2. 无操作日志文件、无缓存文件、无配置文件、无 `completion` 写盘（Q17 / Q20 / Q22）。
3. **可机械验证的安全属性：正常运行不产生任何文件系统或外部工具写操作。** CI 静态 API 门禁 + 重定向写入位置环境变量的运行时快照 + macOS 15/26 上 `sandbox-exec` 的 `(deny file-write*)` 系统调用级强制，共同验证 §10.2 第 1、2 条。sandbox 门禁必须 fail closed；若目标 runner 不再支持该 deprecated 机制，必须先通过 decision record 重定证据或收窄声明，不得静默删除门禁。
4. **`src/fsx/sys.rs` 是唯一 unsafe 豁免点。** 该模块只导出只读系统调用包装，公开签名不得暴露可写句柄、可变缓冲区、写入 flag 或执行任意系统调用的能力；其他文件不得抑制 `unsafe_code` lint。模块内部无法由 Clippy 的 Rust API 禁用表证明零写，**其零写保证明确且仅由 §10.2 第 1 条运行时 harness 覆盖**。P1.1 只建立此边界，不实现 `getattrlist`。

### 8.2 副作用闸门（`policy` 层）

只读不等于无副作用。需要防范的读操作副作用：

| 副作用 | 防范 |
|---|---|
| Homebrew 自动更新 | 设置 `HOMEBREW_NO_AUTO_UPDATE=1`；只调用闭集只读子命令 |
| iCloud materialization | §3.2 的 `IOPOL_MATERIALIZE_DATALESS_FILES_OFF`，失败即记 unknown |
| mount trigger / 自动挂载 | 拒绝跨文件系统、拒绝嵌套挂载与 mount trigger；firmlink 按真实卷身份处理 |
| 错误 Docker context | 固定 `desktop-linux` context；context 不匹配则降级为 unknown |
| 外部命令写状态 | 闭集白名单，每条命令单独审计并记录其只读性依据 |
| CoreSimulatorService 启动 | 裸命令不自动探测；只在显式子命令下发起（Q22） |

**side-effect registry（Q17）：** 记录每个 probe 每次扫描的**最大调用次数**及关闭开关。registry 是数据，测试断言实际调用次数不超过声明值。

### 8.3 明令禁止

- 禁止任何文件系统写操作（§8.1）。
- 禁止 `sudo`。不安装 launchd 守护进程、不安装特权 helper。
- 禁止执行任何 advice 命令。
- 禁止在未验证 `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` 的情况下探测可能 dataless 的路径。
- 禁止遥测、禁止联网、禁止自动更新检查。
- 禁止读取外置规则文件。
- 禁止为「将来的写功能」预留抽象（Q11-A 明确排除 B 选项的路线保留）。

### 8.4 若未来重新引入写操作

必须重新经过独立的 P0、完整安全规格与价值验证。**不得**以「反正 v2 要写」为由在当前引入抽象。当时确定的写路径硬前置记录在 `decisions.md` Q3。

---

## 9. Truth Contract（Q5）

**必须机械化为 CI 门禁，不得是人工检查清单。**

单人开源项目上「不可绕过的治理」的真实失效模式不是不诚实，而是**流程被悄悄放弃却继续声称拥有它** —— 那比一开始就只写建议更差，因为它把已知的弱承诺换成虚假的强承诺。

### 9.1 CI 门禁

1. **声明模式 linter**：在 README、文档与站点文案中 grep 禁用模式。
2. **数字来源断言**：文档中每个公开量化数字必须来自 fixture 生成的文件。
3. **coverage/unknown 基线**：每次发布生成并保存基线。

P4 编写 README 与公开文档时，数字来源断言只能按以下方向演进：对版本号、章节号、日期建立**窄允许清单**；fixture 生成片段按显式标记转写并逐字节比对。**未经 decision record 直接放宽或绕过该检查，即为 truth contract 失效。** P1.1 仅记录此约束，不提前实现 transclusion。

### 9.2 禁用的声明模式（非穷尽）

- 「释放 X GB」/「可释放空间」（把已测量 footprint 写成可释放空间）
- 「解释全部 System Data」/「解释了 System Data」（把已知开发者存储写成 System Data）
- 「全球无商标」
- 为截图隐藏 unknown 或 coverage gap
- 把 `allocation uncertainty width` 称为「共享字节」
- 把区间上界或 observed free delta 伪装成严格物理边界

### 9.3 声明纪律

- **仅量化声明**需要 decision record；定性文案豁免。把门禁压在真正会骗人的东西（数字）上。
- 人类文案**不构成兼容 API**，可自由改写（Q15）。
- **「减少 unknown」需要已发布的基线值**，否则它与「释放 X GB」同为不可证伪声明。
- 核心演进指标：**新增多少可复现的工具链模型、减少多少 unknown**（相对基线），不是「累计可删多少 GB」。
- 商标记录口径固定为：「截至 2026-08-27 的初步多库筛查（USPTO、TMview）未发现冲突」。

### 9.4 benchmark 口径（Q17）

GA runner 记录 fixture benchmark，但**只发布「该 runner image + fixture」的原始时间**，不推广成用户机器性能承诺。runner 轮换后不得把不同硬件结果直接画成趋势。

---

## 10. 测试策略

### 10.1 分层要求

| 层级 | 内容 | 覆盖率门槛 |
|---|---|---|
| `policy` | 零写、副作用上限、闭集命令、context 门控 | **100% 分支** |
| `fsx` | §2 全部计量语义；`decisions.md` 附录 B 全部反例；`proptest` 模糊路径 | ≥ 95% |
| `model` | 区间边界规则、信号归约总序、finding ID 派生 | ≥ 95% |
| `rules` | schema 解析、os 门控、glob 展开、派生 selection policy | ≥ 90% |
| `adapters` | 版本门控与未知版本降级、`not_present` 路径 | ≥ 85% |
| `cli` / `render` | `assert_cmd` 端到端 + `insta` 快照 + payload 逐字节 | 关键路径全覆盖 |

### 10.2 必须存在的具体测试

**这些是「防止说谎」与「防止副作用」测试，缺一不可。**

1. **零写测试** ★ — HOME、TMPDIR/TMP/TEMP 与 XDG 写入位置全部重定向到 fixture 快照根内；全量 scan 后断言 `--root` 前缀内外均无任何文件被创建、修改或删除（对比前后完整 inode + nlink + mtime + ctime + size + xattr 名称和值快照）。CI 另在 macOS 15/26 以 `(deny file-write*)` sandbox 执行完整 scan，证明任意路径的写尝试会被内核拒绝。
2. **副作用上限测试** ★ — 断言每个 probe 的实际外部命令调用次数不超过 side-effect registry 声明值；断言未声明的命令从未被调用。
3. **APFS 反例测试** — `decisions.md` 附录 B 每个反例各一个测试：clone 双计、resource-fork-only（`allocated=2MiB, private=0`）、HFS 压缩（`ditto -x --hfsCompression` 构造）、hardlink 未完整覆盖时 floor 归零、稀疏文件只解释 logical gap。
4. **区间边界测试** — 逐条断言 §2.3 的五条边界规则；断言**不存在**任何输入使 `EF_MAY_SHARE_BLOCKS==0 && snapshots==0` 导致区间收敛。
5. **信号不可加测试** — 断言信号字节永不参与求和；断言任何负信号组合都不令区间收敛；断言 `unexplained_private_gap` 始终存在于输出类型中。
6. **JSON 确定性测试** — 同一 fixture 多次运行的 `payload` **逐字节相同**；`environment` 使用固定注入值（**不允许事后正则清洗**）；adapter 到达顺序变化不影响 payload。
7. **JSON 完整性测试** — 断言任意 region/adapter 失败下 `--json` 仍输出合法完整文档，stdout 不空缺。
8. **退出码矩阵测试** — 0 / 1 / 2 / 3 各自的触发条件；`not_present` 与 `excluded_by_user` 均映射 0。
9. **`--exclude` 前置生效测试** — 断言被排除子树内未发生任何 `stat`、`getattrlist` 或外部命令调用；断言无效 exclude 返回退出码 2。
10. **iCloud 门控测试** — 断言 `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` 设置失败时相关 root 记 unknown 且不继续探测。
11. **规则表完整性测试** — 遍历全部内置规则：`evidence` 非空、正交字段合法、`adapter` 是已编译 id、`paths` 非空、存在对应 fixture、无 `override_reason` 缺失。
12. **advice 类型测试** — 断言 `destructive` advice 在类型上无法进入 probe runner；断言 advice 命令从不含 `--force` / `--yes` / shell 管道；断言命令不含任何用户输入。
13. **finding ID 稳定性测试** — 断言 ID 与发现顺序无关；断言 HOME 变化下 digest 不变；断言算法版本变更时 `--from` 校验失败。

### 10.3 CI 门禁

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo deny check
cargo audit
# 自定义静态检查：
#   1. 全 crate 无 fs 写 API（§8.1）
#   2. policy/ 之外无 std::process::Command（§5.1）
#   3. truth contract 声明模式 linter（§9.1）
#   4. 文档中量化数字均来自 fixture 生成文件（§9.1）
#   5. 两种架构产物的 Mach-O minimum OS version == macOS 13
# 系统调用级零写：macOS 15/26 均以 sandbox-exec (deny file-write*) 执行 scan；不可用即失败
```

矩阵见 §5.3。**支持表由该 workflow 生成，不手写。**

### 10.4 发布前人工验证

自动化测试无法覆盖真实 macOS 的权限与路径现实。每个 minor 发布前，在**新建的测试用户账户**上执行：

1. `sizetrail doctor` — 权限诊断是否准确（含无 FDA 与有 FDA 两种状态）
2. `sizetrail scan` — 三平面数字的口径标注是否正确、coverage gap 是否诚实
3. `sizetrail scan --json | jq` — schema 完整性
4. 未安装 Docker 的机器上 — 断言 `not_present` 且退出码 0
5. 有本地快照的机器上 — 断言区间宽度与信号表述正确，**未虚构具体快照归因**
6. 全程用 `fs_usage` 或等价手段抓取 — **确认无任何写操作**

---

## 11. CLI 规格

### 11.1 命令表面（Q22）

```text
sizetrail                            显示帮助，不自动探测
sizetrail scan [--json]              只读归因报告
sizetrail explain <finding-id>       解释单个 finding
       [--json | --path] [--from <file|->]
sizetrail doctor [--json]            TCC、工具版本、side-effect gate 诊断
sizetrail rules [--json]             查看内置规则表
sizetrail completion <shell>         生成补全脚本（仅打印 stdout）
```

`scan` / `doctor` 的开关：`--no-xcode`、`--no-homebrew`、`--no-docker`、`--exclude <path>`（可重复）。

全局：`--debug`、`--no-color`、`--root <path>`（测试用）。

**裸命令不自动探测。** 因为「读」也有副作用 —— 不应自动启动 CoreSimulatorService、连接 Docker daemon 或运行 Homebrew。**所有有副作用的探测都要求显式子命令。**

### 11.2 输出模式（Q18）

| 模式 | 行为 |
|---|---|
| 人类文本 | findings **流式**输出；进度写 **stderr** |
| `--json` | 扫描完成后按稳定键排序，**一次输出确定性文档** |

- **不因非 TTY 自动切 JSON。**
- v0.1 不提供 JSONL。
- 尊重 `NO_COLOR`、`TERM=dumb`、窄终端。
- **不得仅用颜色传达信息。** 每个状态必须有文字或字形。

**`--json` 只要扫描已初始化就必须输出合法完整文档。** region/adapter 失败进入文档状态，不得导致 stdout 空缺。**文档状态与退出码正交。**

JSON 分两部分：

```json
{
  "schema_version": "0.1.0-unstable",
  "environment": { "...非确定信息：时间、主机、HOME、工具版本..." },
  "payload":     { "...规范化、稳定排序、逐字节 fixture 比对..." }
}
```

fixture 生成时 `environment` 使用**固定注入值**，**不允许事后正则清洗掩盖非确定性**。

**v0.1 JSON 明确不稳定**，但必须带 schema version；v1 后由 semver 管理。

### 11.3 退出码（Q21）

| 码 | 含义 |
|---|---|
| `0` | 所有已启用且适用的 region 完成。`not_present` 与 `excluded_by_user` **均为 0** |
| `1` | 无法初始化或无法形成合法文档的 fatal error。**窗口仅限初始化前失败**，实践中应罕见 |
| `2` | CLI usage error（clap）。**不产生 scan 文档**。无效 `--exclude` 属此类 |
| `3` | 产生了完整文档，但至少一个适用 region 因权限、未知版本、解析失败、超时等未测量 |

每个 region 使用 typed status；stderr 仅作人类诊断。

**退出码 3 属信息性**，无 FDA 时为**预期**状态。就绪性检查 gate 在 `doctor`，不在 `scan` 的退出码。

### 11.4 `explain` 的两种模式（Q24）

| 模式 | 行为 | provenance |
|---|---|---|
| `explain <id>` | 只重探 **owning adapter**。finding 消失时返回 typed `not_found_after_rescan`，**不回退扫描其他 adapter** | live |
| `explain <id> --from <file\|->` | 纯解析先前报告，**不访问文件系统或外部工具**。校验 schema 版本与 ID 算法版本 | `snapshot_only` |

`--from` 模式必须标记报告时间及「当前路径可能已变化」；`--path` 输出**报告捕获时**的路径，**不声称当前身份仍匹配**。

**snapshot replay 不得冒充当前验证。**

### 11.5 排除语义（Q20）

- `--exclude <path>` 可重复，排除**规范化子树**，**不支持 glob**（glob 容易产生用户以为被排除、实际未匹配的虚假安全感）。
- **在遍历/探测前生效。** 被排除子树内不得发生 `stat`、`getattrlist`、外部命令调用或 materialization。
- 不存在或未覆盖任何扫描根的 exclude 是 **usage error（退出码 2）**，不静默继续。
- 报告必须记录 excluded root 与 coverage 变化。
- **不写持久配置。**

---

## 12. 开发阶段（Q9）

**每个阶段必须满足其 Definition of Done 才可进入下一阶段。不允许并行推进或跳阶。**

| 阶段 | 内容 | Definition of Done |
|---|---|---|
| **P0** ✅ | 需求固化 | `decisions.md` 已产出，Q0–Q26 全部消解，frontier 为空 |
| **P1** | truth harness 与计量 schema | repo、CI 全绿、§10.3 五项自定义静态检查就位；空 side-effect registry、运行时计数器及 §10.2 的 1、2 号测试通过；此时无任何 adapter |
| **P1.1** | truth gate hardening | 五项自定义静态检查各有负向测试；零写与命令边界缺口关闭；side-effect registry 成为生产唯一来源；unsafe 唯一豁免边界与强化快照就位；此时仍无任何 adapter 或 FFI 实现 |
| **P1.2** | control-integrity hardening | HOME/TMP/XDG 隔离与越界变异测试；Clippy 禁用集合精确锁定；locked metadata 与生成文档漂移负向测试；macOS 15/26 deny-write sandbox 证明 §8.1 强声明；此时仍不进入 P2 |
| **P2** | read-only Root/fsx/capacity | §10.2 的 3、4、5 号测试通过（全部 APFS 反例）；plane 1 逐数字口径标注完成 |
| **P3** | typed adapter contract | 契约 trait 冻结；`not_present` / 未知版本降级路径有测试；adapter 的真实 probe 注册进 P1 已建立的 side-effect registry |
| **P4** | 两个深 adapter + CLI/JSON | Xcode/CoreSimulator、Homebrew；§10.2 全部 13 项通过；§10.4 人工验证完成 → **发布 v0.1 技术预览（schema 明确不稳定）** |
| **P5** | Docker adapter + 稳定化 | 第三个深 adapter；schema 冻结并文档化；完整口径文档；真机验收 → **发布 v1.0** |
| **v1.x** | 第四个 adapter | Go（`GOCACHE`、`GOMODCACHE`）+ 版本门控 |

**工期估算**（30 小时/人周，从零实现）：P1–P4 约 10–15 人周；P5 使总量达 22–28 人周。

**已永久移出范围：** TUI（约 3–6 人周）、写安全地基（20–27 人周）、adapter 写动作与撤销（6–9 人周）。全部塞入 v1 约 50–70 人周，对单人项目过大。

**为什么先发 v0.1 而不是等 v1.0：** 22–28 人周与 10–15 人周的差别是「今年发得出来」与「可能永远发不出来」，而**单人项目的支配性失效模式是永不发布**。计量契约的早期反馈不需要 Docker。truth harness、schema、Root/fsx 是共享的，故 v0.1 范围是 v1.0 范围的前缀。

**必须叫 v0.x 而非 v1：** 叫 v1 会错误暗示尚未挣得的 schema 稳定性。承认 schema 会变的预览不是虚假声明。

---

## 13. Skills 使用规定

| Skill | 使用时机 | 说明 |
|---|---|---|
| `/ponytail`（full） | P1–P5 全程常驻 | 抵抗过度设计 |
| `/ponytail-review` | 每阶段 diff 完成后、合入前 | 针对本阶段 diff |
| `/ponytail-audit` | P5 之前一次 | 全仓库审计，处理排名靠前条目 |
| `/ponytail-debt` | 每阶段结束 | 确认 `ponytail:` 标记没被永久搁置 |

> **⚠️ ponytail 的适用边界**
>
> 以下内容**明确豁免于 YAGNI 简化**，不得以「精简」为由削减：
>
> - §8 全部只读契约与副作用闸门
> - §2 全部计量口径与区间边界规则
> - §9 truth contract 的 CI 机械化
> - §10.2 列出的 13 个测试
> - 每条规则的 `evidence` 字段与派生 selection policy
>
> ponytail 应作用于 **格式化代码、CLI 样板、抽象层次、依赖选择**。与本节冲突时本节优先。

`/grill-me`：已随仓库提供于 `.agents/skills/`（`grill-me` + `grilling`）。**只能由用户手动触发**（`disable-model-invocation: true`）。P0 已完成；若引入新的重大不确定性可再次运行。

`grilling` 要求「事实由 Agent 自己查，决策才问用户」—— 凡属事实性问题（macOS 各版本路径、外部工具行为、属性可用性）应派 sub-agent 查证，不要反问维护者。

`/tdd`（严格红-绿-重构）与 `/diagnose`（纪律化调试）对 P1、P2 有直接价值。

### 工程约定

- **测试先行**：`policy`、`fsx`、`model` 三层采用 TDD —— 先写失败测试再写实现。
- **提交粒度**：一个提交一个逻辑变更，commit message 说明「为什么」。
- **禁止**：无测试的 `policy`/`fsx`/`model` 代码合入；非测试代码出现 `unwrap()`（clippy 强制）；叙述性代码注释（注释只写代码无法表达的约束与取舍）。
- 发现 spec 有误：**先更新 `decisions.md`（决策变更）或本文（表达错误）并说明，再改代码。** 不允许代码与规格静默分叉。

---

## 14. 风险登记

### 14.1 技术与产品风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| 用户期待清理却只得到解释 | 「找到 60GB 然后什么都做不了」的负面反馈 | Q7 已记录这是刻意选择；每个 observe-only 项必须给出该工具链官方的确切命令 —— 把「碰不了」转为「教你怎么办」 |
| TCC 导致扫描不完整 | 归因失真、用户不信任 | §3.1 typed `permission_denied` + 退出码 3 + `doctor` 指引；**绝不静默报 0** |
| iCloud materialization | **违反只读契约，产生真实下载与写入** | §3.2 强制 `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` 并验证；失败即记 unknown |
| adapter 包装的第三方 CLI 输出格式变化 | 解析失败或**静默误读** | §5.2 钉住已验证版本范围 + 未知版本显式降级。**这是本架构最大的长期维护风险** |
| macOS 大版本挪动路径 | 规则失效 | §6 规则数据化 + `os` 门控 + 每次大版本后回归 |
| 区间过宽导致结论无用 | 用户看到「0 到 20GB」而无从判断 | §2.4 信号解释宽度；`filesystem_compressed` 明确展示为「private floor 不提供信息」 |
| truth contract 腐烂 | 由弱承诺变为**虚假的强承诺** | §9 必须机械化为 CI，不得是人工清单 |
| 单人项目永不发布 | 项目致命 | §12 先发 v0.1 双 adapter 预览；已永久移出 TUI 与写路径 |
| 命名首次接触被误解 | 传播损失 | 已在 Q26 用「冷读可读性」这条筛选终局；`SizeTrail` 含 size 指向领域 |
| 无护城河 | mole 若实现同一契约则差异消失 | 承认它（§1.2）。靠聚焦与执行质量存在，不宣称独占功能 |

### 14.2 与 mole 的关系（许可与伦理）

**mole 是 GPL-3.0，已有 65k star。**

1. **不得复制 mole 的任何代码**（含大段改写），否则 SizeTrail 必须整体转为 GPL-3.0。交互模型与信息架构的**理念**借鉴不构成侵权，但具体代码、文案措辞、配色不得照搬。
2. mole 的 README 要求派生产品换名并注明来源。SizeTrail 已换名；README 应致谢 mole 为灵感来源。

---

## 附录 — 实测基线与 APFS 反例

见 `decisions.md` 附录 A（实测环境基线）与附录 B（必须进 fixture 的 APFS 反例清单）。

`probe_attrs.c` 是 fixture 的起点。正式版本**必须**使用 `FSOPT_PACK_INVAL_ATTRS`（或动态解析 buffer）并逐项检查 returned mask —— 固定结构在属性缺失时会字段错位。
