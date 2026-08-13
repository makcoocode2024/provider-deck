## Why

当前版本号是纯手工维护，而 `build_all.bat` 的 `:read_version` 只做交叉核对：三处版本号一致就放行，不一致就中止。这意味着连续两次正式打包如果忘了手改版本号，第二个包会**沿用同一个版本号**——NSIS 安装版覆盖不掉旧版、release 目录被同名覆盖、SHA256SUMS 对不上任何一次发布，而脚本全程 `[OK]`，因为「一致」这个条件确实满足了。

`:read_version` 能挡住"三处不一致"，挡不住"三处一致但都是旧的"。补掉的正是后者。

## What Changes

- **新增** `scripts/bump-version.mjs`：读 `package.json` 的 version 作为唯一事实来源，patch + 1，同步写入全部携带版本号的文件，并把新版本号插入 `CHANGELOG.md` 顶部。
- **新增** `CHANGELOG.md`：仓库根，倒序排列，每次正式打包由脚本插入一节。
- **修改** `build_all.bat`：在测试闸门全绿之后、创建 release 目录之前插入自增步骤；步骤编号 10 → 12。日常开发命令（`npm run dev` / `tauri dev` / 五道闸门）完全不碰版本号。
- **修改** `scripts/write-release-docs.mjs`：无改动（保持不变，仅由 bat 把 CHANGELOG.md 复制进 release 目录）。
- **不变**：`scripts/check-version.mjs` 一行不改。它在自增之后**再跑一次**，作为独立校验层确认四处确实对齐——自增脚本自己的自检不能当作唯一证据。

### 与原始需求的三处偏差（已在实现中处理，需知悉）

1. **需求说「双文件版本永久对齐」，实际是四文件。** 除 `package.json` 与 `src-tauri/Cargo.toml`，还有 `src-tauri/tauri.conf.json`（`check-version.mjs` 已在核对它，只改两处会让 bat 在下一步自己失败）和 `src-tauri/Cargo.lock` 的 `provider-deck` 条目（不改则每次发布后工作树变脏，且将来若给 cargo 加 `--locked` 会直接构建失败）。
2. **需求说「打包时触发自增」，实现放在测试闸门之后。** 若放在打包最前面，任何一次 cargo/tsc/vitest/eslint 失败都会留下一个已自增但没有产物的版本号，版本号被白吃掉且工作树变脏。测试全绿才自增，失败时源文件零改动——与 bat 现有的「失败不动源文件」承诺一致。
3. **CHANGELOG 条目内容不自动编造。** 脚本只写入可验证的事实（版本号、日期、git commit），正文留占位并支持 `--note` 传入。从 commit message 猜测变更摘要会产出与实际不符的发布说明。

## Capabilities

### New Capabilities

- `release-versioning`: 正式打包时的补丁号自增、多文件版本对齐、CHANGELOG 顶部插入、以及自增与测试闸门的先后次序约束。

### Modified Capabilities

（无。既有三个 `reasoning-*` 能力不受影响。）

## Impact

**构建**

- `build_all.bat`：`:prepare_output` 拆成 `:prepare_log`（建日志、并发守卫）与 `:prepare_release_dir`（建版本目录），因为版本目录名依赖自增后的版本号，而测试步骤的日志重定向依赖日志先存在。新增 `:bump_version` 步骤。
- 新增环境变量逃生口 `PROVIDER_DECK_SKIP_BUMP=1`：用于「同一版本重跑打包」的场景（例如上一次在打包阶段失败）。设置后跳过自增，仍走 `check-version.mjs` 核对。

**脚本**

- 新增 `scripts/bump-version.mjs`、`scripts/bump-version.test.mjs`。
- `scripts/check-version.mjs`、`scripts/write-release-docs.mjs` 不改。

**测试**

- Vitest 基线 154 例 / 7 文件 → 新增纯函数用例若干，文件数 +1。`scripts/**` 不在 `tsconfig.app.json` 的 `include` 里，也不在 eslint 的 `files`（只匹配 `**/*.{ts,tsx}`）里，因此 `.mjs` 脚本的唯一自动化闸门是 vitest。
- Rust 单测、Playwright 不受影响。

**不受影响**

应用运行时逻辑零改动：`src/`、`src-tauri/src/` 全部不动。推理档位、实时请求链路、Codex 鉴权、客户端模块、`SupportLevel`、`config.rs` 写入管线均不涉及。
