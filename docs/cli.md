# CLI 使用与结果契约

## 命令概览

```sh
# 静态元数据和声明的符号/版本需求，不运行目标
ExeViewer inspect ./app --format json

# 当前机器：按当前文件系统和 LD_LIBRARY_PATH/LD_PRELOAD 推演
ExeViewer diagnose ./app
ExeViewer diagnose ./app --interactive
ExeViewer diagnose ./app --format json > local.json

# B 环境：文件均在 rootfs 内解析，输入产物可以在 rootfs 外
ExeViewer diagnose ./app --rootfs ./target-root --target-path /opt/app/bin/app \
  --cwd /opt/app --library-path /opt/app/lib --kernel-release 5.15.0 \
  --isa-level 2 --format json > target.json

# 固定目标加载器的默认目录；覆盖而不是追加
ExeViewer diagnose ./app --default-path /lib64:/usr/lib64

# 对比同一产物的两份 JSON 诊断快照
ExeViewer compare --env-a local.json --env-b target.json --format json

# 导入在目标环境采集的 glibc 日志，不执行目标
ExeViewer trace --log loader.log --baseline target.json --format json

# 显式执行本机程序，保留其输出、加载器日志和结构化事件
ExeViewer trace --execute ./app --output-dir ./capture-001 --timeout 15 \
  --baseline local.json --format json -- --application-argument

# 原有轻量报告及交互查看器仍可使用
ExeViewer --report ./app
ExeViewer ./app > report.txt
ExeViewer --interactive ./app
```

`diagnose`、`inspect`、`compare` 和 `trace` 默认直接输出文本；`--format json` 提供结构化结果，无需 TTY。旧的 `--report` 是静态概览，不能与子命令混用。`diagnose --interactive` 使用同一诊断报告提供结论、问题、搜索证据和环境四个终端页面；退出码仍反映诊断结论。该选项需要 TTY，且不能与 `--format` 同时指定。

`inspect` 的 JSON 包含动态符号与精确版本需求/定义；文本是面向人的概览，包含版本需求和符号数量。诊断解析从程序头、PT_LOAD/PT_DYNAMIC、动态哈希及重定位获取必要信息，不要求 `.symtab` 或节区表。旧 `--report` 保留更宽泛的 ELF 基本查看能力；新诊断解析限定 ELF64 小端。

## 目标环境

- `--rootfs` 默认 `/`。离线 rootfs 不继承宿主的 LD_LIBRARY_PATH/LD_PRELOAD，不使用宿主库补齐缺失依赖。
- `--target-path` 是产物在目标内的安装路径，用于 `$ORIGIN`。若产物不在 rootfs 下又没有提供安装路径，报告 `INSTALL_PATH_UNKNOWN`。
- `--cwd` 是目标工作目录，处理相对库路径、空搜索项。离线默认 `/`，本机默认当前工作目录。
- `--library-path`、`--preload` 显式覆盖对应环境输入，传空字符串可清除。目标 `/etc/ld.so.preload` 仍单独读取。
- `--default-path` 明确指定目标加载器编译时默认目录。默认值是常见 x86-64 glibc 目录集合，不能代表所有发行版。
- `--machine` 是目标 ELF machine 编号，默认 62（x86-64）。其他架构返回不支持或不完整。
- `--isa-level 1..4` 提供目标 x86-64 ISA 等级；只检查 GNU property 声明的必要等级。未声明不等于没有特殊指令。
- `--kernel-release` 提供目标内核版本；本机默认读取 `/proc/sys/kernel/osrelease`，离线不读宿主内核。只检查 GNU ABI-tag 明确声明的最低 Linux 内核要求，不模拟所有系统调用。

绝对符号链接在目标 rootfs 内重新解释；包含 `..` 的路径在符号链接解析后处理。库搜索支持请求方 RPATH/RUNPATH、祖先 RPATH、显式库路径、当前 glibc cache 格式及默认目录，记录实际候选尝试和重复对象复用。CPU hwcaps、未知动态 token、旧缓存格式等未建模情况会标为 unknown。

静态解析使用文件快照时刻的内容。rootfs 应保持不变；它不是针对并发恶意修改文件系统的安全隔离沙箱。

## JSON 与诊断

所有成功生成的结构化输出包含 `schema_version: 1` 和 `kind`。诊断结果包含：

- `artifact`、`objects[].elf.sha256`：输入和每个已解析对象的标识。
- `environment`：rootfs、工作目录、库路径、预加载、默认目录、目标架构及可选 CPU/内核条件。
- `status`、`scope`、`limitations`：结果和适用范围。
- `objects`：对象及引入链、依赖、版本定义/需求、动态符号。
- `dependencies`：请求方、依赖名、最终路径、是否复用、按顺序记录的候选路径与来源及结果。
- `diagnostics`：规则 ID、严重程度、确定性、对象、引入链、证据及修复方向。

常见规则包括 `INTERPRETER_MISSING`、`LIBRARY_NOT_FOUND`、`VERSION_MISSING`、`SYMBOL_UNRESOLVED`、`ARCH_MISMATCH`、`ISA_REQUIREMENT`、`KERNEL_REQUIREMENT`；`*_UNKNOWN` 和 `UNSUPPORTED_*` 明确表达证据或模型限制。

缺失强动态符号按照启动对象集合诊断；弱符号不作为阻断。目标库完全没有版本定义时，glibc 可能容许无版本提供者，因此此情况单独警告，不误报为必然失败。找不到预加载文件也按 glibc 的忽略行为给出警告。版本检查核对具体名称，例如 `GLIBC_2.34` 是否实际被选中的提供者定义，不按版本字符串大小猜测。延迟绑定、插件宿主、IFUNC、复杂作用域和 ABI 语义需要实测。独立 so 缺少可确定的宿主作用域，返回不完整；若真实部署使用预加载，可以对宿主 exe 指定 `--preload`，不能将预加载等同于任意 dlopen 行为。

`compare` 消费保存的完整诊断报告，不重新读取当前文件系统。要求输入主产物 SHA-256 相同；逐项对比请求方/依赖、搜索记录、命中路径、库内容哈希和诊断。路径别名变化也会显示为差异，用户需结合证据判断影响。

## 退出码与流

| 命令 | 0 | 1 | 2 |
| --- | --- | --- | --- |
| diagnose | 检查范围内未发现阻断 | 已发现阻断且无未完成检查 | 证据不足、模型不支持或输入/解析失败 |
| compare | 无依赖/诊断差异且两边完整 | 存在差异 | 任一报告不完整或输入错误 |
| trace | 已收集到事件且未发现本次执行失败 | 目标非零退出或记录加载错误 | 超时、无事件、输入错误或采集不完整 |
| inspect | 静态读取成功 | — | 输入或解析失败 |

诊断同时存在 error 和 unknown 时，退出码为 2，报告保留所有已证明的阻断。没有阻断不等于所有运行路径都兼容。

文本模式的操作错误写 stderr；JSON 模式的操作错误写 stdout，结构为 `kind: error`、`status: incomplete`、`error`。Clap 参数语法错误沿用 stderr + 退出码 2，不承诺 JSON。旧报告保留原有成功 0、失败非零的契约。管道关闭不会导致 panic；诊断退出码仍反映分析结论。

## 运行采集

只有 `trace --execute` 会运行目标。采集在当前机器和当前环境进行，不进入 rootfs、不模拟目标 CPU，也不是沙箱。程序参数放在 `--` 后。

`--output-dir` 必须是新目录，避免覆盖既有证据。目标 stdin 关闭，stdout/stderr 保存为文件；加载器日志为 `loader.<pid>`。CLI stdout 仅用于报告。默认超时 15 秒，轮询日志大小约 32 MiB 限制；达到限制后停止独立进程组，采集结束也会清理同进程组内后台子进程。自行脱离进程组的程序不属于该清理机制保证范围。

事件保留原始文件、行号、PID 和日志文本，区分搜索、候选、映射、初始化、动态加载、版本检查、绑定和加载错误。多个进程日志按来源分组，不伪造跨进程全局时间顺序。候选尝试不是成功加载证据。

`--baseline` 将已映射/初始化的绝对路径与静态报告对照。执行采集时校验主产物哈希。路径别名或日志缺失也可能导致差异，未观察到不能直接推断没有加载。导入日志不能验证其环境或完整性。

## 测试

```sh
cargo test --offline
```

Linux x86-64 上的端到端用例需要 Python 3、GCC 和 `/lib64/ld-linux-x86-64.so.2`。测试自行编译小型版本化共享库和可执行文件，覆盖搜索作用域、版本/符号错误、路径隔离、无节区表、缓存、预加载、A/B 对比、日志采集和超时，并用真实 glibc loader 校验本机库选择。
