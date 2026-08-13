# Tasks · add-release-version-autobump

## 1. 自增脚本

- [x] 1.1 新增 `scripts/bump-version.mjs`：导出纯函数 `bumpPatch` / `replaceJsonVersion` / `replaceCargoPackageVersion` / `replaceCargoLockVersion` / `prependChangelogEntry`，主流程用入口守卫包住，供测试直接 import 而不触发副作用。
- [x] 1.2 读取四处版本号（`package.json`、`src-tauri/Cargo.toml` 的 `[package]` 段、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.lock` 的 `provider-deck` 条目），不一致即中止并列出各自当前值。
- [x] 1.3 校验版本号为三段纯数字；带预发布标识或构建元数据时中止。
- [x] 1.4 先在内存中算出四份新内容，全部成功后再落盘，避免半改状态。保留 LF 行尾与无 BOM。
- [x] 1.5 `CHANGELOG.md` 顶部插入新版本小节；文件不存在则创建。正文不编造，支持 `--note` 传入。
- [x] 1.6 落盘后回读四处并自检一致；打印「旧版本：x，新版本：y」。
- [x] 1.7 支持 `--dry-run`（只打印不落盘）与 `--out <file>`（把新版本号写入文件供 bat 读取）。

## 2. 单元测试

- [x] 2.1 新增 `scripts/bump-version.test.mjs`：`bumpPatch` 正常进位、`0.1.9 → 0.1.10`、非三段/预发布/构建元数据一律抛错。
- [x] 2.2 四种文件改写函数各自的成功用例，以及"找不到目标字段即抛错"用例。
- [x] 2.3 `replaceCargoLockVersion` 只改 `provider-deck` 条目，不误伤同名 version 键的依赖条目。另加一条：锁文件顶部的 `version = 3`（格式版本，非包版本）必须保持不变。
- [x] 2.4 `prependChangelogEntry`：已有内容时新小节在最前且旧内容逐字保留；文件为空时创建。
- [x] 2.5 用例中不出现任何明文密钥或鉴权字段（含一条正向断言，防止否定断言空转）。

## 3. 打包脚本改造

- [x] 3.1 `build_all.bat` 拆 `:prepare_output` 为 `:prepare_log` 与 `:prepare_release_dir`，保留全部既有守卫（并发日志锁、release 目录清空校验、NSIS 暂存目录清空校验）。`CARGO_HOME` / `CARGO_TARGET_DIR` 一并前移到 `:prepare_log`——`cargo test` 用的是同一个 target 目录。
- [x] 3.2 插入 `:bump_version` 步骤，位置在 `:run_tests` 之后、`:prepare_release_dir` 之前；步骤编号统一改为 12 步。
- [x] 3.3 自增后重跑 `check-version.mjs` 作为独立校验层，并据其输出刷新 `VERSION`。另加「自增后版本号必须与自增前不同」的守卫。
- [x] 3.4 支持 `PROVIDER_DECK_SKIP_BUMP=1` 跳过自增。
- [x] 3.5 把 `CHANGELOG.md` 复制进 release 目录（在 `:write_checksums` 之前，确保被哈希覆盖）。
- [x] 3.6 **超出原清单**：三个临时文件路径拆开（`VERSION_PROBE_FILE` / `BUMP_OUT_FILE` / `RECHECK_FILE`）。`DisableDelayedExpansion` 会把 `%RANDOM%` 冻结在启动那一刻，单变量会让三步共用同一文件名；任一步清理失败，下一步就会把上一步的残留当成自己的结果读走。
- [x] 3.7 **超出原清单**：`BUILD FAILED` 区块区分已自增/未自增。原文无条件打印「源文件未改动」，自增之后那句就是假的，会把人引到错误的方向排查。

## 4. 交接文档

- [x] 4.1 `HANDOFF.md` 新增第 11 节版本升级规则：打包自动升 patch；破坏性接口/架构变更手动改 MINOR；重大重构/不兼容对外 API 手动改 MAJOR。
- [x] 4.2 写明四文件对齐范围、自增在闸门之后的理由、`PROVIDER_DECK_SKIP_BUMP` 逃生口、「破坏性」的判定标准（以数据与外部契约为准，不以改动量为准）、git tag 不自动打的理由。

## 5. 验证与发布

- [ ] 5.1 `--dry-run` 验证脚本，确认不落盘。
- [ ] 5.2 五道闸门全绿（cargo / tsc / vitest / eslint / playwright）。
- [ ] 5.3 执行 v0.1.12 完整 Release 打包，核对产物版本号、CHANGELOG 顶部、SHA256SUMS 覆盖范围。
