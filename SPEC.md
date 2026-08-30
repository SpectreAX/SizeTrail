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
| v0.1 范围 | Xcode/CoreSimulator 单 adapter，schema 明确不稳定（Q29） |
| v0.2 范围 | 追加 Homebrew adapter |
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

**P2 的机器可读口径表：** 每个已测数字在 JSON 中携带以下 `basis`；未测量项不伪造数字或近似 basis。

| 数字 | 主口径 | 回退 / 能力门控 |
|---|---|---|
| container allocated | `statfs: (f_blocks - f_bfree) × f_bsize` | 仅在 `VOL_CAP_FMT_SHARED_SPACE` 的 capability 与 valid 位同时存在时称为 container；否则 unmeasurable |
| volume size | `getattrlist: ATTR_VOL_SIZE` | `statfs: f_blocks × f_bsize` |
| volume used | `getattrlist: ATTR_VOL_SPACEUSED` | 仅当前者未返回时用 `statfs: (f_blocks - f_bfree) × f_bsize`，JSON 明示回退 |
| volume free | `getattrlist: ATTR_VOL_SPACEFREE` | `statfs: f_bfree × f_bsize` |
| normal available | `getattrlist: ATTR_VOL_SPACEAVAIL` | `statfs: f_bavail × f_bsize` |
| important available | CoreFoundation `kCFURLVolumeAvailableCapacityForImportantUsageKey` | 属性不可用即 unmeasurable |
| opportunistic available | CoreFoundation `kCFURLVolumeAvailableCapacityForOpportunisticUsageKey` | 属性不可用即 unmeasurable |

`container allocated` 与 `volume used` 即使数值偶然相等也不得合并；它们的 scope 与 basis 不同。

**v1 measurement 还必须显式携带 `quantity`（Q56）。** `quantity` 回答「测的是什么」，
`basis` 回答「怎样测得」；两者不得互相代替。Docker 至少使用
`disk_image_logical_limit`、`host_allocated_footprint`、`daemon_used`、
`daemon_reclaimable`、`object_count`、`active_object_count`。Docker CLI 的 human-size
字符串使用 `rounded_bytes { reported, lower_bound_bytes, upper_bound_bytes }`，不得转写为
`exact_bytes`。只有原始整数来源才可用 exact value。

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

JSON 中该区间的 `basis` 必须是显式的 `private_floor_allocated_ceiling`；不得把它标成
单一 `private_size` 或 `allocated_footprint`，因为两端来自不同计量口径。
区间值同时携带 `applicable_action = permanent_unlink_after_references_close`。P4 未
建立扫描前后 link/clone/snapshot 稳定性证明，故 v0.1 的 floor **一律为 0**（Q40）；
不得把一次读取过程或负信号当作并发稳定证据。

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

卷级快照事实通过公开 `fs_snapshot_list(2)` 瞬时枚举，不运行 `diskutil`。实现先以
`statfs(scan_root).f_mntonname` 找到真实 mount root，打开后用 `fstatfs(fd).f_fsid`
回验，再读取首批结果；返回值大于 0 已足以证明“存在”，无需枚举或保存名称。任一步失败都产生
`volume_snapshot_state_unavailable`，不得当作“无快照”。该事实只说明枚举瞬间卷上
是否存在快照，不提供 extent → snapshot 归因，也不构成扫描期间稳定性证明。hosted
runner 只要求验证“枚举成功（允许 0 条）”与非 mount-root 的 `EINVAL`；至少一条快照
的正例由维护者普通用户机器实测，不能被一份通常为 0 条的绿色 hosted suite 冒充。

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

**硬性要求：必须区分「这里是 0 字节」和「我看不见这里」。**

- 对每个实际扫描 target 执行与 scan 同构的只读探测；记录具体 `target`、失败
  `stage` 与原始 `errno`。
- `ENOENT` 是 absent/changed，`EACCES` 是 access denied，`EPERM` 是来源未知的
  policy denial；**不得把任一单独错误写成全局 FDA 状态。**
- `stat` / `access` 成功不代表目录枚举或属性读取成功。诊断必须以实际读取阶段为准。
- 被拒绝时使用 typed capability status，**不得静默报 0**。
- 映射到退出码 3（§11.3），属**信息性**而非错误。
- `sizetrail doctor` 输出逐 target 的读取能力与修复指引；不读取 Mail、Safari、TCC
  数据库或任何非扫描目标来猜测权限。
- macOS 没有供普通进程可靠查询全局 FDA 状态的公开 API。`TERM_PROGRAM`、父进程
  与 bundle id 只能输出为未验证的 launcher hint，不能作为授权主体证据。
- 可打印打开系统设置的命令供用户自行执行；SizeTrail 永不执行该命令。

### 3.2 iCloud / File Provider（严格约束）

**读取 `SF_DATALESS` 前的元数据调用本身就可能 materialize 文件** —— 这会导致真实的网络下载与磁盘写入，直接违反只读契约。

- 扫描前**必须**设置并**验证** `IOPOL_MATERIALIZE_DATALESS_FILES_OFF`。
- 设置或验证失败 → 整个相关 root 记为 `unknown`，不继续探测。
- `EDEADLK` 同样记为 `unknown`。
- **Cloud / File Provider roots 永久排除**，不进入任何外部工具调用。
- **IOPOL 验证失败必须在 JSON 上与路径类失败可区分（Q33）。** 折叠成单一原因字符串等于让红线 6 不可审计。

### 3.2.1 root 路径策略（Q32）

`Root::open` 的固定顺序，**顺序本身是约束**：

1. 给定路径做纯文本检查：绝对、无 `.` / `..`、非 cloud 前缀（不触碰文件系统）。
2. 设置并验证 I/O policy。
3. **规范化 root 一次** —— `fs::canonicalize` 是元数据调用，可能触发 materialization，因此**必须**在第 2 步之后。
4. 按物理路径重做 cloud 前缀检查（防 symlink 指入 `CloudStorage`）。
5. 取物理 fsid/fileid 作为 root 身份。

规范化后同时保留给定路径与物理路径。**cloud 排除与 mount/firmlink 边界一律以物理路径与真实 fsid 判定；`measure_object` 的前缀检查是物理前缀，不是文本前缀。**

**root 以下的遍历继续使用 `FSOPT_NOFOLLOW_ANY`。** 不得对 root 自身施加 —— 那会使 `/tmp` 与 `$TMPDIR`（macOS 上均经 symlink）不可用，而它保护的只是 root 的命名方式，不是遍历期的逃逸面。

### 3.3 SIP

SIP 默认启用。把它当免费安全网，但不得依赖它作为唯一防线（用户可能关闭）。永久只读后本项影响已大幅下降。

### 3.4 依赖的系统工具

已确认可用：`tmutil`、`brctl`、`mdutil`、`diskutil`、`xcrun`。`fd` 不存在 → 不得依赖；
v0.1 使用 §4 的受控串行遍历，不为未经测量的并发收益绕过 `Root` 边界。

**所有外部命令必须：** 由 policy registry 固定绝对 executable、参数、环境与调用上限 → 缺失时记 `not_present`（退出码 0，非失败）→ 版本落在已验证范围外时显式降级 → 绝不 panic。不得额外依赖 PATH-sensitive `which`；developer-tool shim 的文件存在也不构成工具链可用证据（Q35）。

只允许**闭集**的只读命令。见 §8.2。

---

## 4. 技术选型

| 关注点 | 选择 | 理由 |
|---|---|---|
| 语言 | Rust stable | 单二进制无运行时；类型系统可静态排除写路径 |
| CLI | `clap` builder API | 标准选择，自带 completion 生成；不用 derive，因为 derive 宏会注入 lint `allow`，与 crate root 的 `forbid(clippy::disallowed_methods)` 冲突。不得仅为减少样板而重新启用 derive |
| 遍历 | `std::fs::read_dir` 经 `Root` 串行调度 | v0.1 先保证 exclude-before-probe、mount/firmlink 与 materialization gate 不被第三方 walker 绕过；CI 记录真实 benchmark，只有数据证明需要时再为并发重新建模这些边界 |
| 序列化 | `serde` + `serde_json` + `toml` | 规则表 TOML，输出 JSON |
| 错误 | `std::io::Error` + typed enums | 当前错误面无需额外依赖 |
| 诊断 | 直接写 stderr | 不写日志文件，不引入 logging framework |
| 测试 | `cargo test` + `assert_cmd` + `tempfile` + `xattr` | 见 §10 |

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
    fn probe(&self, ctx: &mut PolicyCtx) -> AdapterState;

    /// 枚举 store 对象。只读。
    fn inventory(&self, ctx: &mut PolicyCtx, state: &AdapterState) -> Inventory;

    /// 归类：action/mechanism、recoverability、sensitivity（Q4）。
    fn classify(&self, inv: &Inventory) -> Result<Vec<Finding>, InventoryGapReason>;

    /// 生成建议。永不执行。
    fn advise(&self, f: &Finding) -> Vec<Advice>;
}
```

`PolicyCtx` 显式使用可变借用，因为每次外部 probe 都必须更新 P1 的运行时调用计数器。
不得用内部可变性隐藏这一副作用状态，也不得让 adapter 绕过 context 自行起进程。
同一次扫描所得的 `AdapterState` 必须显式传给 `inventory`（Q36）；每个 adapter 每次
扫描只运行一次 `probe`，不得重复探测或在 adapter 内缓存隐式状态。

**没有 `execute`。** 契约就是 `probe → inventory → classify → advise`（Q7 + Q11）。

**adapter 纪律（Q8）：**

- v1 只上 **3 个全深度** adapter：Xcode/CoreSimulator、Homebrew、Docker Desktop。**浅 adapter 比没有更差** —— 它把 mole 的 feature-local 问题搬进我们自己的架构。
- 每个 adapter **必须钉住已验证的第三方 CLI 版本范围**，未知版本显式降级。这是最大的长期维护风险：adapter 包装的是第三方 CLI，输出格式会变。
- 规则**只能引用已编译的 adapter id**，不能提供任意命令。**这是安全属性，不是架构品味** —— 允许 TOML 携带任意命令等于开命令注入面。

**P3 Xcode probe（Q35）：** registry 仅含 `xcode-select -p`、`xcodebuild -version`、`xcodebuild -checkFirstLaunchStatus` 三条绝对只读命令，各最多一次，共用 `SIZETRAIL_NO_XCODE_PROBE`。固定 C locale 并移除 `DEVELOPER_DIR` / `SDKROOT` / `TOOLCHAINS` 与已知 `xcrun_*` 重定向变量。标准 CLT selection 是 `not_present`；未知版本（诊断同时保留 version + build id）与未完成 first-launch/license 是不同 degraded reason。P3 不运行 `xcrun` 或 `simctl`。当前 hosted 精确验证对为 `16.4 (16F6)` 与 `26.6 (17F113)`；矩阵漂移必须令测试失败并显式更新。

**P4 CoreSimulator inventory（Q39、Q43）：** 永不执行 `xcrun simctl` 或 Xcode 的
`usr/bin/simctl` wrapper。registry 先以固定 `PlistBuddy Print :CFBundleVersion` probe
读取全局 CoreSimulator 版本；只有它与已验证 Xcode version/build 的精确映射相等，才直调
固定 `PrivateFrameworks/.../Resources/bin/simctl` 的 devices/runtimes JSON，各最多一次、
硬超时 30 秒。版本不等、unknown/not-ready、probe failure、timeout、malformed JSON 全部
typed gap，且不等时两个 simctl 调用均为 0。**但版本不等不得丢弃 device set 的字节（Q45）：**
计量本身是文件系统的，只有枚举与身份识别需要该 binary，因此版本不等时仍按
`xcode.simulator_device` 已声明的 `paths` 静态展开并计量每个 device set，另附
`simulator_identity_unavailable` typed gap 说明设备名、runtime 关联与 availability 不可得。
不得由 UUID 目录名伪造身份。runtime 继续受门控：它们位于 root 之外且区分 runtime 与其承载
卷需要 simctl，这是刻意边界。直调路径是 technical preview 的私有入口，
必须由 hosted exact-version lane 证明，任何路径、文件类型或版本漂移 fail closed。直接
child，不 `pkill` daemon、不重试；它可能启动/连接 CoreSimulatorService 与
simdiskimaged，必须进入 side-effect registry 并提供同一个关闭开关。用户排除完整
Devices root 时 devices probe 调用为 0。simctl stderr 原文只写 stderr，payload 只保留
稳定 typed warning。advice 可展示 `xcrun simctl`，但 SizeTrail 不执行。

**P5 Docker Desktop（Q55、Q56）：** Docker daemon 的 images、containers、volumes 与
BuildKit cache 是 typed object set，不是可伪造为 `Docker.raw` 的文件路径。adapter 只通过
Docker.app 内绝对 CLI 执行三条闭集 probe：context inspect、version JSON、system-df JSON；
每条每次扫描至多一次，共用 `SIZETRAIL_NO_DOCKER_PROBE`。连接 daemon 前必须验证
`desktop-linux` endpoint 为当前 HOME 下的 Docker Desktop Unix socket；清除
`DOCKER_HOST`、`DOCKER_CONTEXT`、`DOCKER_CONFIG`、`DOCKER_API_VERSION`、
`DOCKER_CERT_PATH`、`DOCKER_TLS_VERIFY` 等重定向输入。unknown version 或 endpoint mismatch
均 fail closed，且 system-df 调用数为 0。

`system df --format json` 的 size/reclaimable 是 vendor 舍入字符串，按 Q56 输出
`rounded_bytes`，basis 为 `docker_system_df`；禁止标为 exact。该命令可能唤醒 Resource
Saver，并会遍历 daemon 中的 image/container/volume filesystems，二者均登记为已知读副作用。
静态 VM disk image 计量不依赖 daemon probe 成功，且不得与 daemon 数字求和或相减。

### 5.3 验证矩阵（Q12）

**API/deployment target 与验证矩阵是分离的两件事。**

| 目标 | 状态 |
|---|---|
| deployment / API target | macOS 13 |
| P1–P3 runtime 验证 | macOS 15、26 标准 arm64 lane（Q30） |
| Intel runtime | P4 加入；此前仅构建 x86_64 产物并检查 deployment target，不声称运行时已验证 |
| macOS 14 arm64 | 仅在 `macos-14` runner 存续期间验证（2026-11-02 下线），之后自动移出矩阵。Intel 需 `macos-14-large` |
| `xcode-27` lane | P4 加入；**只证明 macOS 26 上的工具链/SDK 兼容性。** 它是运行在 macOS 26 arm64 上的 SDK preview，不是 macOS 27 runtime；GitHub 当前无 `macos-27` runner |
| macOS 13 | best-effort，未经 hosted runtime CI 验证 |

- CI 检查两种架构产物的 **Mach-O minimum OS version**。
- **支持表由实际 CI matrix 生成**，不静态声称永久版本列表。
- 结构性约束：最多两个 GA + 一个 beta，自动轮换。

#### 真实环境 lane（Q44）

除上述 lane 外，另有一个 **non-blocking** lane 在 runner 的真实 `$HOME` 上执行真实 scan，取证 fixture 无法覆盖的环境形态（hosted image 预建的 CoreSimulator device set 与 runtime 由 Apple 生成，不是为本项目生成）。

**它必须保持 non-blocking。** hosted image 的 Xcode 版本是移动目标；若该 lane required，版本轮换会让 `main` 变红，而当时最省事的修法就是放宽 §4 的精确版本门控。**不得让 CI 压力具备侵蚀安全门禁的能力。**

**永久覆盖边界：`iOS DeviceSupport` 在 hosted runner 上永久不可构造**（它只来自连接真实 iOS 设备），永久停留在 fixture 证据。这是边界，不是待办。

### 5.4 目录布局

```
src/
  main.rs
  model.rs       finding / signal / advice / interval
  scan.rs        report composition
  adapters/      mod.rs（契约）+ xcode.rs
  rules/         mod.rs + builtin/*.toml
  capacity.rs    plane 1 容量事实与口径标注
  fsx/           mod.rs（Root）+ sys.rs（唯一 unsafe/FFI 边界）
  policy.rs  ★   外部命令闸门 + registry
tests/
  support/       伪造 HOME 树与零写快照器
  fixtures/      checked-in Xcode HOME / rule / APFS evidence fixtures
  apfs_*.rs      §2 计量正确性测试（含 decisions.md 附录 B 反例）
  policy_*.rs    ★ 零写与副作用上限测试
  cli_*.rs       端到端（全部走 --root）
  scan_json.rs   payload 逐字节与部分失败契约
```

**`--root` 沙箱保留。** 永久只读后它不再是删除防护，而是**测试注入机制** —— 引擎全程只通过可注入的 `Root` 抽象访问文件系统，使 fixture 测试无需真实 HOME。这条能力是 §10 测试策略成立的前提。

Root 的路径读取以 `FSOPT_NOFOLLOW_ANY` 拒绝任意位置的 symlink（该 flag 已存在于 Ventura 的 XNU 8792），不能仅靠词法 `starts_with`；后者无法阻止 `root/link/outside` 在同一物理卷上越界。每个成功读取的对象仍须比较真实 fsid，以独立拒绝 nested mount 与 System/Data firmlink 边界。

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
fixture_id = "xcode-derived-data"

[rule.preconditions]
process_not_running = ["Xcode"]        # owner 进程在跑则标注状态
```

### 6.2 硬性约束

- **`selection policy` 必须派生，不得存储（Q4）。** 任何「默认呈现强度」都是 `(recoverability, sensitivity, precondition)` 的**纯函数**；需要例外时必须填 `override_reason`，使 schema 本身可校验。存成字段则规则作者可在高 sensitivity 项上直接置真 —— 那正是 T0–T4 想防的错误换地方复发。
- **白名单语义。** 只有匹配规则的路径才被测量。**禁止**「X 下除 Y 以外全部」的黑名单写法。
- 每条规则必须有**非空 `evidence`**。写不出「删了会怎样」的规则不允许合入。
- 每条规则必须有至少一个对应 fixture 测试。
- fixture 必须断言 rule id、正交分类与至少一个期望匹配路径，不能只是同名空文件。
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

### 7.2 Finding subject 与 ID（Q24、Q55）

finding 的定位是 tagged union：

```text
filesystem_path { normalized_path }
toolchain_object_set { object_set_id }
```

内置规则使用同构的 tagged subject pattern。`filesystem_path` 的 pattern 可展开文件系统
对象；`toolchain_object_set` 只能由对应的 compiled adapter 产生，不能携带命令或路径。
`explain --path` 只接受 filesystem subject；object set 必须明确返回“没有文件系统路径”。

```text
f1:<adapter_id>:<digest>
```

- `digest` 由**版本化算法**根据 `adapter_id + rule_id + canonical subject key` 派生。
- filesystem subject 的 key 仍是 **HOME 相对**的 normalized path，保持既有 f1 ID 不变。
- toolchain object set 的 key 带显式类型前缀，不能与真实路径碰撞。
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

**Reveal 只适用于 filesystem subject 并只打印路径（Q14、Q55）。** Finder reveal 可能启动 File Provider、枚举目录、生成缩略图并写 Finder 自身状态，不能混入零副作用契约。路径是一等机器输出，用户自行组合 `open -R`。toolchain object set 只能得到 adapter 的 typed command advice 与解释，不能伪造 Reveal 路径。

---

## 8. 只读契约（本项目最重要的一节）

**SizeTrail 永不删除、移动、驱逐、thin、prune 或以任何形式修改用户/系统数据。**

这是**永久产品边界**，不是发布切片（Q11）。**本节任何要求都不适用 ponytail 的 YAGNI 简化。**

### 8.1 零写不变量

1. 整个 crate 不存在 `fs::write`、`fs::remove_*`、`fs::rename`、`File::create`、`OpenOptions::write/append/create`。唯一输出是 stdout / stderr。`src/lib.rs` 与 `src/main.rs` 两个独立 crate root 必须以 `forbid(clippy::disallowed_methods)` 锁死该约束；`disallowed_types` 与 `unsafe_code` 因各有唯一合法豁免点，保持 `deny` 并由边界脚本限制豁免位置。非测试代码没有合法 `unwrap()` 豁免，故两根同时 `forbid(clippy::unwrap_used)`。
2. 无操作日志文件、无缓存文件、无配置文件、无 `completion` 写盘（Q17 / Q20 / Q22）。
3. **可机械验证的安全属性：SizeTrail 进程自身正常运行不向用户或系统数据路径发起任何写操作。** CI 静态 API 门禁 + 重定向写入位置环境变量的运行时快照 + macOS 15/26 上 `sandbox-exec` 的 `(deny file-write*)` 系统调用级强制，共同验证 §10.2 第 1、2 条。唯一例外是 dyld 注册 DOF 时对字符设备 `/dev/dtracehelper` 的 `file-write-data`（Q34）；profile 以 literal path + exact operation 窄允许，其他写继续全部拒绝。deny 规则必须携带每次运行唯一的 message token，并由 unified log 实时观察器断言 scan token 的其他违规记录为零；仅断言 scan 成功或文件未出现不够，因为产品可能吞掉 `EPERM`。START/END 必拒哨兵分别证明观察器已接通、事件已排空，并必须在同一事件中同时匹配 token、`file-write-create` 与精确目标路径。sandbox 门禁必须 fail closed；若目标 runner 不再支持该 deprecated 机制，必须先通过 decision record 重定证据或收窄声明，不得静默删除门禁。该门禁以 `SIZETRAIL_NO_XCODE_PROBE=1` 关闭外部命令，使继承 Seatbelt 的 vendor child 不与产品进程混算；同时仍断言 capacity complete，故不能以关闭 probe 跳过产品测量。外部工具继续由 §8.2 的 registry 与真实 hosted full-scan 测试取证。该证据不覆盖 sandbox 前已打开的额外 fd，也不覆盖外部 child 或 IPC 请求未沙箱化 daemon 改状态。
4. **`src/fsx/sys.rs` 是唯一 unsafe 豁免点。** 该模块只导出只读系统调用包装，公开签名不得暴露可写句柄、可变缓冲区、写入 flag 或执行任意系统调用的能力；其他文件不得抑制 `unsafe_code` lint。所有系统调用返回值与 `errno` 必须检查并向上返回，禁止用 `let _ =` 或等价形式丢弃可能表示写尝试或调用失败的结果。两个 crate root 以 `forbid(clippy::let_underscore_must_use, clippy::let_underscore_untyped)` 机械拦截 `let _ = Result` 与未标类型的 raw status；这些 lint **不能**识别写语义，也拦不住显式类型的 raw integer 或裸调用，故只作为契约的局部机械化，不能替代代码审查与运行时证据。模块内部无法由 Clippy 的 Rust API 禁用表证明零写，**其零写保证明确且仅由 §10.2 第 1 条运行时 harness 覆盖**。P1.1 只建立此边界，不实现 `getattrlist`。
5. 当前 crate **没有 `build.rs`**。build script 是独立 crate；未来若引入，必须同样设置 `forbid(clippy::disallowed_methods)`，纳入 lint suppression 边界与系统调用级零写测试。未同时补齐这些控制前，禁止新增 `build.rs`。

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

**side-effect registry（Q17）：** 记录每个 probe 每次扫描的**最大调用次数**、关闭开关、
硬 timeout 与已知副作用。registry 是数据，测试断言实际调用次数不超过声明值；timeout
必须终止并回收子进程，且映射为 typed `unmeasurable`，不能令完整 JSON 缺失。
P4 hosted 实测证明 `xcrun`/Xcode simctl wrapper 会写 cache/tty，且在 CoreSimulator
版本不等时明文调用 `xcodebuild -runFirstLaunch`；因此 Q43 禁止执行 wrapper。生产只在
固定 `PlistBuddy Print` probe 与已验证版本精确相等后直调固定 global simctl binary。
registry/`doctor` 必须公开 direct simctl 可能启动或连接 CoreSimulator daemon；不能把
“inventory 子命令语义只读”偷换成“未沙箱化 daemon 没有状态变化”。

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

### 9.0 门禁有效性（Q31，适用于本规格所有门禁）

**任何用于证明某能力的门禁，必须同时断言该能力确实执行了。只断言「没有失败」的门禁视为无效门禁。**

只读产品的正常行为就是「什么都不做」，因此「什么都没发生」与「什么都没执行」在退出码上不可区分 —— 这使 fail-open 成为本项目门禁的**主要**失效模式，而非边缘情况。P2 审计一次性发现三处同型缺陷（沙箱 probe root 因 symlink 被拒、沙箱不断言已测量、反例构造失败静默 skip）。

派生规则：

- 构造失败一律 **fail closed**。例外只能进入代码内的窄允许清单，并由锁定测试保证清单变更可见。
- `--nocapture` 保留诊断输出**不构成**证据 —— 绿色状态是唯一会被读取的信号。
- 每个新门禁必须回答：什么情况下它会在能力未执行时保持绿色？答案必须是「没有」。

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

因此 benchmark 资产**必须自带 runner 身份**（`ImageOS`、`ImageVersion`、架构）：它是独立发布的文件，一旦与 artifact 名分离，缺身份就无法归因也无法察觉 image 轮换（Q46）。CI 下这些变量缺失即 fail closed。

**release notes 必须是仓库文件 `docs/release-notes/<tag>.md`，由 `--notes-file` 发布（Q46）。** 自动生成的 notes 不在仓库里，§9.1 的两道门禁原理上无法检查它 —— 那是把诚实寄托在维护者克制上，Q5 已否决该形态。CI contract 断言 release workflow 不含 `--generate-notes`：未被实际使用的门禁是装饰（§9.0）。

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

1. **零写测试** ★ — HOME、TMPDIR/TMP/TEMP 与 XDG 写入位置全部重定向到 fixture 快照根内；全量 scan 后断言 `--root` 前缀内外均无任何文件被创建、修改或删除（对比前后完整 inode + nlink + mtime + ctime + size + xattr 名称和值快照）。另对 `/tmp`、`/var/tmp` 与真实 HOME 下的 `Library/Logs`、`Library/Caches`、`Library/Preferences`、`Library/Application Support` 做浅层新条目兜底；它有意不覆盖既有条目修改或任意绝对路径。CI 在 macOS 15/26 以 deny-write sandbox 执行完整 scan：只允许 Q34 的 `/dev/dtracehelper` exact operation，START/END 哨兵逐项匹配 token + operation + path，并断言 scan 的其他拒绝事件为零，证明产品没有吞掉失败的用户/系统数据写尝试。
2. **副作用上限测试** ★ — 断言每个 probe 的实际外部命令调用次数不超过 side-effect registry 声明值；断言未声明的命令从未被调用。
3. **APFS 反例测试** — `decisions.md` 附录 B 每个反例各一个测试：clone 双计、resource-fork-only（`allocated=2MiB, private=0`）、HFS 压缩（先创建 CPIO，再以 `ditto -x --hfsCompression` 构造）、hardlink 未完整覆盖时 floor 归零、稀疏文件只解释 logical gap。**构造失败一律 fail closed（Q31）** —— 测试必须失败，不得打印标记后 `return`。若某 runner 确实无法构造某反例，只能把该 fixture id 加入代码内的窄允许清单，并由锁定测试使清单变更可见；`--nocapture` 保留诊断输出不构成证据。
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
14. **真实环境测试（Q44，non-blocking lane 专用）** — 在 runner 真实 `$HOME` 上执行真实 scan，拆成两条互不替代的断言。**A 文件系统侧归因（按类别，不留 gap 逃逸口）**：磁盘上存在 device 目录时该类别必须被计量。「至少一条 finding」曾在 133 个 device set 缺失时通过；改为「被报告或有 typed gap」后，又把 `core_simulator_version_mismatch` 当成覆盖了那 133 个目录，放过了通配展开被兄弟文件中断的真实缺陷（Q45）。这些字节不需要版本门控 probe，零即缺陷。**B 版本门控降级**：版本不匹配时必须干净降级为 `unknown_version`，既不使 scan 失败，也不执行 simctl wrapper。两条禁止：**不得断言字节值**（真实体积非确定，只断言 `floor ≤ ceiling`、区间不收敛、每个数字带 basis、无跨 basis 求和、typed gap 合法，且不进第 6 项的逐字节 payload fixture）；**不得用作零写门禁**（Xcode/CoreSimulator 后台服务会独立修改这些目录，快照断言按构造 flaky，而 flaky 门禁的结局是被关掉且声明留下）。

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
# crate root 强度：lib/bin 均 forbid disallowed_methods；该 lint 在 src/ 内零豁免
# 系统调用级零写：macOS 15/26 均以 sandbox-exec 执行 scan；除 Q34 的精确 dyld 设备例外外，断言唯一 token 的 deny 事件为零；不可用即失败
#   该门禁必须额外断言 scan 实际完成了测量（§9.0）：probe root 取物理路径，
#   且断言 capacity region 为 complete。否则 root 初始化普遍失败时门禁仍会全绿。
```

矩阵见 §5.3。**支持表由该 workflow 生成，不手写。**

### 10.4 发布前人工验证

自动化测试无法覆盖真实 macOS 的全部权限与路径现实。每个 minor 发布前，在 required
hosted lane 的临时普通用户账户与维护者普通用户账户上共同执行；两类证据的边界不得
互相冒充：

1. `sizetrail doctor` — 在 SizeTrail 的真实 target 上分别验证可读与策略拒绝；核对
   target、stage、errno 与 typed status。**不把测试结果改写为全局 FDA 状态**
2. `sizetrail scan` — 三平面数字的口径标注是否正确、coverage gap 是否诚实
3. `sizetrail scan --json | jq` — schema 完整性
4. 仅选中 Command Line Tools、无完整 Xcode 的机器上 — 断言 Xcode `not_present` 且退出码 0
5. 有本地快照的机器上 — 实测 `fs_snapshot_list` 正例，并与强制 snapshot=true 的
   分类 fixture 共同断言 typed signal 与区间表述；**不得把这两段组合证据描述成
   hosted end-to-end 正例，也不得虚构具体快照归因**
6. required hosted lane 对关闭 registry 外部 probe 的 SizeTrail 进程运行 deny-write
   Seatbelt 门禁；真实 full scan 另行验证 registry 命令与已知副作用。fixture 同时运行
   TreeSnapshot 与高价值真实路径兜底。两者只覆盖调用进程，未沙箱化 daemon 的空格
   继续留在 §12.1

---

## 11. CLI 规格

### 11.1 命令表面（Q22）

```text
sizetrail                            显示帮助，不自动探测
sizetrail scan [--json]              只读归因报告
sizetrail explain <finding-id>       解释单个 finding
       [--json | --path] [--from <file|->]
sizetrail doctor [--json]            target 读取能力、工具版本、side-effect gate 诊断
sizetrail rules [--json]             查看内置规则表
sizetrail completion <shell>         生成补全脚本（仅打印 stdout）
```

`scan` / `doctor` 的开关：`--no-xcode`、`--no-homebrew`、`--no-docker`、`--exclude <path>`（可重复）。

全局：`--debug`、`--no-color`、`--version`、`--root <path>`（测试用）。

`--version` 报告构建版本（Q48）。`scan` 与 `doctor` 文档在顶层携带 `tool_version`，与 `schema_version` 并列 —— 二者演进节奏不同，v0.1.1 的 schema 未变而构建已变。它不属于 `environment`（与主机无关，不参与 fixture 环境注入），也不属于 payload（不是被计量的数据）。`--version` 不读文件系统、不起子进程，因此不受下述探测约束。

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
| `0` | 所有已启用且适用的 region 完成。`not_present`、`excluded_by_user` **以及仅含 `declared_scope_boundary` gap 的 `complete` region** 均为 0（Q54） |
| `1` | 无法初始化或无法形成合法文档的 fatal error。**窗口仅限初始化前失败**，实践中应罕见 |
| `2` | CLI usage error（clap）。**不产生 scan 文档**。无效 `--exclude` 属此类 |
| `3` | 产生了完整文档，但至少一个适用 region 因权限、未知版本、解析失败、超时等**环境性**原因未测量。已声明的永久范围边界不是此类 |

每个 region 使用 typed status；stderr 仅作人类诊断。

**退出码 3 属信息性**，目标路径受策略拒绝时为**预期**状态。就绪性检查 gate 在
`doctor`，不在 `scan` 的退出码。

### 11.4 `explain` 的两种模式（Q24）

| 模式 | 行为 | provenance |
|---|---|---|
| `explain <id>` | 只重探 **owning adapter**。finding 消失时返回 typed `not_found_after_rescan`，**不回退扫描其他 adapter** | live |
| `explain <id> --from <file\|->` | 只读取用户显式提供的报告输入；不重探报告路径、不运行 adapter 或外部工具。file 模式除该报告文件外不访问文件系统；stdin 模式只读 stdin。校验 schema 版本与 ID 算法版本；`--path` 对 object-set subject 显式失败 | `snapshot_only` |

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

**通道覆盖矩阵是 P2 起的常设交付物。** P2 起每个阶段及阶段内小节完成前都必须基于阶段后代码重新推导矩阵，并为本次新开的通道增加行，不能只沿用上一版结论。P2 至少逐项列出 `fsx/sys.rs` 内每个 `extern` 声明，以及读操作诱发的系统代写；P3 至少逐项列出每个外部命令、子进程与未沙箱化 daemon 的状态变化通道；P4 继续加入每条 `simctl`、规则解析、显式报告文件读取、completion 与新增依赖的运行时通道；P5 逐小节加入 Docker CLI、配置读取、VM metadata 与 daemon IPC。

### 12.1 当前 P5.1 通道覆盖矩阵

`静态`包含 crate-root forbid、豁免边界、Clippy 清单锁；`符号锁`包含 extern 精确集合、无直接 `libc`、无 build script、禁止 inline asm/dynamic lookup；`运行时`包含 TreeSnapshot、高价值路径兜底与 deny-write sandbox；`专用`是本行的行为/差分 fixture。`—` 表示该层不覆盖，不能据此扩张安全声明。

**deny-write sandbox 逐子命令观测（Q49）。** 它对二进制宣告的每个子命令（`scan`、`doctor`、`rules`、`completion`、`explain`）以及 `--version` 各起一次独立运行，每次使用自己的 violation token，并各带一条 fail-closed 的「确实做了工作」断言 —— 只证明 `scan` 却把结论表述为产品属性，正是本门禁要防的过度声称。静态测试枚举二进制的子命令并与门禁比对，因此新增子命令而不纳入观测会失败，不会静默扩张声明。

| 通道 | 静态 | 符号锁 | 运行时 | registry | 专用证据 / 剩余空格 |
|---|---|---|---|---|---|
| safe `std` 文件写与元数据写 | 是 | — | 是 | — | Clippy 负向变异 + read-only harness + sandbox token |
| dyld USDT/DOF 注册 `/dev/dtracehelper` | — | — | 精确允许 `file-write-data` | — | 平台 loader 行为；literal device + exact operation 是唯一 Seatbelt 例外，其他 `/dev` 写不允许 |
| `getattrlist` | unsafe 仅限 `fsx/sys.rs` | 是 | 是 | — | Rust/C object 与 volume 差分、APFS fixtures；返回 mask/volume valid 逐项检查 |
| `statfs` | 同上 | 是 | 是 | — | capacity fixture；x86_64 锁定 SDK 现代 `$INODE64` ABI；仅作显式 basis 与 `SPACEUSED` 失败回退 |
| `fstatfs` | 同上 | 是 | 是 | — | x86_64 锁定 SDK 现代 `$INODE64` ABI；打开 mount root 后回验当前 mount-session `fsid`，拒绝解析期间的 unmount/remount 竞态 |
| `fs_snapshot_list` | 同上 | 是 | 是 | — | 只请求 name + returned mask；首批返回大于 0 即为存在，Data 卷 0 条成功、非 mount-root `EINVAL`；维护者机器的 System 卷为 snapshot-positive 实证，hosted 不声称正例 |
| `setiopolicy_np` | 同上 | 是 | 不适用（刻意修改进程策略） | — | atime/materialize/mount-trigger 三项均 set 后 get 验证；失败阻断 root |
| `getiopolicy_np` | 同上 | 是 | 是 | — | policy round-trip fixture |
| `CFURLCreateFromFileSystemRepresentation` | 同上 | 是 | 是 | — | capacity fixture；仅创建内存 URL |
| `CFURLCopyResourcePropertyForKey` | 同上 | 是 | 是（只覆盖调用进程写） | — | important/opportunistic fixture；未沙箱化 daemon 的状态变化 **未覆盖** |
| `CFNumberGetValue` | 同上 | 是 | 是 | — | capacity fixture、类型/负值检查 |
| `CFRelease` | 同上 | 是 | 是 | — | 所有权分支均显式释放 |
| 两个 CoreFoundation capacity key | 同上 | 是 | 不适用 | — | extern static 同样进入精确集合 |
| File Provider materialization | — | — | 只覆盖调用进程写 | — | `MATERIALIZE_DATALESS_FILES_OFF` set+get；失败不探测，`SF_DATALESS` 标注并跳过；daemon 侧后果 **未被 sandbox 观测** |
| atime / 读诱发 metadata 写 | — | — | 是 | — | `VFS_ATIME_UPDATES_OFF` set+get |
| autofs / mount trigger | — | — | sandbox 不证明 mount 状态 | — | `VFS_TRIGGER_RESOLVE_OFF` set+get；`EDEADLK`/失败令 root unknown |
| nested mount 与 System/Data firmlink | — | — | — | — | 每对象比较真实 `(fsid,fileid)`，真实 fsid 改变即拒绝；synthetic boundary test |
| store 内符号链接与 `Other` 条目 | safe 元数据读取；永不 `children` | `getattrlist` 精确符号集 | TreeSnapshot + 逐子命令 sandbox | — | Q51 Xcode fixture 断言链接只贡献自身 logical/allocated footprint；撤销为 store 级 Err 时同一测试转红。两类均复用 `Root::measure_object` 的 `FSOPT_NOFOLLOW_ANY` 路径，绝不跟随目标 |
| Homebrew prefix 独立 `Root` | safe 路径发现后只走 `Root` API | 与 HOME Root 相同的 FFI 精确集合 | Homebrew TreeSnapshot + scan sandbox | — | Apple Silicon、Intel 与 repository Cellar fallback fixtures；HOME/prefix 分成独立 finding，测试断言不跨 Root 求和；prefix 初始化失败只降级该侧 |
| Homebrew `.git` 版本元数据读取 | 内容打开前经 prefix `Root::measure_object`；无命令 | `getattrlist` 精确符号集 | Homebrew TreeSnapshot + scan sandbox | 0 次调用 | loose ref、detached HEAD、packed refs、出界 `.git` symlink 与缺失 describe-cache fixtures；dataless/出界/未知版本 typed degraded 但不阻断计量 |
| Homebrew keg receipt 解析 | 先经 prefix `Root::measure_object`，再 safe read | `getattrlist` 精确符号集 | Homebrew TreeSnapshot + scan sandbox | — | fixture 只读取 `installed_on_request`；字段缺失保持 unknown，receipt 的名字与 artifact target 均不作为归因证据 |
| Homebrew store 符号链接枚举与 link text | safe `read_link`；不 `stat`/canonicalize target | `getattrlist` 精确符号集 | Homebrew dangling-target fixture + TreeSnapshot + scan sandbox | — | Cellar/Cask cache 链接只计自身；Caskroom staged link 只作 prefix 外 gap 证据；`/Applications` 永不进入 region |
| `brew.env` 改向 `HOMEBREW_CACHE` / `HOMEBREW_LOGS` | — | — | — | — | **未覆盖**：v0.2 不读取或解析 `brew.env`；仅当默认 cache **根**不存在时发一条 `unsupported_path_override`（`declared_scope_boundary`）。子目录缺席是常态，不发 gap |
| 外部命令 / 子进程 | `Command` 仅 policy | — | sandbox 逐子命令覆盖直接进程写尝试 | 是 | registry 精确锁定六条 Xcode/CoreSimulator 与三条 Docker probe；adapter 只能提交 `ProbeId`，不能提交程序、参数或用户输入 |
| Homebrew 外部命令 / 子进程 | `Command` 仅 policy；Homebrew 无 probe id | — | Homebrew read-only harness + scan sandbox | 精确为 0 | 完整 Homebrew inventory 后逐 registry id 断言计数仍为 0；`SIDE_EFFECT_REGISTRY` 精确集合测试锁住未新增条目 |
| Docker CLI context 配置读取 | `Command` 仅 policy；adapter 不直接读 CLI 配置 | — | 产品 sandbox 中尚未接线 Docker | 是，max 1、10s | 固定 Docker.app binary 与 `desktop-linux`；清除 Docker 连接重定向环境并关闭 CLI hooks；只接受当前 HOME 的 per-user Unix socket。CLI 子进程如何读取其内部配置不由 TreeSnapshot 单独证明 |
| Docker `context inspect desktop-linux --format json` | `Command` 仅 policy | — | 产品 sandbox 中尚未接线 Docker | 是，max 1、10s | 长度一 JSON array、context 名、endpoint 与 `SkipTLSVerify=false` 全部验证；TCP、SSH、系统 socket 与其他 HOME 早退，后续两条调用为 0 |
| Docker `--context desktop-linux version --format json` | `Command` 仅 policy | — | 产品 sandbox 中尚未接线 Docker | 是，max 1、15s | 当前只接受 checked-in 的 Desktop/CLI/Engine/API 精确组合；nonzero、未知或畸形输出降级，`system df` 调用为 0 |
| Docker `--context desktop-linux system df --format json` | `Command` 仅 policy | — | 产品 sandbox 中尚未接线 Docker | 是，max 1、120s | NDJSON 必须恰含四类；string count 转换、human-size、重复/缺失/未知类、负数、溢出、非 UTF-8 与额外 stdout fixtures；timeout/nonzero 无重试 |
| Docker CLI 诱发 daemon / Resource Saver 状态变化 | — | — | 不覆盖 daemon | 是（只限制调用次数） | **未覆盖 daemon 写**；version 可能唤醒 Resource Saver，system-df 还会遍历 daemon storage；已登记、无自动重试、可由 `SIZETRAIL_NO_DOCKER_PROBE` 关闭 |
| `/usr/bin/xcode-select -p` | `Command` 仅 policy | — | 是（直接进程） | 是，max 1 | 生产 probe 测试实际执行；只判 selection，标准 CLT → `not_present` |
| `/usr/bin/xcodebuild -version` | `Command` 仅 policy | — | 是（直接进程） | 是，max 1 | 仅 selection 为完整 Xcode 候选后运行；固定 locale/清除重定向环境；未知版本降级 |
| `/usr/bin/xcodebuild -checkFirstLaunchStatus` | `Command` 仅 policy | — | 是（直接进程） | 是，max 1 | 仅已验证版本运行；非零为 `not_ready`，绝不调用写入型 `-runFirstLaunch` / `-license accept` |
| `/usr/libexec/PlistBuddy ... Print :CFBundleVersion .../CoreSimulator.framework/.../Info.plist` | `Command` 仅 policy | — | 产品 sandbox 中关闭 | 是，max 1、10s | 固定只读参数；结果须与 verified Xcode/CoreSimulator 映射精确相等，否则两条 simctl 调用均为 0 |
| `/Library/Developer/PrivateFrameworks/CoreSimulator.framework/Versions/A/Resources/bin/simctl list --json devices` | `Command` 仅 policy | — | 产品 sandbox 中关闭 | 是，max 1、30s | private exact-version 入口；退出 0 + 完整 JSON；UUID/dataPath 后缀验证；完整 Devices exclude 时调用为 0；known effect 仅 CoreSimulator service |
| `/Library/Developer/PrivateFrameworks/CoreSimulator.framework/Versions/A/Resources/bin/simctl list --json runtimes` | `Command` 仅 policy | — | 产品 sandbox 中关闭 | 是，max 1、30s | private exact-version 入口；runtime identity 校验；大小无 vendor 口径时 typed unmeasurable，不 raw delete；known effect 同上 |
| child stdout/stderr 与进程生命周期 | `Command` 仅 policy | — | 完整 scan sandbox 覆盖调用进程 | 是 | 固定输出由 policy 并行排空；registry 硬 timeout 会终止并回收子进程，单测断言 typed `timed_out` 与调用计数 |
| `simctl` 诱发 CoreSimulatorService / simdiskimaged 状态变化 | — | — | 不覆盖 daemon | 是（只限制调用次数） | 已知读副作用；无自动重试、不 pkill daemon、可由 `SIZETRAIL_NO_XCODE_PROBE` 关闭；daemon 自身写 **未覆盖** |
| 内置 TOML 规则解析 | 无外置 rules path；`include_str!` 编入 | — | scan sandbox 覆盖依赖运行路径 | — | schema `deny_unknown_fields`、无 command 字段、每条 rule + fixture 分类断言；toml crate 未执行路径仍不作全局保证 |
| `explain --from file` 显式报告文件读取 | safe read API；无写 API | — | read-only harness；sandbox 逐子命令观测 | — | 仅用户指定报告；先 IOPOL + dataless gate；不重探 finding path、不运行 adapter。stdin 模式无文件读取 |
| `completion` 生成 | safe stdout | — | snapshot harness；sandbox 逐子命令观测 | — | clap builder + `clap_complete` 只写 stdout；集成测试断言 cwd 无新文件 |
| 人类 finding 输出 | safe stdout/stderr | — | sandbox 明确允许输出 fd | — | 文案不稳定；稳定面是 JSON typed signals。当前实现每个 inventory stage 完成后立即输出该 stage 的 findings，不等待全部 adapter 完成 |
| simctl stderr | safe stderr | — | sandbox 明确允许 stderr fd | 是 | 原文只写 stderr；payload 仅 `simctl_stderr_nonempty` 稳定标签，避免主机文本污染 fixture |
| build script | crate root 不覆盖 | 明确断言不存在 | sandbox 构建后不覆盖 | — | 新增前必须重新开门禁 |
| dependency crate 内部写 | 不覆盖依赖源码 | 无直接 libc 只缩小本 crate FFI | 是（仅运行到的路径） | — | 未执行依赖路径与任意 daemon 写 **未覆盖** |
| 继承的可写 fd（stdout/stderr 之外） | — | — | Seatbelt 不追溯 sandbox 前 fd | — | **未覆盖**；当前程序不接收或构造此类 fd |
| IPC 请求未沙箱化 daemon 改状态 | — | extern 锁仅覆盖本 crate 直接入口 | 不覆盖 daemon | — | **未覆盖**；保持 §8.1 的既有证据边界，不声称已解决 |

| 阶段 | 内容 | Definition of Done |
|---|---|---|
| **P0** ✅ | 需求固化 | `decisions.md` 已产出，Q0–Q26 全部消解，frontier 为空 |
| **P1** | truth harness 与计量 schema | repo、CI 全绿、§10.3 五项自定义静态检查就位；空 side-effect registry、运行时计数器及 §10.2 的 1、2 号测试通过；此时无任何 adapter |
| **P1.1** | truth gate hardening | 五项自定义静态检查各有负向测试；零写与命令边界缺口关闭；side-effect registry 成为生产唯一来源；unsafe 唯一豁免边界与强化快照就位；此时仍无任何 adapter 或 FFI 实现 |
| **P1.2** | control-integrity hardening | HOME/TMP/XDG 隔离与越界变异测试；Clippy 禁用集合精确锁定；locked metadata 与生成文档漂移负向测试；macOS 15/26 deny-write sandbox 证明 §8.1 强声明；此时仍不进入 P2 |
| **P1.3** | zero-write channel hardening | lib/bin crate root 锁为 forbid；零写主 lint 纳入豁免边界；真实高价值路径新条目兜底；sandbox 能证明零写尝试为零或重新决策证据边界；`fsx/sys.rs` 返回值与未来 build script 契约写明；此时仍不进入 P2 |
| **P2** | read-only Root/fsx/capacity | §10.2 的 3、4、5 号测试通过（全部 APFS 反例）；plane 1 逐数字口径标注完成；重新推导通道覆盖矩阵并加入本阶段所有 FFI 与读诱发系统代写通道 |
| **P3** | typed adapter contract | 契约 trait 冻结；`not_present` / 未知版本降级路径有测试；adapter 的真实 probe 注册进 P1 已建立的 side-effect registry；重新推导通道覆盖矩阵并加入本阶段所有命令、子进程与 daemon 通道 |
| **P4** | 首个深 adapter + CLI/JSON | Xcode/CoreSimulator（Q29 已将 Homebrew 移出本阶段）；§10.2 全部 13 项通过；§10.4 人工验证完成 → **发布 v0.1 技术预览（schema 明确不稳定）** |
| **P4.1** | Homebrew adapter | 复用 P3 契约，无新增控制面 → **发布 v0.2** |
| **P5** | Docker adapter + 稳定化 | §12.3 顺序 DoD 全部完成；第三个深 adapter；schema 冻结并文档化；完整口径文档；真机验收 → **发布 v1.0** |
| **v1.x** | 第四个 adapter | Go（`GOCACHE`、`GOMODCACHE`）+ 版本门控 |

**工期估算**（30 小时/人周，从零实现）：P1–P4 约 10–15 人周；P5 使总量达 22–28 人周。**该口径已被 P1–P1.3 的实际速度证明不适用于当前工作流（Q29），此处仅保留为范围相对大小的参考，不得作为发布承诺或范围裁剪依据。**

**已永久移出范围：** TUI（约 3–6 人周）、写安全地基（20–27 人周）、adapter 写动作与撤销（6–9 人周）。全部塞入 v1 约 50–70 人周，对单人项目过大。

**为什么先发 v0.1 而不是等 v1.0：** 22–28 人周与 10–15 人周的差别是「今年发得出来」与「可能永远发不出来」，而**单人项目的支配性失效模式是永不发布**。计量契约的早期反馈不需要 Docker。truth harness、schema、Root/fsx 是共享的，故 v0.1 范围是 v1.0 范围的前缀。

**必须叫 v0.x 而非 v1：** 叫 v1 会错误暗示尚未挣得的 schema 稳定性。承认 schema 会变的预览不是虚假声明。

---

## 12.2 P4.1 — Homebrew adapter（Q50–Q52）

本节的详细程度足以直接实施。所有路径与行为均已在维护者机器（Homebrew 6.0.19，`/opt/homebrew`）实测，或读 Homebrew 源码验证；见 `decisions.md` Q50–Q52。

### 12.2.0 前置任务：符号链接不得中断计量（Q51，先于本阶段单独发布）

`measure_store` 目前对 `RootEntryKind::Symlink | Other` 返回 Err，使整个 store unmeasurable。这是**已发布 Xcode adapter 的活缺陷**（`.framework` 内部的 `Versions/Current` 会让任何构建过 framework 的 DerivedData 变成 unmeasurable），也是 Homebrew 的阻断项（本机 `Cellar` 有 581 个符号链接）。

要求：

1. 遍历枚举符号链接与 `Other`，**永不跟随**，各只贡献自身 allocated 字节。
2. fixture 加入符号链接条目，并验证撤销修复后测试转红。
3. 作为独立提交与独立版本发布，不与 Homebrew adapter 捆绑。

### 12.2.1 零外部调用（Q50）

**Homebrew adapter 的外部命令调用次数必须为 0。`SIDE_EFFECT_REGISTRY` 不新增条目，并需测试断言 Homebrew 路径下的调用计数为 0。**

版本只能读取，不能执行：

```text
$HOMEBREW_REPOSITORY/.git/HEAD            → ref 或直接 sha
$HOMEBREW_REPOSITORY/.git/refs/…          → sha
$HOMEBREW_REPOSITORY/.git/packed-refs     → sha（refs 文件不存在时）
$HOMEBREW_REPOSITORY/.git/describe-cache/<sha>  → 版本串，如 6.0.19
```

任一环节缺失、shallow 或无法解析 → `AdapterState::Degraded { reason: UnknownVersion }`。**不得**用 keg receipt 的 `homebrew_version` 冒充当前版本：它记录的是当次安装所用版本，本机实测 receipt 为 `4.6.17-43-ga469d12` 而当前为 `6.0.19`。

与 Xcode 不同，Homebrew **没有 verified-version 白名单**：adapter 不执行它，因此版本不影响调用安全，只作为报告字段。版本未知不阻断计量。

### 12.2.2 prefix 与 root 解析

prefix 与 repository **不是同一目录**：Apple Silicon 上 prefix 与 repository 同为 `/opt/homebrew`；Intel 上 prefix 是 `/usr/local` 而 repository 是 `/usr/local/Homebrew`。不得假设两者相等。

发现顺序（全部只读，不执行 `brew`）：

1. 候选 prefix 依次检查 `bin/brew` 存在且 `Cellar` 或 `Caskroom` 存在：`/opt/homebrew`、`/usr/local`。
2. Cellar 位置：`$PREFIX/Cellar`，若不存在而 `$REPOSITORY/Cellar` 存在则用后者。
3. 均未命中 → `AdapterState::NotPresent`（退出码 0，不是错误）。

`HOMEBREW_PREFIX` 不是用户可设变量（`bin/brew` 由自身路径推导并显式禁止 `brew.env` 覆盖），因此**不读该环境变量**。`HOMEBREW_CACHE` / `HOMEBREW_LOGS` 确实可由 `brew.env` 覆盖，v0.2 **不支持**这些覆盖，并对非默认缓存位置的可能性发 gap 而不是假装默认位置就是全部。

**`--root` 下的 prefix**：候选路径拼接到 root 之下，使 fixture 可提供 `<root>/opt/homebrew`。这与 `~/` 模式在 `--root` 下解析为 root 是同一机制。

prefix 位于 HOME 之外，因此需要**为 prefix 单独 `Root::open`**。若该 Root 打不开（含 prefix 在其他卷上），记 typed gap 并只报告 HOME 侧的 store，不跨 Root 求和。

### 12.2.3 store 与规则表

`src/rules/builtin/homebrew.toml`。HOME 侧用 `~/` 模式走现有 `expand_home_pattern`；prefix 侧用 prefix Root。

| rule id | 路径 | mechanism | recoverability | sensitivity |
|---|---|---|---|---|
| `homebrew.cache_downloads` | `~/Library/Caches/Homebrew/downloads` | `generated` | `redownload_bandwidth` | `low` |
| `homebrew.cache_api` | `~/Library/Caches/Homebrew/api`、`api-source` | `generated` | `redownload_bandwidth` | `low` |
| `homebrew.cache_bootsnap` | `~/Library/Caches/Homebrew/bootsnap` | `generated` | `rebuild_time_cost` | `low` |
| `homebrew.cache_build_tools` | `~/Library/Caches/Homebrew/{cargo_cache,go_cache,go_mod_cache,glide_home,java_cache,npm_cache,pip_cache,gclient_cache}` | `generated` | `rebuild_time_cost` | `low` |
| `homebrew.logs` | `~/Library/Logs/Homebrew` | `user_owned` | `unrecoverable` | `medium` |
| `homebrew.cellar` | `$CELLAR/*/*` | `vendor_managed` | `redownload_bandwidth` | `medium` |
| `homebrew.caskroom` | `$PREFIX/Caskroom/*` | `vendor_managed` | `redownload_bandwidth` | `medium` |
| `homebrew.taps` | `$PREFIX/Library/Taps/*/*` | `generated` | `redownload_bandwidth` | `low` |

`homebrew.logs` 的 `evidence` 必须写明它是**用户状态而非缓存**：失败构建的日志无法重新生成。`~/Library/Logs/Homebrew` 在从未源码构建的机器上不存在（本机即如此），缺失是正常状态而非 gap。

`homebrew.cellar` 是 rack/keg 两层：`Cellar/<formula>/<pkg_version>`。同一 rack 可同时持有多个 keg。它是**已安装软件，不是缓存** —— `evidence` 不得暗示可回收，`advise` 不得给出删除建议。

### 12.2.4 计量规则

1. **符号链接不跟随**（§12.2.0）。`Cask/<token>--<version>` 指向 `../downloads/…`，跟随会把同一批字节数两遍。
2. **hardlink 去重**沿用现有 `FileIdentity` 机制。
3. `estimate_disposition(&extents, false)` —— 与 Xcode 一致，floor 结构性归零（Q40）。
4. **不跨 Root 求和**：HOME 侧与 prefix 侧的 store 各自成 finding，不产生合计字段。
5. keg 身份来自目录名，**不来自 receipt**（receipt 无名字字段）。`installed_on_request` 缺失记 unknown，不记 `false`。

### 12.2.5 归因边界与 typed gap

新增 `InventoryGapReason::CaskArtifactOutsidePrefix`，并同步 `src/scan.rs` 的 `coverage_reason` 与 `gap_reason_id`（两处均为穷举 match）。

| 情形 | gap reason |
|---|---|
| cask 的 artifact 被移到 prefix 之外（Caskroom 只剩符号链接） | `CaskArtifactOutsidePrefix`，coverage status 为 `declared_scope_boundary`（Q54）；**不**使 Homebrew region 变为 `unmeasurable`，**不**产生退出码 3 |
| `.git/describe-cache` 不可读或 shallow | adapter `Degraded { UnknownVersion }` |
| prefix Root 打不开（含跨卷） | `AccessDenied` 或 `TraversalFailed` |
| `HOMEBREW_CACHE` 可能被 `brew.env` 改向 | 仅当 `~/Library/Caches/Homebrew` **根目录**不存在时发一条 `UnsupportedPathOverride`（`declared_scope_boundary`）。单个 cache 子目录缺席不发 gap |

**`/Applications` 不进入 Homebrew region 的计量**（Q52）：首要证据是 Caskroom 内 staged source symlink 的 link text；只做词法归一化与 prefix 边界比较，**不得** `stat`、`canonicalize`、遍历或计量目标。cask receipt 的 `uninstall_artifacts` 仅可作为补充证据，因为普通 `app` receipt 通常没有绝对 target；不得把 receipt 或 symlink 指向的字节求和。

### 12.2.6 advice 契约

`brew cleanup` **会卸载 formula**（`clean!` 内部调用 `autoremove`，除非设 `HOMEBREW_NO_AUTOREMOVE`），不只是删缓存。因此它是 `destructive`，在类型上不得进入 probe runner。

`brew cleanup -n` 的预览**不可靠**，必须明确说明而不是当作 dry-run 推荐：`cleanup_unreferenced_downloads` 与 `cleanup_cache_db` 在 dry-run 下直接 early-return，因此它们将删除的 `downloads/` blob **从不出现在预览里**，量级可达数 GB。

`homebrew.cellar` 与 `homebrew.logs` 只给 `RevealAdvice`，不给任何删除命令。

### 12.2.7 必需测试

1. prefix 发现：Apple Silicon 布局、Intel 布局（prefix ≠ repository）、`$REPOSITORY/Cellar` 回退、两者皆无 → `NotPresent`。
2. 版本读取：`describe-cache` 命中；`HEAD` 为 ref 与为裸 sha 两种；`packed-refs` 回退；缺失 → `UnknownVersion`。
3. **零调用断言**：Homebrew adapter 完整跑一遍后，`InvocationTracker` 计数为 0。
4. 符号链接 fixture：Cellar 内版本化库链接、`Cask/` 指向 `downloads/` 的链接；断言不重复计数且不 unmeasurable。
5. `CaskArtifactOutsidePrefix`：Caskroom 只剩符号链接的 cask 必须产生该 gap；coverage status 为 `declared_scope_boundary`，scan 退出 0（Q54）。
6. `installed_on_request` 缺失 → unknown，不得为 `false`。
7. 每条规则一个 `tests/fixtures/rules/<fixture_id>.json`。
8. 零写 harness：Homebrew fixture 前后快照逐字段相等。
9. `--no-homebrew` → `RegionStatus::ExcludedByUser`，退出码 0。
10. real-environment lane 增加 Homebrew 断言（hosted runner 预装 Homebrew）。

### 12.2.8 Definition of Done

- Q51 的符号链接修复已单独发布。
- 上述十项测试全绿；`SIDE_EFFECT_REGISTRY` 条目数未变，并有测试锁定。
- 通道覆盖矩阵按本阶段后代码重新推导：新增 prefix Root、`.git` 元数据读取、receipt 解析、符号链接枚举各成一行；明确写出「未支持 `brew.env` 缓存改向」这一空格。
- `--no-homebrew` 已接线（当前 flag 已声明但从未被读取）；`explain` 的 `f1:xcode:` 前缀硬编码已泛化；`validate_excludes` 已覆盖 Homebrew root。
- `COMPILED_ADAPTER_IDS` 含 `"homebrew"`，`builtin_rules()` 解析两张表。
- 生成文档重生成；release notes 落 `docs/release-notes/v0.2.0.md` → **发布 v0.2**。

---

## 12.3 P5 — Docker Desktop adapter 与 v1 schema 稳定化（Q55–Q56）

本阶段严格按下列小节推进；前一小节的测试与门禁未绿，不进入下一小节。Docker Desktop
把多种 daemon 对象封装在同一 VM disk image 中，因此“宿主文件计量”与“daemon 分类计量”
是两个不相加的证据面，不能用其中一个替代另一个。

### 12.3.0 v1 schema 前置

1. `Finding.normalized_path` 改为 Q55 的 tagged `subject`；filesystem subject 保持既有 f1
   digest 输入，object-set subject 使用带类型前缀的 canonical key。
2. 内置规则从纯 `paths` 改为同构 tagged subject patterns；静态规则贡献仍只需 TOML +
   fixture。规则 schema 继续 `deny_unknown_fields`，且绝无 command 字段。
3. `Measurement` 增加 typed `quantity`；`MeasurementValue` 增加 Q56 的
   `rounded_bytes`。舍入边界算法必须有 formatter fixture，且任何无法证明格式精度的输入
   直接 unmeasurable，不能给伪区间。
4. `explain --path` 对 object set 显式失败；filesystem finding 的既有 ID fixture 必须不变。
5. 本小节只建立表达能力，不注册 Docker probe；测试先红后绿。

### 12.3.1 probe 闭集与版本门控

生产只允许 Docker.app 自带的绝对 binary。registry 顺序与上限：

| probe id | 固定语义 | max | timeout | 已知副作用 |
|---|---|---:|---:|---|
| `docker.context_inspect` | 读取 `desktop-linux` endpoint JSON | 1 | 10s | 读取 Docker CLI 用户配置 |
| `docker.version` | 固定 context 的 client/server JSON | 1 | 15s | 连接 daemon，可能唤醒 Resource Saver |
| `docker.system_df` | 固定 context 的 summary JSON | 1 | 120s | 可能唤醒 VM；遍历 image/container/volume filesystems |

要求：

1. context endpoint 必须是 `unix://<HOME>/.docker/run/docker.sock`；TCP、SSH、系统 socket、
   其他 HOME 或无法解析一律 `InvalidSelection`，且后两条调用均为 0。
2. 清除全部已知 Docker 连接重定向环境变量；命令、参数、context 名均不得来自用户输入。
3. client version、server version、Docker Desktop platform 与 negotiated API 必须落在 checked-in
   verified set；未知组合 `Degraded { UnknownVersion }`，system-df 调用为 0。
4. `system df` stdout 必须恰好解析成四类 typed row：images、containers、local volumes、
   build cache。缺类、重复类、未知类、负数、溢出、非 UTF-8 或额外 stdout 均 typed gap。
5. disabled、timeout、nonzero 与 malformed output 各有 fixture；无自动重试。

### 12.3.2 宿主 VM disk image

默认位置：`~/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw`。同时支持：

- 当前 `~/Library/Group Containers/group.com.docker/settings-store.json` 的 `DataFolder`；
- Docker Desktop 4.34 及更早 `settings.json` 的 `dataFolder`；
- legacy `Docker.qcow2`。

设置文件先经 HOME `Root::measure_object` 与 dataless gate，再读取内容。自定义 DataFolder
为独立 `Root`；finding 单独报告，绝不与 HOME Root 或 daemon category 求和。raw 与 qcow2
同时存在且无法由已验证版本规则唯一选择时发 typed `ambiguous_disk_image`，不得相加。

disk image finding 只报告：

- apparent/logical limit：`quantity=disk_image_logical_limit`，`basis=logical_size`；
- host allocated footprint：`quantity=host_allocated_footprint`，`basis=allocated_footprint`。

它不产生 disposition interval：删除 VM disk 等于丢失全部 image、container 与 volume，不能
让一个删除上界把这种操作包装成普通 store 清理。daemon unavailable 或 unknown version
不得阻断这两项静态计量。

### 12.3.3 daemon object-set inventory

summary row 映射为四个 `toolchain_object_set` finding：

| object_set_id | rule id | 必需 quantity |
|---|---|---|
| `docker.images` | `docker.images` | object_count、active_object_count、daemon_used、daemon_reclaimable |
| `docker.containers` | `docker.containers` | 同上 |
| `docker.volumes` | `docker.volumes` | 同上 |
| `docker.build_cache` | `docker.build_cache` | 同上 |

counts 是 vendor 原始整数；size/reclaimable 永远使用 `rounded_bytes` +
`basis=docker_system_df`。四类不得求和成“daemon total”：image shared layers 与不同 storage
driver 口径可能重叠。也不得从 host allocated 中相减得到 VM overhead 或 unknown。

Docker Desktop 在 classic/containerd store 间切换时，daemon 只显示 active store；旧 store
仍可能留在同一 disk image。每份成功报告固定携带
`daemon_inventory_excludes_inactive_store` declared-scope gap。单次 scan 也不提供 actual host
free delta；该数字只有用户执行厂商命令后比较两份独立报告才存在，口径文档必须明说。

### 12.3.4 rules 与 advice

内置规则：`docker.virtual_disk`、`docker.images`、`docker.containers`、`docker.volumes`、
`docker.build_cache`。每条均有非空 evidence 与 fixture。

- virtual disk、containers、volumes 为高风险/用户状态，不给出直接文件删除建议；
- images 为 re-downloadable，build cache 为 rebuildable；
- command advice 只使用当前 verified Docker CLI 支持的官方命令，固定
  `--context desktop-linux`，绝不附加 `--force` / `--yes` / shell pipeline；
- 若展示 `docker system prune --volumes`，必须明确它会删除 stopped containers、未使用
  network/image/build cache 及未使用 anonymous volumes，且不是推荐的一键下一步；
- `Docker.raw` / `Docker.qcow2` 永不进入 RevealAdvice 的“可删除”语义。

### 12.3.5 CLI、coverage 与测试

1. `scan` / `doctor` 接线 `--no-docker`；excluded 为退出 0。
2. live `explain` 只重探 Docker owner；`--from` 对 object set 零 probe；`--path` 明确失败。
3. exact `--exclude` 覆盖默认与自定义 disk-image Root，并在任何 probe/stat 前生效；object set
   不接受路径 exclude，使用 `--no-docker`。
4. TreeSnapshot、deny-write sandbox 与 registry count 覆盖三个 Docker probe；sandbox 关闭
   Docker probe 但仍断言静态 disk image measurement 确实执行。
5. fixture 覆盖默认/custom/legacy/ambiguous disk image、context mismatch、unknown version、
   disabled、timeout、malformed/partial/duplicate system-df、四类 row、舍入边界与 advice。
6. 维护者真实 Docker Desktop 机器执行 `doctor`、text scan、JSON scan、live explain，并记录
   Docker Desktop/CLI/Engine/API 精确版本与 context endpoint。hosted runner 无 Docker Desktop
   时不得把 `not_present` 冒充真机正例。
7. Docker fixture benchmark 记录墙钟时间；数字只发布为 runner+fixture 原始值，不推广。

### 12.3.6 v1 freeze 与 Definition of Done

- `COMPILED_ADAPTER_IDS` 为恰好 `homebrew`、`xcode`、`docker`；规则与 fixtures 全绿。
- registry 精确新增三条 Docker probe，并证明所有早退路径的调用上限。
- §12.1 通道覆盖矩阵重新推导，至少新增三条 CLI、context 配置读取、settings 读取、外置
  DataFolder Root、VM disk metadata、daemon/Resource Saver IPC 与 inactive-store 空格。
- schema version 变为 `1.0.0`；生成并逐字节锁定 JSON Schema、fixture report、coverage/unknown
  baseline 与 measurement-basis 文档。v0.x report 不得被 v1 `--from` 尽力误读。
- README 只转写生成片段；版本号/章节号/日期仅使用 §9.1 窄允许清单。所有公开数字经 fixture
  或 support-matrix 生成。
- required CI、dependency policy、zero-write sandbox 全绿；维护者真机验收记录齐备。
- 执行 `/ponytail-review`、`/ponytail-audit` 与 `/ponytail-debt`，处理阻塞项。
- release notes 落 `docs/release-notes/v1.0.0.md`，使用 `--notes-file` 发布 v1.0。

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
| TCC 或其他策略导致扫描不完整 | 归因失真、用户不信任 | §3.1 逐 target typed capability + 原始 errno + 退出码 3 + `doctor` 指引；**绝不静默报 0，也不把 `EPERM` 唯一归因为 FDA** |
| SizeTrail 近似标风险 | 品牌投入后被迫改名 | Q41 已完成 preview 前人工近似筛查；`FILETRAIL`（US 99272187）保留黄色风险；不声称法律清权，商用扩张前做律师 clearance |
| iCloud materialization | **违反只读契约，产生真实下载与写入** | §3.2 强制 `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` 并验证；失败即记 unknown |
| adapter 包装的第三方 CLI 输出格式变化 | 解析失败或**静默误读** | §5.2 钉住已验证版本范围 + 未知版本显式降级。**这是本架构最大的长期维护风险** |
| macOS 大版本挪动路径 | 规则失效 | §6 规则数据化 + `os` 门控 + 每次大版本后回归 |
| 区间过宽导致结论无用 | 用户看到「0 到 20GB」而无从判断 | §2.4 信号解释宽度；`filesystem_compressed` 明确展示为「private floor 不提供信息」 |
| truth contract 腐烂 | 由弱承诺变为**虚假的强承诺** | §9 必须机械化为 CI，不得是人工清单 |
| 单人项目永不发布 | 项目致命 | §12 先发 v0.1 单 adapter 预览（Q29）；已永久移出 TUI 与写路径 |
| 命名首次接触被误解 | 传播损失 | 已在 Q26 用「冷读可读性」这条筛选终局；`SizeTrail` 含 size 指向领域 |
| 无护城河 | mole 若实现同一契约则差异消失 | 承认它（§1.2）。靠聚焦与执行质量存在，不宣称独占功能 |

### 14.2 与 mole 的关系（许可与伦理）

**mole 是 GPL-3.0，已有 65k star。**

1. **不得复制 mole 的任何代码**（含大段改写），否则 SizeTrail 必须整体转为 GPL-3.0。交互模型与信息架构的**理念**借鉴不构成侵权，但具体代码、文案措辞、配色不得照搬。
2. mole 的 README 要求派生产品换名并注明来源。SizeTrail 已换名；README 应致谢 mole 为灵感来源。

---

## 附录 — 实测基线与 APFS 反例

见 `decisions.md` 附录 A（实测环境基线）与附录 B（必须进 fixture 的 APFS 反例清单）。

`probe_attrs.c` 是 fixture 的常设 C oracle。正式版本**必须**使用 `FSOPT_PACK_INVAL_ATTRS` 并逐项检查 returned mask 与 volume valid mask；同时必须按对象种类使用不同的已验证布局（目录不会因 PACK 而获得 file-attribute group 的占位）。PACK 防止已请求但无效的受支持属性挪动后续字段，**不构成「一个固定结构可读取任意对象」的承诺**。Rust 与 C 对 object/volume 两种布局均做差分验证。
