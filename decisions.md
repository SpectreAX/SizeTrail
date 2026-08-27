# SizeTrail 决策记录（Decision Record）

> 本文起源于 **P0 阶段**，记录 Q0–Q26 及实现期新增决策、被否方案与理由。
>
> **`SPEC.md` 由本文派生。** 两者冲突时，**本文优先** —— SPEC 是本文的规格化表达，不是独立来源。
> 新决策先写入本文并编号，再更新 SPEC。不允许 SPEC 携带本文没有的决策。

| 项 | 值 |
|---|---|
| 产品名 | SizeTrail |
| 命令 | `sizetrail`（唯一二进制，无短别名） |
| 决策轮次 | Q0–Q28，全部已消解 |
| 记录日期 | 2026-08-27 |
| 状态 | frontier 为空，P0 完成，可进入 P1 |

---

## 命名（Q26）

**决策：产品名 `SizeTrail`，命令 `sizetrail`。**

历史候选与淘汰原因：

| 候选 | 淘汰原因 |
|---|---|
| `Decalc` | 语义失真。永久只读后（Q11），「除垢」暗示已放弃的清理动作；且高概率被读成 "decimal calculator"，GitHub 现存同名项目恰为计算器 |
| `DiskLedger` | 商标邻近风险最高 |
| `AllocWhy` | **对目标受众自信地错。** crates.io 搜索 `alloc` 前 12 结果全为内存分配器或内存测量工具（bumpalo 542M、allocator-api2 443M、alloc-stdlib 215M、gpu-allocator 17M、wee_alloc 5.5M 下载），无一涉及磁盘。本产品发布至 crates.io 后将落入该命名空间，开发者冷读判为堆分析工具的概率接近于一 |
| `DiskClue` | `CLUE` 在 USPTO 009/042 类有多项 live 软件标；另有 Byteclue 数字取证品牌 |
| `DiskTale` | 拼写近邻 `DISCTALE` 是活跃 iOS/Rust 产品；"tale" 把审计解释弱化为叙事 |
| `APFSLens` | 过度承诺文件系统层分析，并把 Docker/Homebrew adapter 锁进不准确的品牌 |
| `AllocWhys` / `Allocount` | 分别与 SaaS `AllocWise`、Rust `alloc_count` profiler 邻近冲突 |

**筛选条件的修正（重要方法论记录）：** 前期七条筛选条件全为排除性（无商标、无注册表冲突、无品牌占位、不锁实现、不承诺动作、长度合理、语义不过度承诺），**没有一条检验名字是否传达领域**。纯排除性筛选存在退化最优解 —— 无意义字符串与任何东西都不冲突也不承诺任何东西，故满分通过。`AllocWhy` 接近该退化点：它通过「裸搜不被品牌占据」是因为无人会搜它。

补入的第八条：**开发者冷读该名称能形成大致正确的猜测，或至少不形成自信的错误猜测。** 含糊的名字会被点开查证，错的名字被直接跳过。

`SizeTrail` 选中理由：含 `size` 指向领域；`trail` 取 "audit trail"（证据链）义，正对应 Q13 类型化信号与 Q15 `--explain` 展开；命名空间在 crates / npm / PyPI / GitHub 严格同名均空。

**商标记录口径（不得改写）：** 「截至 2026-08-27 的初步多库筛查（USPTO、TMview）未发现冲突」。**不得声称「全球无商标」。** knockout search 不等于法律清权；WIPO 要求同时检索相关国家/地区数据库及近似标。公开 preview 前需再做一次人工近似标复核。

三个候选各败于上一轮未检验的维度（Decalc 语义、DiskLedger 商标、AllocWhy 可读性），说明筛选条件系试错发现、当前这套大概仍不完整。终止该循环的办法是停止优化而非再生成候选。

---

## Q0 — 项目性质

**决策：要发布的开源产品，不是自用工具，也不是作品集。**

后果：护城河问题是存在性的，必须在 frontier 中处理「诚实定位在推广压力下如何存活」（→ Q5）。若为自用工具，整个产品契约框架都是开销。

Codex 前三轮把此项当作已知条件，但它并非已知。

---

## Q1 — 产品契约

**决策：A —— 审计级、范围明确的 macOS 存储解释器。**

公开每个数字的口径、覆盖范围、重叠与未知；**不声称对账 Apple 的 System Data 总数**。

**竞争论据的事实修正（原论据失实，已更正）：** 核查 mole 源码后确认 —— `System Data` 一词仅出现在代码注释中，无对应功能；无 purgeable 计量、无未归因余量、无口径披露、无 `brctl`；快照处理仅为打印数量 + 提示用户自行运行 `tmutil`（`lib/clean/system.sh:1475`）；所谓 hidden-space insights 实为针对 issue #1253 的单个硬编码特例（`lib/clean/system.sh:709-715`）。

因此缺口**不是**「无人解释」，而是「解释被实现为特例集合而非模型」—— mole 的架构把 System Data 不透明性当 bug report 逐个打补丁。

**差异化的诚实表述：** SizeTrail 的差异是「一等归因模型」（共同的 attribution 实体、口径、覆盖范围、unknown residual），不是「mole 没有任何相关能力」。mole 已有 allocated-size、硬链接去重、purgeable 标量与若干 insight，但分散于 Status / Analyze / Clean 三处。若 mole 将来完整实现同一契约，SizeTrail 没有不可复制的技术护城河，只能靠更聚焦、更可信的执行质量存在。

被否：B（全卷分类器，等于进入通用磁盘分析器领域）、C（带解释的安全清理器，放弃归因主定位）。

---

## Q2 — 首要用户

**决策：C —— 仅开发者，围绕 Xcode、模拟器、容器与构建缓存收窄。**

引擎与安全策略保持通用，**规则集**限定开发者场景。因规则表已数据化，收窄在架构上零成本且可逆。

**成本限定（对原论证的修正）：** 「选 C 零成本」只覆盖**规则集收窄**，不覆盖 per-toolchain 深度。Docker.raw 证明深度需要有类型的 adapter，不是 TOML 路径能表达的（→ Q8）。

被否：A（开发者+管理员+技术型用户，付出模糊定位代价却换不到额外灵活性）、B（所有终端用户，与需要理解 APFS/TCC 的 UX 不匹配）。

### Q1+Q2 合并表述（权威）

> SizeTrail 是面向开发者发布的开源、审计级 macOS 存储解释器。v1 深入 Xcode、模拟器、容器与构建工具；通用的是归因引擎和安全策略，规则集限定开发者场景。它不声称复现 Apple 的 System Data 总数。清理是受限附属能力（后经 Q11 降为零），计量契约与口径文档是一等交付物。

---

## Q3 — 威胁模型

**决策：A —— 防规则错误、误操作、符号链接逃逸与正常并发变化；明确不承诺抵抗同 UID 恶意竞态。**

**理由的修正（原理由较弱）：** 不是「B 与 macOS 13+ 和废纸篓优先不兼容」，而是 **B 在防错的东西** —— 同 UID 攻击者对目标路径本就有完整写权限，无需竞态即可直接删除。SizeTrail 不提权、不引入任何它原本没有的能力。故 A 不是妥协，而是唯一正确的威胁模型：**防 bug 与意外，不防对手。**

被否：B（抵抗同 UID 恶意进程）、C（继续把 inode 复核称为完整 TOCTOU 防护 —— 这是错误安全声明）。

**注：Q11 后本项整体退出产品**（无 mutation surface 即无此威胁模型）。保留记录，作为未来任何写命令的硬前置。当时确定的写路径要求：`renameatx_np` 隔离区 + write-ahead manifest + 事后 inode 验证 + 完整后代 manifest 复核 + 冲突时保留隔离项绝不覆盖恢复；firmlink 按真实卷身份处理；原生命令走独立 semantic-ID guard 分支；严格独占单实例锁。准确的保证表述是：「隔离保证只有事后身份匹配的 staged object 才能进入不可逆处置；竞态可能暂时隔离错误对象，但不会将其永久删除。」

---

## Q4 — 废除单轴 T0–T4

**决策：A —— 改为正交字段。**

原 T0–T4 既不互斥也不穷尽（T4 强制走废纸篓故同时是 T2；iCloud 项同时是原生委派、用户数据邻接和状态依赖）。它混淆了「怎么处理」和「处理有多危险」。

字段：`action/mechanism`、`recoverability`、`sensitivity`。风险徽章由这些派生。

两处收紧：

1. **`selection policy` 必须派生而非存储。** 存成字段则规则作者可在高 sensitivity 项上写 `default_selected = true`，schema 拦不住 —— 那正是 T0–T4 想防的错误换地方复发。默认选择应是 `(recoverability, sensitivity, precondition)` 的纯函数，例外需填 `override_reason`。**存事实，派生策略。**
2. **`recoverability` 拆为可枚举值**：Trash 恢复（零成本）/ 重建（成本是时间）/ 重新下载（成本是带宽）/ 依赖外部设备存在 / 不可恢复。使恢复代价机器可读，`evidence` 保留为人类文本但不再承担该语义。

被否：B（一项多 tier，把冲突转移到规则求值）、C（继续单 tier 并补更多层级）。

---

## Q5 — 诚实定位如何抵抗推广压力

**决策：A —— 建立机械化的 truth contract。**

三处收紧：

1. **必须机械化为 CI 门禁**，不得是人工清单。单人开源项目上「不可绕过的治理」的真实失效模式不是不诚实，而是**流程被悄悄放弃却继续声称拥有它** —— 那比「只在贡献指南里建议」更差，因为它把已知的弱承诺换成虚假的强承诺。门禁 = 脚本 grep 禁用声明模式 + 断言文档中每个公开数字均来自 fixture 生成文件。
2. **decision record 仅对量化声明要求**，定性文案豁免。把门禁压在真正会骗人的东西（数字）上，同时大幅降低仪式感。
3. **「减少 unknown」需发布基线值**，否则它与「释放 X GB」同为不可证伪声明。

禁止的声明模式（示例，非穷尽）：「释放 X GB」、「解释全部 System Data」、把「已测量 footprint」写成「可释放空间」、把「已知开发者存储」写成「解释了 System Data」、为截图隐藏 unknown。

核心演进指标：**新增多少可复现的工具链模型、减少多少 unknown**（相对已发布基线），不是「累计可删多少 GB」。

被否：B（仅建议不门禁）、C（技术输出严谨但推广允许宽松概括）。

---

## Q6 — 放弃 APFS 守恒归因等式

**决策：A —— 三平面模型。不跨 measurement basis 求和。**

| Plane | 内容 |
|---|---|
| 1 容量事实 | container allocated、各 volume used、普通/重要/机会性用途可用容量 |
| 2 工具链归因 | 每个 store 输出自己的度量向量、来源、范围、重叠与 unknown |
| 3 处置结果 | 操作 footprint、预计边际回收（区间，见 Q10）、实际 free-space delta |

**删除全局 `unattributed: u64`，改为结构化 `coverage_gaps`。** 只有同口径且已证互斥时才允许算 remainder。

### 已独立核实的三个前提

1. **`st_blocks` 双计 clone。** 实测：20MB 文件 `cp -c` 克隆后两文件各报 `blocks=40960`，`du -sk` 对约 20MB 物理占用的目录报 40MB。故 `st_blocks` 解决稀疏文件但**不解决 clone**，它是 allocated footprint 而非「物理占用」。
2. **`ATTR_CMNEXT_PRIVATESIZE` 存在**（`sys/attr.h:560`）；`ATTR_VOL_SPACEUSED` 在 508，`VOL_CAP_FMT_SHARED_SPACE` 在 291。
3. **Apple `df` 的口径比原描述更极端。** `df.c` 的 `usedblks()` 以 `getattrlist(ATTR_VOL_SPACEUSED)` 为主，`f_blocks - f_bfree` **仅为失败回退**（第 534–567 行）；且第 641 行 `availblks = f_bavail + used`，连容量百分比的分母都不是容器总量。「同一个数字有多个口径」在 Apple 自己的工具里就已成立，故 plane 1 逐个数字声明口径是硬要求。

被否：B（保留带 `accounting_gap` 的容器等式 —— 仍会诱使 UI 与推广把算术差误读为「未归因空间」）、C（用更多 APFS 私有指标补齐单一等式 —— 在共享块与私有 Apple 分类下无实现基础）。

---

## Q7 — 可处置范围边界

**决策：A —— 全部扫描并解释；`user_adjacent` / `user_owned` 一律 observe-only。**

两处修正：

1. **必须明确记录 A 的后果：** v1 可处置范围缩至 go-build / node-gyp 量级，而 Android SDK 6.6G、Docker 镜像等大头全部 observe-only。这与 Q1-A 一致，但必须是**刻意选择的后果**，否则第一波反馈必然是「它找到 60GB，然后什么都做不了」。
   对应产品要求：observe-only 项必须输出该工具链**官方的确切命令**与解释，SizeTrail 不执行但精确告知用户该做什么 —— 把「碰不了」转为「教你怎么办」。
2. **不接受 `re-downloadable` 一律 observe-only：** 具备官方清理命令且路径已验证的 re-downloadable（如 Homebrew 缓存 444M）安全性不低于 `rebuildable`，否则 A 显得教条而非风控。

被否：B（允许用户显式选择 user-adjacent 项）、C（完全不扫描 user-adjacent）。

**注：Q11 后「可处置范围」归零**，本项的存活部分是「全部扫描并解释」+「输出官方命令」（→ Q11 的 `CommandAdvice`）。

---

## Q8 — typed toolchain adapter 为一等架构概念

**决策：A —— TOML 负责静态路径发现与证据；闭集 typed adapter 负责动态工具链。**

Docker.raw 同时封装镜像、容器、volume、BuildKit cache 等不同风险对象，官方口径至少包含 raw logical/max、host allocated、daemon used、daemon reclaimable 与实际 host delta 五个不同数字。直接删除 raw 接近重置整个 Docker 环境。

四处补充：

1. **「规则只能引用已编译 adapter id、不得提供任意命令」写成显式安全属性**，不是架构品味 —— 允许 TOML 携带任意命令等于开命令注入面，会一次性作废全部 guard 工作。
2. **契约实为 `probe → inventory → classify → advise`**（`execute` 按 Q7-A 已缺席，Q11 后永久缺席），按此命名以避免过早构建执行路径。
3. **adapter 数量纪律：** v1 只上 3 个全深度 adapter，不要 10 个浅的。**浅 adapter 比没有更差** —— 它把 mole 的 feature-local 问题搬进自己的架构。
4. **每个 adapter 必须钉住已验证的第三方 CLI 版本范围并在未知版本显式降级。** 这是 `os` 门控的同构物，也是本架构最大的长期维护风险 —— adapter 包装的是第三方 CLI，输出格式会变。

被否：B（全部能力均为 TOML 路径规则，无法安全解释 opaque store）、C（每个工具链独立实现，不共享 contract）。

---

## Q9 — v1 最小可发布范围

**决策：分两步 —— 双 adapter 的 v0.1 技术预览（明确不稳定），随后三 adapter 的 v1.0（稳定 schema）。**

这不是原选项 A 或 B，而是两者的重构。

工期估算口径：维护者熟悉 Rust/macOS，每人周约 30 小时有效工程时间；仓库仅有规格文件，按从零实现估算。

| 发布范围 | 最可能工期 |
|---|---:|
| 双 adapter 只读技术预览（Xcode/CoreSimulator、Homebrew；CLI/JSON；三平面计量；truth CI） | 10–15 人周 |
| 三 adapter 可信只读 v1（+ Docker Desktop；稳定 schema；完整文档与真机验收） | 22–28 人周 |
| 第四个 Go adapter（`GOCACHE`、`GOMODCACHE`） | +2–4 人周 |
| 完整 TUI | +3–6 人周 |
| 完整写安全地基（quarantine、crash recovery、mount/firmlink、锁、两类 guard） | +20–27 人周 |
| adapter 写动作与撤销硬化 | +6–9 人周 |

全部塞进 v1 约 **50–70 人周**（全职 12–18 个月，半职很可能超过两年）。最大变量不是写代码，而是 macOS/Xcode/Docker 版本矩阵、TCC 真机测试与 crash recovery。

**先发 v0.1 预览的理由：** 22–28 人周与 10–15 人周的差别是「今年发得出来」与「可能永远发不出来」，而**单人项目的支配性失效模式是永不发布**。计量契约本身的早期反馈不需要 Docker。truth harness、schema、Root/fsx 是共享的，故双 adapter 范围是三 adapter 范围的前缀，先发损失很小。

**必须叫 v0.x 而不是 v1：** 叫 v1 会错误暗示尚未挣得的 schema 稳定性。承认 schema 会变的预览不是虚假声明。

**注：写安全地基那 20–27 人周（600–810 小时）大于其余全部之和，这直接导出了 Q11。**

阶段重排：(1) truth harness 与计量 schema；(2) read-only Root/fsx/capacity；(3) typed adapter contract；(4) 三个深 adapter；(5) CLI/JSON、文档、真机验证与发布。

**v0.1 的 adapter 数量已由 Q29 收窄为一个（Xcode/CoreSimulator）。** 本条的两步结构、v0.x 命名理由与工期口径继续有效；「双 adapter」这一具体切点作废。

被否：C（完整原规格 v1，50–70 人周）。原 P4 TUI 与 P5–P7 mutation 整体移出（Q11 后永久删除）。

---

## Q10 — Plane 3 区间模型

**决策：A —— 受条件约束的 `[private floor, reference-counted ceiling]`，同时输出 coverage、unknown 与适用动作。**

区间思路（下界 Σ privatesize、上界 Σ allocated）成立，**但仅在下列条件全部满足时**：

- APFS 上的非目录 file forks；
- 按真实 `(fsid, fileid)` 去重（**不是** `(dev, ino)` —— APFS firmlink 要求前者）；
- 目标集包含该 inode 的**全部 hardlink**；
- 数值成功返回且扫描后无 link/clone/snapshot 并发变化；
- 动作为永久 unlink，并最终关闭所有 open reference。

边界规则：

- 不满足完整 hardlink 条件的对象**下界为 0**。
- `allocated` 缺失时**上界为 unknown**。
- 目录、xattr 与文件系统 metadata **不进入区间**。
- 区间宽度**不能称「共享字节」**（多个 clone 会让上界重复膨胀），只能叫 `allocation uncertainty width`。
- Trash 动作的立即释放**恒为 0**；区间只表示将来清空 Trash 后的 deferred potential。
- **实际容器 free delta 必须单列**，不能宣称落在区间内 —— 它受 open fd、目录 metadata 与并发写盘影响。

### 已纠正的错误主张

**「存在 snapshot 时把上界压到 0」是错的。** 反例：snapshot 创建后才生成的文件，删除仍可完全释放。`ATTR_CMNEXT_PRIVATESIZE` 已内生排除被 clone/snapshot 困住的 extent，故该门控冗余且过粗 —— 它应是逐对象的，不是全局条件。若担心扫描后新建 snapshot，应令**未来保证的下界失效为 0**，而不是把上界归零。

原顾虑的合法部分保留为诊断而非门控：`floor≈0 且 ceiling 高` 是有信息的组合，但**不能诊断为「主要被快照占住」**（clone sharing 产生相同现象，且公开 API 无 file extent → snapshot 映射）。只能表述为「共享块或快照导致的不确定性」，并可并列显示该卷的快照事实。

被否：B（只显示 private floor 不给上界）、C（保留 snapshot 归零）。

---

## Q11 — 永久只读

**决策：A —— 永久只读。这是产品边界，不是发布切片。**

SizeTrail **永不**删除、移动、驱逐、thin、prune 或以任何形式修改用户/系统数据。adapter 固定为 `probe → inventory → classify → advise`。

**删除：** `clean`、undo/history、Trash、sudo、guard/quarantine、`--yes`、原 P5–P7、Q3 的 mutation 威胁模型及约 26–36 人周实现成本。

理由（非单纯省工）：

1. Q1 的核心价值是审计级解释；执行删除没有增强这个契约。
2. Q7 已确认真正的大项大多只能 observe-only。剩余可安全代执行的 go-build、Homebrew cache 等 convenience 不足以支撑比整个只读 v1 更大的安全子项目（Q9 显示写路径 20–27 人周 > 其余之和）。
3. 对 Docker volume、模拟器数据与本地镜像，SizeTrail 没有比厂商工具更强的对象语义；代执行只会增加一层责任和失败状态。
4. 永久只读形成清晰、可传播、**可机械验证**的安全属性：正常运行不产生文件系统或外部工具写操作。
5. 若未来出现无法由只读 advice 满足的真实需求，应重新证明，而不应让「反正 v2 要写」成为当前过度设计的依据。

### advice contract（只读不代表 advice 可以不负责任）

- 命令必须来自版本门控的 adapter 数据，**绝不拼接用户输入**。
- 明确标记 `inspect` / `reversible` / `destructive`。
- **永不**附加 `--force`、`--yes` 或 shell 管道。
- 没有 dry-run 的命令必须明确写「厂商未提供可靠预览」。
- `docker system prune --volumes` 可精确展示，但必须同时说明它会删除 stopped containers、未使用对象及 anonymous volumes；**不得包装成推荐的一键下一步**。
- SizeTrail 通过再次扫描提供前后差异，但**永不自动运行**建议命令。

**两种完全分离的 advice 类型：** `CommandAdvice` 与 `RevealAdvice`。destructive advice **只能渲染，类型上不能进入 probe runner**。

### 安全工作的转向

从「防误删」变为「保证探测只读」。需防范的读操作副作用：Homebrew 自动更新、iCloud materialization、mount trigger、错误 Docker context、可能写状态的外部命令。规模和责任显著更小但不为零。

被否：B（v1 只读但保留 mutation 路线）、C（承诺 v2 mutation，保留 guard/quarantine 架构）。

---

## Q12 — 最低支持版本

**决策：A —— API 基线与验证矩阵分离。deployment/API target 为 macOS 13。**

- **API 基线：macOS 13。** 13 使用完整、保守的基础计量路径；14.0/14.4 新属性仅作 capability-gated 补充诊断。所有属性仍检查 volume capability、`valid` 位与 `ATTR_CMN_RETURNED_ATTRS`。
- **当前 GA 验证：macOS 15、26**，Apple Silicon 与 Intel。
- **macOS 14 arm64：** 仅在 deprecated runner 存续期间验证（`macos-14` 将于 2026-11-02 下线），之后自动移出矩阵。Intel 需 larger runner（`macos-14-large`）。
- **Xcode 27 lane：** 只证明 macOS 26 上的工具链/SDK 兼容性。`xcode-27` 运行在 macOS 26 arm64 上，是 SDK preview，**不是 macOS 27 runtime**；GitHub 当前没有 `macos-27` runner。
- **macOS 13：** best-effort，未经 hosted runtime CI 验证。
- CI 同时检查两种架构产物的 **Mach-O minimum OS version**。
- **支持表由实际 CI matrix 生成**，不静态声称永久版本列表。
- 结构性约束：矩阵最多两个 GA + 一个 beta，自动轮换。

被否：B（提高到 14.4）、C（提高到 15+）。提高基线最多节省 1–2 人日，却排除仍常见的开发者机器；非 APFS、外置卷和不返回属性的对象仍要求 fallback，故不会消灭双路径。

### 属性版本矩阵（已核实）

| 能力 | 最低版本 |
|---|---|
| `PRIVATESIZE`、`CLONEID`、`SEALED`、`EF_MAY_SHARE_BLOCKS`、`EF_IS_PURGEABLE`、`SF_DATALESS` | macOS 13 已有 |
| `CLONE_REFCNT`、`ATTRIBUTION_TAG`、`EF_SHARES_ALL_BLOCKS` | macOS 14.0 |
| `CLONE_MAPPING`、卷级 attribution capability | macOS 14.4 |

### 被反例否决的收敛规则（不得重新引入）

```text
EF_MAY_SHARE_BLOCKS == 0 && volume_snapshots == 0   ⟹   allocated == private   ❌ 错误
```

在无 Data 卷快照的 macOS 27 APFS 上实测反例：

- 仅有 2 MiB resource fork 的文件：`allocated = 2 MiB`，`private = 0`，extended flags 为 0。
- HFS 压缩文件：`allocated = 12,288`，`private = 0`，`resource fork = 0`，`extflags = 0`（卷 `disk3s1`，snapshots 0）。

根因：`ATTR_FILE_ALLOCSIZE` 覆盖所有 forks，而 `PRIVATESIZE` 并非它在所有存储形态下的完整独占对应量。普通唯一文件常见相等，但**不能升级为 truth-contract 恒等式**。

压缩反例的可靠构造方式：先制作 CPIO，再用 `ditto -x --hfsCompression` 解包（直接 `cp` 会解压）。此构造过程必须固定在 fixture 中。

其他已收窄的属性用法：

- `VOL_CAP_FMT_CLONE_MAPPING`：可作能力门控，但须同时检查 capability 的 `valid` 位、实际置位状态与 returned mask。Ventura 头文件中不存在，故仅为可选增强。
- `VOL_CAP_FMT_SEALED`：可作强拒绝信号，但永久只读后写 guard 已退出架构。
- 完整 pure-clone family **不能无条件**令区间收敛到 `allocated`：同卷快照仍可能持有共享 extent；且 clone ID 描述数据 stream 而 `ATTR_FILE_ALLOCSIZE` 涵盖所有 forks，未经 fixture 证明不得视为同一集合。仅列为后续可验证的 tightening optimization，**不进入首版正确性契约**。
- `EF_MAY_SHARE_BLOCKS` 清零只排除「可能与另一文件共享块」，不排除 snapshot trapping。`EF_SHARES_ALL_BLOCKS` 只是 full-clone 信号。
- `EF_IS_PURGEABLE` 是逐对象删除策略标志，可形成「文件系统标记为 purgeable 的对象」视图，但**不提供该对象对系统 purgeable capacity 的独占字节**，不能与 plane 1 容量标量配平。
- `ATTR_CMNEXT_ATTRIBUTION_TAG` 是 `u64`，内核结构明确称其为 bundle name 的 **64 位 hash**，不是可展示的 bundle-id 字符串，无公开反向映射契约。可用于分组或辅助验证，**不能替代路径/adapter 归因**。

### iCloud 约束

读取 `SF_DATALESS` 前的元数据调用本身就可能 materialize。扫描必须先设置并验证 `IOPOL_MATERIALIZE_DATALESS_FILES_OFF`；失败则整个相关 root 记为 unknown，`EDEADLK` 同样记为 unknown。**Cloud / File Provider roots 永久排除**，不进入外部工具或 Finder reveal。

---

## Q13 — 不确定性是「信号」而非「成因」

**决策：A —— 输出类型化的观测信号。**

```text
observation = direct | derived
relation    = possible_width_explanation | tested_width_correlate
            | logical_allocation_gap | reclaim_policy | deletion_scope
scope       = object | inode | volume
```

**永久保留 `unexplained_private_gap`；禁止把标签字节相加或声称它们分解了区间。任何负信号组合都不能令区间收敛。**

| 观测 | 能解释什么 | relation |
|---|---|---|
| `EF_MAY_SHARE_BLOCKS` | clone sharing 的可能性 | possible_width_explanation |
| 卷存在快照 | snapshot retention 的可能性，**不能定位到对象** | possible_width_explanation |
| `rsrcallocsize > 0` | 当前 APFS fixture 中与 private gap 相关的计量域差异 | tested_width_correlate |
| `UF_COMPRESSED` | 当前 APFS fixture 中与 private gap 相关的计量盲点 | tested_width_correlate |
| `EF_IS_SPARSE` | `logical − allocated`，**不解释** `allocated − private` | logical_allocation_gap |
| `linkcount > 1` | 删除范围与 inode 去重，**不解释**对象的 private gap | deletion_scope |
| `EF_IS_PURGEABLE` | 文件系统回收策略，**不解释** private gap | reclaim_policy |

resource fork 与压缩行为是**受支持系统上的 fixture 事实，不是公开 APFS 恒等式**。所有标签可能并发；即使全部未命中，仍可能存在无法解释的 private gap。

补充约束：

- **信号模型与有损呈现分层**；`--explain <finding-id>` 必须**无损**展开。
- `unmeasurable` 与 `unexplained_private_gap` 是**不同类型**。
- **hardlink 反例进入 fixture**，证明未完整覆盖 link set 时 floor 必须归零。
- `filesystem_compressed` 高优先级展示为「private floor 不提供信息」，同时保留完整信号。

被否：B（称为机器判定的 `cause`）、C（只保留区间不输出信号）。

---

## Q14 — 不启动 Finder

**决策：B —— SizeTrail 永不启动 Finder。**

路径是一等机器输出；只打印路径，用户自行组合 `open -R`。

理由：Finder reveal 可能启动 File Provider、枚举目录、生成缩略图并写 Finder 自身状态，不能混入零副作用契约。选 B 而非 A（独立 `reveal` 子命令调用 `/usr/bin/open -R`）是因为在永久只读产品中，把路径作为机器输出已足够，引入 UI 委派会为便利性重新打开副作用面。

被否：A（独立 `reveal` 子命令）、C（scan 结果自动联动 Finder）。

---

## Q15 — 报告的主分类

**决策：A —— 一级按开发者心智/adapter，二级 finding 按对象用途。**

一级：Xcode & Simulators、Homebrew、Docker Desktop（+ 未归属桶，见下）。

技术信号通过固定优先级归约成一条摘要（如「clone sharing detected；回收估算不确定」）。归约规则版本化并受 truth contract 管辖，`--explain` 展开完整 observation/relation/scope 集合。

补充：

- **新增「未归属到任何工具链」桶。** 表示「已测量但 ownership 未归属」，**不能**与 `unmeasurable` 或 coverage gap 混同。
- **摘要归约使用确定性总序**：`relation priority → signal ID → scope → finding ID`；fixture 覆盖 tie-break。
- 人类文案**不构成兼容 API**，可自由改写。

被否：B（一级按技术来源：cache/clone/snapshot/resource fork/compression/logs —— 底层 APFS 信号会淹没用户真正的问题）、C（两套并列一级分类 —— 产生重复计量视觉幻觉）。

---

## Q16 — 不支持自定义规则

**决策：A —— v0.1 与 v1.0 仅内置规则和编译内 adapter。**

不支持 `rules.d`。v0.1 schema 明确不稳定，此时开放扩展格式会制造事实上的兼容承诺。用户仍可提交规则贡献；v1 后再根据真实需求决定。

**两条不同的贡献路径：**

1. **静态内置规则仍为 TOML** —— 新增规则只需规则、evidence 和 fixture，**不要求 Rust**。
2. **动态工具链能力才需要 typed adapter 代码。**

内置 TOML 用 `include_str!` 编入二进制，使规则、coverage 与版本绑定；**不读取外置规则**。

被否：B（scan-only 自定义路径规则，标记 untrusted）、C（自定义规则获得同等地位）。

---

## Q17 — 无持久缓存

**决策：A —— v0.1 与 v1.0 均无持久缓存。**

单次进程内共享 adapter 结果；每次命令重新扫描。用真实性能数据决定 v1.x 是否增加显式 opt-in cache。

理由：维持默认 state-free，使 no-write release trace 最简单可靠。目录 mtime 和 TTL 都不能证明深层数据未变化；在没有真实性能数据前，缓存是在用正确性购买未经证明的速度。

**benchmark 口径：** GA runner 记录 fixture benchmark，但只发布「该 runner image + fixture」的原始时间，**不推广成用户机器性能承诺**。runner 轮换后不能把不同硬件结果直接画成趋势。

**side-effect registry：** 记录每个 probe 每次扫描的最大调用次数及关闭开关。

被否：B（v1.0 必须加持久缓存）、C（v0.1 即加 TTL 缓存）。

---

## Q18 — 流式输出与稳定 JSON 共存

**决策：A —— 人类文本模式流式输出，`--json` 扫描完成后一次输出确定性文档。**

- 人类文本模式流式输出 findings；进度写 **stderr**。
- `--json` 在扫描完成后按稳定键排序，一次输出确定性文档。
- **不因非 TTY 自动切 JSON。**
- v0.1 不增加 JSONL。

补充约束：

- **`--json` 只要扫描已初始化，就必须输出合法完整文档。** region/adapter 失败进入文档状态，不得导致 stdout 空缺。**文档状态与退出码正交。**
- JSON 分为两部分：
  - `environment`：时间、主机、HOME、工具版本等**非确定**信息；
  - `payload`：规范化、稳定排序、**逐字节 fixture 比对**。
- fixture 生成时 environment 使用**固定注入值**，不允许事后正则清洗掩盖非确定性。
- v0.1 JSON 明确不稳定，但必须带 **schema version**；v1 后由 semver 管理类型化 JSON。

被否：B（`--json` 改 NDJSON event stream，把到达顺序纳入协议）、C（所有模式都等扫描完成）。

---

## Q19 — 无 TUI

**决策：A —— v0.1 与 v1.0 均不做 TUI。删除 ratatui/crossterm 依赖与原 P4。**

永久只读后原 Selection/Confirm/Result 流程已全部消失；开发者审计场景可由文本与 JSON 完成。省去约 3–6 人周，也避免在 schema 尚未稳定时同时冻结另一套信息架构。不损害 Q1 契约。

**这是显式产品决策：** 原「TUI System Data 清理器」已演化为「开发者、永久只读、纯 CLI、三 adapter 的归因报告器」。维护者知悉并接受，**不得在实现期以遗漏为由重新加入**。

被否：B（v1.0 必须提供只读 TUI）、C（v0.1 即保留 TUI）。

---

## Q20 — 扫描范围排除

**决策：A —— 闭集 adapter 开关 + 可重复的精确 `--exclude <path>`。**

- adapter 开关：`--no-xcode`、`--no-homebrew`、`--no-docker`。
- `--exclude <path>` 可重复，排除规范化子树，**不支持 glob**（glob 容易产生用户以为被排除、实际未匹配的虚假安全感）。
- 报告必须记录 excluded root 与 coverage 变化。
- **不写持久配置。**

补充约束：

- **`--exclude` 在遍历/探测前生效**；被排除子树不得发生 `stat`、`getattrlist`、外部命令调用或 materialization。
- 不存在或未覆盖任何扫描根的 exclude 是 **usage error**，不静默继续。
- `excluded_by_user` 为独立终态，映射退出码 **0**。

被否：B（仅 adapter 开关）、C（持久配置 + glob）。

---

## Q21 — 退出码契约

**决策：A —— 四个固定退出码。**

| 码 | 含义 |
|---|---|
| `0` | 所有已启用且适用的 region 完成。adapter 未安装记 `not_present`，**不算失败**。`excluded_by_user` 亦为 0 |
| `1` | 无法初始化或无法形成合法文档的 fatal error。**窗口仅限初始化前失败**，实践中应罕见 |
| `2` | CLI usage error，由 clap 使用，**不产生 scan 文档** |
| `3` | 产生了完整文档，但至少一个适用 region 因权限、未知版本、解析失败、超时等未测量 |

每个 region 使用 typed status；stderr 仅作人类诊断。

**退出码 3 属信息性**，无 FDA 时为预期状态；就绪性检查 gate 在 `doctor` 而非 `scan` 退出码。

被否：B（只用 0/1，partial 返回 0）、C（任何 unmeasurable 返回 1 —— 会把「机器没装 Docker」误判为错误）。

---

## Q22 — CLI 表面

**决策：A。**

```text
sizetrail                          显示帮助，不自动探测
sizetrail scan [--json]
sizetrail explain <finding-id> [--json|--path] [--from <file|->]
sizetrail doctor [--json]
sizetrail rules [--json]
sizetrail completion <shell>
```

- `--exclude`、`--no-xcode` 等只作用于 `scan` / `doctor`。
- **所有有副作用的探测都要求显式子命令。** 裸命令不应自动启动 CoreSimulatorService、连接 Docker daemon 或运行 Homebrew —— 因为「读」也有副作用。
- `doctor` 有独立价值：报告 TCC、工具版本与 side-effect gate。
- `completion <shell>` **仅打印至 stdout，不写文件。**

被否：B（裸 `sizetrail` 等同 `scan`）、C（仅保留 scan 与 explain）。

---

## Q23 — 无短命令

**决策：A —— 只发布 `sizetrail`。**

删除 `dk` symlink、安装探测与相关文档。审计命令不是每天敲十次的交互工具；规范名更可发现，也减少安装和支持分支。`dk` 另有极高的 shell alias 遮蔽风险（dotfiles 社区中常见的 `docker` / `kubectl` 别名会静默覆盖二进制）。

若真实用户反馈要求短命令，由用户自设 alias。**不保留兼容 alias。**

被否：B（安装时可选 symlink）、C（发布两个二进制名）。

---

## Q24 — `explain` 如何解析 finding ID

**决策：A —— 同时支持 live 重探与快照重放，两者 provenance 必须不同。**

**finding ID 格式：`f1:<adapter_id>:<digest>`**

- `digest` 由版本化算法根据 `adapter_id + rule_id + normalized_path` 派生，其中路径为 **HOME 相对的规范化路径**。
- **绝不使用发现序号。**

两种模式：

1. **`sizetrail explain <id>`** —— 只重探 owning adapter。finding 消失时返回 typed `not_found_after_rescan`，**不回退扫描其他 adapter**。
2. **`sizetrail explain <id> --from <file|->`** —— 纯解析先前报告，**不访问文件系统或外部工具**。结果明确标记 `snapshot_only`、报告时间及「当前路径可能已变化」。校验 schema 版本与 ID 算法版本。`--path` 输出报告捕获时的路径，**不声称当前身份仍匹配**。

**snapshot replay 不得冒充当前验证。**

被否：B（仅 live 重探）、C（仅 `--from`）。

---

## Q25 — 许可

**决策：A —— `MIT OR Apache-2.0` 双许可。**

Rust 生态常见组合；使用者可选简洁 MIT 或带明确专利条款的 Apache-2.0。额外维护成本仅两份许可证文件与贡献声明，不影响运行时复杂度。

### 与 mole 的关系（许可与伦理）

mole 是 **GPL-3.0**，已有 65k star。两点必须遵守：

1. **不得复制 mole 的任何代码**（含大段改写），否则 SizeTrail 必须整体转为 GPL-3.0。交互模型与信息架构的**理念**借鉴不构成侵权，但具体代码、文案措辞、配色不得照搬。
2. mole 的 README 要求派生产品换名并注明来源。SizeTrail 已换名；README 应致谢 mole 为灵感来源。

被否：B（仅 MIT）、C（仅 Apache-2.0）。

---

## Q27 — 零写强声明的系统调用级证据

**决策：保留 §8.1 的强声明，并在 macOS 15/26 CI 中用 `sandbox-exec` 的 `(deny file-write*)` profile 执行完整 scan。**

证据：2026-08-27 在 GitHub hosted `macos-15`（15.7.7）与 `macos-26`（26.5.2）标准 arm64 runner 实测，`/usr/bin/sandbox-exec` 均存在，`(version 1)(allow default)(deny file-write*)` 可解析；对重定向 HOME 与 TMPDIR 的写尝试均返回 `Operation not permitted` 且未创建文件；当前 `sizetrail scan --json` 在同一 profile 下成功。该 CI 必须 fail closed：命令缺失、profile 失效、写入尝试或 scan 无法在零写 sandbox 下完成，均令门禁失败。

`sandbox-exec` 与 profile 语言已 deprecated，不视为永久稳定 API。其可用性由每次目标 runner CI 重新证明；若任一已验证 runner 不再支持，必须重新进入决策，不能静默删除门禁而保留强声明。

被否方案：

- **仅靠 tempdir 树快照而保留原声明。** 变异测试已证明真实 HOME 可落在快照根之外，证据不足。
- **改弱为“受控 fixture 内未观测到写”。** 当前目标 runner 已能提供覆盖目标进程及其子进程任意路径写尝试的系统调用级强制，现阶段无需降级声明。
- **其他无特权观察器。** EndpointSecurity 需要受限 entitlement、FDA 与特权客户端；FSEvents/kqueue 只能观察部分成功变化；OpenBSM/ktrace 需要特权；App Sandbox 无法兼容任意开发者路径扫描。它们均不能替代本门禁。

影响：`SPEC.md` §8.1、§10.2、§10.3 与 §12；P1.2 同时要求 HOME/TMP/XDG 写入位置重定向和快照 harness，作为 sandbox 之外的可诊断第二证据层。

---

## Q28 — 零写沙箱必须证明“未尝试”，不只证明“未成功”

**决策：macOS 15/26 CI 必须观测并拒绝 scan 期间的任意 `file-write*` Sandbox violation；scan 退出 0 与输出合法不能替代“违规记录为零”。**

P1.2 的 deny-write profile 只阻止写成功。若产品吞掉 `EPERM`，scan 仍可退出 0 并输出合法 JSON，门禁会在用户机器上实际写入的代码存在时误报成功。P1.3 改用带唯一 `(with message "<token>")` 的 deny 规则与无 root 的 unified log 实时观察器：START/END 两个必拒写哨兵分别证明观察器已经接通、scan 事件已经排空；独立 SCAN token 的违规数必须为零。观察器退出、哨兵超时、scan 失败、输出非法或任一 SCAN violation 均 fail closed。

2026-08-27 在 GitHub hosted macOS 15.7.7 与 26.5.2 标准 arm64 runner 实测：吞掉写错误并退出 0 的进程仍产生带 token 的 `deny(1) file-write-create` 事件，真实 SizeTrail scan 为零事件，无需 root。`(with report)` **不能**附于 deny action，两版本均以 `report modifier does not apply to deny action` 拒绝 profile，故不采用。

证据边界：Seatbelt 不追溯限制 sandbox 应用前已经打开的可写 fd；stdout/stderr 是产品明确允许的输出通道。`file-write*` 也不覆盖 IPC 请求未沙箱化 daemon 改状态。两者必须在通道覆盖矩阵中保持未覆盖，不得由本决策扩张声称。

影响：`SPEC.md` §8.1、§10.2、§10.3 与 §12。

---

## Q29 — v0.1 收窄为单 adapter

**决策：v0.1 技术预览只含 Xcode/CoreSimulator 一个深 adapter；Homebrew 移至 v0.2，Docker 仍在 v1.0。**

Q9 选择双 adapter 时，唯一的比较依据是人周估算。P1 连同 P1.1–P1.3 三轮加固的实际落地速度证明该口径（30 小时/人周、从零实现）无法描述当前工作流，因此「两个 adapter 才够一次发布」这个前提不再成立 —— 决定发布时机的是 P2 的 FFI 正确性与 P4 的第三方 CLI 版本门控，不是 adapter 计数。

选 Xcode/CoreSimulator 而非 Homebrew 作为唯一 adapter：它是目标受众最大的存储去处（DerivedData、iOS DeviceSupport、模拟器 runtime 与 device data），且同时覆盖 Q7 的 observe-only 边界（device data 是 user_adjacent）与 Q8 的 typed adapter 必要性（CoreSimulator 需要动态枚举）。Homebrew 主要是路径型缓存，对计量契约的压力测试价值低于前者。

单 adapter 不放松任何既有契约：§10.2 的 13 项测试、三平面计量、coverage_gaps 与 truth CI 全部照旧。未实现的 Homebrew/Docker 归属继续落在「未归属到任何工具链」桶与 `coverage_gaps`，**不得**因 adapter 变少而弱化覆盖率表述或暗示扫描已完整。

被否：维持双 adapter（把已知失准的工期估算当作范围依据）、P2 后仅发 plane 1 容量事实（`df` 与 `diskutil` 已覆盖，单独发布无用户价值）。

影响：`SPEC.md` §0 摘要表、§12 阶段表与工期段。Q9 的两步结构与 v0.x 命名理由不变。

---

## Q30 — P1–P3 CI 运行时矩阵收窄

**决策：P1–P3 的 hosted runtime CI 只运行 macOS 15 与 26 的标准 arm64 lane；Intel runtime 与 `xcode-27` lane 推迟到 P4。** 两种架构的 release 产物仍在每次 CI 中构建，并继续检查 Mach-O deployment target 为 macOS 13；这只证明 x86_64 可链接与 API baseline，不得写成 Intel runtime 已验证。

理由：私有仓库 larger runner 按高倍率计费，而 P1–P3 尚无 adapter 版本兼容性需要 Intel 或 Xcode preview 真机证明。Q12 的「支持表由实际 CI matrix 生成」优先于静态愿望清单；未运行的 lane 不能进入已验证支持声明。

被否：在 P1–P3 为 Intel larger runner 持续付费；把 x86_64 交叉构建误写为 runtime 验证；删除 x86_64 构建与 minimum-OS 检查。

影响：Q12 中「当前 GA 验证 Apple Silicon 与 Intel」在 P4 前作废；`SPEC.md` §5.3 与 `.github/workflows/ci.yml`。

---

## Q31 — 门禁必须断言能力已执行，而非仅断言未失败

**决策：任何用于证明某能力的门禁，必须同时断言该能力确实执行了。只断言「没有失败」的门禁视为无效门禁。反例构造失败一律 fail closed；例外只能进入代码内的窄允许清单，并由锁定测试保证清单变更可见。**

P2 审计发现三处同型缺陷，全部是「门禁保持绿色，而它本该检验的能力静默没有执行」：

1. 零写沙箱门禁的 probe root 取自 `mktemp -d /tmp/...`，而 `/tmp` 是指向 `/private/tmp` 的 symlink，`FSOPT_NOFOLLOW_ANY` 因此 `ELOOP`。scan 退出 3，脚本在 `set -e` 下被这个退出码杀掉且无任何诊断输出。
2. 即便 probe root 可用，该门禁也只断言退出码为 0 与 `schema_version` 存在，**从不断言 scan 真的测量了任何东西**。任何让 root 初始化普遍失败的回归都会以全绿通过 §8.1 强声明。
3. APFS 反例测试在构造失败时打印 `SIZETRAIL_P2_COVERAGE_GAP` 后 `return`，测试判定为 PASS。这些反例是「区间不得收敛」的全部经验基础。

三者的共同点不是疏忽，而是**门禁的成功条件被写成了「没有观察到失败」**。在只读产品里这尤其危险：产品的正常行为就是「什么都不做」，因此「什么都没发生」与「什么都没执行」在退出码上不可区分。

P1.1–P1.3 修掉的多数缺口也属此类（`expect` 绕过、`deny` 可降级、沙箱只证明未成功而非未尝试）。因此本条上升为常设规格条款，不再逐个打补丁。

`--nocapture` 保留诊断输出**不构成**证据：绿色状态是唯一会被读取的信号，没人审阅通过运行的 stdout。原 `SPEC.md` §10.2.3 一面要求打印标记、一面声称「不能由绿色状态暗示已验证」，这两句自相矛盾，以本条为准。

被否：只修三处具体缺陷而不立通用条款（同型缺陷会在 P3/P4 随新门禁重新出现）；保留 print-and-continue 并依赖人工审阅日志；用环境变量在 CI 上放宽反例要求（放宽会成为默认路径）。

影响：`SPEC.md` §9.1 新增门禁有效性条款、§10.2.3 改为 fail closed、§10.3 沙箱门禁必须断言已测量。

---

## Q32 — root 在 `open` 时规范化一次

**决策：`Root::open` 在验证 I/O policy 之后对 root 规范化一次，同时保留给定路径与物理路径；cloud 排除与 mount/firmlink 边界一律以物理路径与真实 fsid 判定；root 以下的遍历继续使用 `FSOPT_NOFOLLOW_ANY`。**

原实现对 root 自身也施加 `FSOPT_NOFOLLOW_ANY`，导致任何经由 symlink 祖先到达的 root 被拒。实测（macOS 27，本机）：

| root | 结果 |
|---|---|
| `/tmp/...`（`/tmp` → `/private/tmp`） | 拒绝 |
| `$TMPDIR`（`/var/folders/...`，`/var` → `/private/var`） | 拒绝 |
| `/private/tmp/...` | 接受 |
| `$HOME`、`/Users/Shared` | 接受 |

即 macOS 上两个标准临时目录位置全部不可用。测试套件未暴露此问题，因为 `tests/apfs_counterexamples.rs` 在构造 fixture 后先调用了 `fs::canonicalize`；零写沙箱门禁没有这样做，于是长期是红的。

对**永久只读**产品而言，symlink 的风险是遍历期逃逸到 root 之外，而不是 root 自身如何被命名。规范化一次即可把逃逸判定建立在物理身份上，严格性不降反升：`measure_object` 的前缀检查从文本前缀变为物理前缀。这也与既有的「firmlink 按真实卷身份处理」一致。

**顺序是硬约束：** `fs::canonicalize` 是元数据调用，可能触发 materialization，因此必须排在 `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` 设置并验证**之后**。cloud 前缀检查在规范化前按给定路径做一次（纯文本，不触碰文件系统），规范化后按物理路径再做一次，防止 symlink 指入 `CloudStorage`。

被否：保留对 root 的 `NOFOLLOW_ANY`（使 `--root` 在标准临时目录不可用，且只保护了命名方式而非遍历）；在门禁脚本里绕过而不改产品（真实用户的 `--root /tmp/x` 仍会失败）；在规范化后放弃 root 以下的 `NOFOLLOW_ANY`（那才是真正的逃逸面）。

影响：`SPEC.md` §3.2 顺序约束与新增 root 路径策略；`src/fsx/mod.rs`；`scripts/check-zero-write-sandbox.sh`。

---

## Q33 — root 失败原因必须 typed

**决策：`Root::open` 返回 typed 错误，capacity 的 `unmeasurable` 原因随之 typed 并进入 JSON。`IOPOL` 验证失败必须与路径类失败在输出上可区分。**

原实现把所有 root 失败折叠成单一字符串 `"root initialization or read-policy verification failed"`。后果是**无法从 JSON 判断 materialization 闸门是否失败过** —— 而红线 6 要求 IOPOL 失败时整个 root 记 unknown，这条要求若在输出上不可验证，就只是一句无法审计的声明。它同时让用户对 `--root` 路径问题只能得到不可行动的错误。

最小原因集：`read_policy_verification_failed`（红线 6，必须独立）、`root_path_unresolvable`、`root_path_not_encodable`、`cloud_root_excluded`、`root_identity_unavailable`、`symlink_traversal_rejected`、`not_normalized_absolute`。capacity 内部原因（`volume_capacity_query_failed`、`shared_container_capability_unavailable`、`capacity_arithmetic_overflowed`、`core_foundation_capacity_unavailable`）同时 typed，避免 typed 与字符串混用。

被否：保留单一字符串并靠 stderr 区分（stderr 只作人类诊断，不是机器契约）；只在人类文本模式区分。

影响：`SPEC.md` §3.2；`src/fsx/mod.rs`、`src/capacity.rs`、`src/model.rs`。

---

## 附录 A — 实测环境基线

采集于 2026-08-26，作为规则表量级参考与回归基线：

```text
macOS 27.0 (26A5421a) · Apple Silicon · SIP enabled
/dev/disk3s1  228Gi 总量  163Gi 已用  38Gi 可用  (82%)
APFS 容器可用 40.4GB
本地快照：无

~/Library/Android                                  6.6G
~/Library/Application Support                      6.3G
  ├ Steam                                          1.6G
  ├ com.apple.wallpaper                            870M
  ├ QuarkCloudDrive                                840M
  └ Google                                         804M
~/Library/Caches                                   2.3G
  ├ go-build                                       571M
  ├ Homebrew                                       444M
  └ ShipIt (更新残留)                              303M
~/Library/Logs                                      89M
~/Library/Containers                                15M
/private/var/log                                    87M
/Library/Caches                                    4.0K

外部工具：tmutil ✓  brctl ✓  mdutil ✓  diskutil ✓  xcrun ✓  fd ✗
```

**该机器 `~/Library` 合计约 16GB，而卷已用 163GB。** 差额主要是用户文件，不属于开发者工具链归因范围 —— 这正是 Q6 要求 `coverage_gaps` 结构化呈现（而非单一 `unattributed` 数字）的原因。

## 附录 B — 已验证的 APFS 反例清单（必须进 fixture）

| 反例 | 证明什么 |
|---|---|
| 20MB 文件 `cp -c` 克隆 | `st_blocks` 双计 clone；`du` 报 40MB 而物理约 20MB |
| 仅 2 MiB resource fork 的文件 | `allocated = 2 MiB` 而 `private = 0`，extflags 为 0 |
| HFS 压缩文件（`ditto -x --hfsCompression`） | `allocated = 12,288`、`private = 0`、`rsrc = 0`、`extflags = 0`，无快照 |
| hardlink 未完整覆盖 link set | floor 必须归零 |
| 稀疏文件 | 只解释 `logical − allocated`，不解释 `allocated − private` |

`probe_attrs.c` 是这些 fixture 的起点。正式版本**必须**使用 `FSOPT_PACK_INVAL_ATTRS`（或动态解析 buffer）并逐项检查 returned mask —— 固定结构在属性缺失时会字段错位。
