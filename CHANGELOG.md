# Changelog

本文件由 `build_all.bat` 的正式打包流程自动追加版本小节，最新版本在最上方。
补丁号由打包流程自增；MINOR 与 MAJOR 需按 HANDOFF.md 的版本升级规则手工调整。

## 0.1.12 - 2026-08-13

- 新增正式打包时的补丁号自动自增：`scripts/bump-version.mjs` 以 `package.json` 为唯一输入，
  同步对齐 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、
  `src-tauri/Cargo.lock` 四处版本号，并在本文件顶部插入版本小节。
- `build_all.bat` 步骤 10 → 12：新增 `:bump_version`（排在测试闸门之后，闸门失败则版本号不动），
  原 `:prepare_output` 拆为 `:prepare_log` 与 `:prepare_release_dir`。
- 新增 `PROVIDER_DECK_SKIP_BUMP=1` 逃生口，用于同一版本重跑打包。
- 发布包新增 CHANGELOG.md，并纳入 SHA256SUMS.txt 校验范围。
- 应用运行时逻辑无改动。

Git commit: f834d06280a737e95ea5c19e30087fb4b2605b8a
