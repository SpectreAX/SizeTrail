# AGENTS.md — SizeTrail

**动手前先读 `decisions.md`，再读 `SPEC.md`。**

- `decisions.md` 是决策来源（Q0–Q26 全部已消解）。
- `SPEC.md` 是它的规格化表达。
- **两者冲突时 `decisions.md` 优先。**

SizeTrail 是一个**永久只读**的 macOS 存储归因解释器。它**不删除任何东西**。它的价值全部来自计量口径的诚实与可验证 —— 因此说谎（包括无意的过度声称）比崩溃更严重。

## 红线（不可协商）

1. **零写。** 整个 crate 不得出现 `fs::write`、`fs::remove_*`、`fs::rename`、`File::create`、`OpenOptions::write/append/create`。唯一输出是 stdout / stderr。无日志文件、无缓存、无配置、`completion` 也只打印不写盘。
2. **`policy` 层是发起外部命令的唯一出口。** 其他任何层不得直接调用 `std::process::Command`。
3. **不跨 measurement basis 求和。** 禁止全局 `unattributed` 字段，禁止用平面相减得出「未归因空间」。用结构化 `coverage_gaps`。
4. **区间不得收敛。** `EF_MAY_SHARE_BLOCKS == 0 && snapshots == 0` **不能**推出 `allocated == private`（resource fork 与 HFS 压缩是已验证反例）。任何负信号组合都不得令区间收敛。`unexplained_private_gap` 永久保留在输出类型中。
5. **信号不是成因。** 观测信号禁止相加，禁止声称它们分解了区间。不得指出某文件被哪个具体快照保留 —— 公开 API 没有 extent → snapshot 映射。
6. **探测前必须验证 `IOPOL_MATERIALIZE_DATALESS_FILES_OFF`。** 失败则整个相关 root 记 unknown。Cloud / File Provider roots 永久排除。**元数据调用本身就可能 materialize，这会产生真实的下载与写入。**
7. **绝不 `sudo`。** 不装 launchd 守护进程、不装特权 helper。
8. **绝不执行 advice 命令。** `destructive` advice 必须在**类型上**无法进入 probe runner。命令绝不拼接用户输入，绝不附加 `--force` / `--yes` / shell 管道。
9. **绝不启动 Finder。** 路径是一等机器输出，用户自行组合 `open -R`。
10. 无遥测、无联网、无自动更新检查、不读外置规则。

## 豁免于任何形式精简的内容

`SPEC.md` §2（计量口径与区间边界）、§8（只读契约与副作用闸门）、§9（truth contract 的 CI 机械化）、§10.2（13 个测试）、每条规则的 `evidence` 与派生 selection policy。

## Skills 使用

- `/ponytail`（full）：实现阶段常驻。**作用于格式化、CLI 样板、抽象层次、依赖选择，不作用于上述红线与豁免内容。** 冲突时红线优先。
- `/ponytail-review`：每阶段 diff 完成后、合入前执行。
- `/ponytail-audit`：P5 发布前执行一次。
- `/ponytail-debt`：每阶段结束执行，确认 `ponytail:` 标记没被永久搁置。
- `/grill-me`：已随仓库提供于 `.agents/skills/`。**只能由用户手动触发**（`disable-model-invocation: true`），不要等它自动发起。P0 已完成。其方法 skill `grilling` 要求事实由 Agent 自查、只把决策交给用户 —— **事实性问题（macOS 路径、外部工具行为、属性可用性）请派 sub-agent 查证，不要反问维护者。**

## 工作方式

- 按 `SPEC.md` §12 的阶段推进。**每阶段满足 Definition of Done 才进入下一阶段，不跳阶、不并行。**
- `policy`、`fsx`、`model` 三层用 TDD：先写失败测试，再写实现。
- 所有端到端测试走 `--root` 沙箱。**注意：`--root` 现在是测试注入机制，不是删除防护**（已无删除）。
- 每条新规则必须有非空 `evidence` + 对应 fixture 测试。规则是 TOML 数据，不是代码 —— 新增规则**不要求写 Rust**；只有动态工具链能力才需要 typed adapter。
- 规则**只能引用已编译的 adapter id**。让 TOML 携带任意命令是命令注入面，不是灵活性。
- 发现规格有误：**先改 `decisions.md`（若属决策变更）或 `SPEC.md`（若属表达错误）并说明理由，再改代码。** 不允许代码与规格静默分叉。

## 已被显式删除、不得重新加入的东西

以下都是**决策结果**，不是遗漏。实现期不得以「这里似乎缺了点什么」为由补回：

- TUI（Q19）、`ratatui` / `crossterm` 依赖
- `clean` / `undo` / `history` / Trash / guard / quarantine / `--yes`（Q11）
- `dk` 短命令与任何兼容 alias（Q23）
- 持久缓存（Q17）、外置规则 `rules.d`（Q16）、持久配置（Q20）
- 单轴 T0–T4 安全层级（Q4，已替换为正交字段）
- 为将来写功能预留的抽象（Q11-A 明确排除路线保留）

## 禁止

- 非测试代码中出现 `unwrap()`（clippy 强制）
- 叙述性代码注释（注释只写代码无法表达的约束与取舍）
- 无测试的 `policy` / `fsx` / `model` 代码合入
- 引入 async 运行时、ORM、网络库、GUI 框架、TUI 框架
- 复制 mole（GPL-3.0）的代码或文案
- 使用 `SPEC.md` §9.2 列出的任何禁用声明模式（「释放 X GB」、「解释全部 System Data」、把 uncertainty width 称为「共享字节」等）

## 命令

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all
```
