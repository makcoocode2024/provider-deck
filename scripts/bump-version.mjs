// 正式打包时把补丁号 +1，并把新版本号同步写进所有携带版本号的文件。
//
// 为什么需要它：build_all.bat 的 :read_version 只核对"三处是否一致"，
// 挡得住漏改一处，挡不住三处一致但都是上一次发布的旧值。那种情况下第二个包
// 会沿用同一个版本号 —— NSIS 安装版覆盖不掉旧版、release 目录被同名覆盖、
// SHA256SUMS 对不上任何一次发布，而脚本全程 [OK]，因为"一致"确实成立。
//
// 为什么是四个文件而不是需求里说的两个：
//   package.json              发布文件名与 npm 元数据
//   src-tauri/Cargo.toml      exe 的文件属性
//   src-tauri/tauri.conf.json 安装包写进注册表的版本（check-version.mjs 已在核对它，
//                             只改两处会让 bat 在下一步自己失败）
//   src-tauri/Cargo.lock      provider-deck 自身条目（不改则每次发布后工作树变脏，
//                             将来若给 cargo 加 --locked 会直接构建失败）
//
// 用法：node scripts/bump-version.mjs [--dry-run] [--note "<摘要>"] [--out <file>]
//   --dry-run  只计算与打印，不落盘
//   --note     写进 CHANGELOG 的本次变更摘要；不给则留占位，不编造
//   --out      把新版本号单独写入该文件，供 bat 用 for /f 读取
//
// 落盘策略是全有或全无：四份新内容先在内存里全部算好，任一处算不出就中止，
// 一个字节都不写。半改状态比不改危险得多 —— 它能通过一部分核对。

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";

/** 读文本并剥掉 BOM。带 BOM 的 JSON 会让 JSON.parse 直接抛，报错指不到真正的原因。 */
export function readText(path) {
  return readFileSync(path, "utf8").replace(/^﻿/, "");
}

/**
 * 补丁号 +1。
 * 只接受三段纯数字：带预发布标识（0.1.12-rc.1）或构建元数据（0.1.12+build.5）时，
 * "下一个补丁号"是什么并无唯一答案 —— 与其猜，不如停下让人手改。
 */
export function bumpPatch(version) {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version);
  if (!match) {
    throw new Error(
      `版本号必须是三段纯数字（如 0.1.11），实际读到「${version}」。` +
        `带预发布标识或构建元数据的版本号请手工调整后再打包。`,
    );
  }
  const [, major, minor, patch] = match;
  return `${major}.${minor}.${Number(patch) + 1}`;
}

/** 从 JSON 文本里取顶层 version，不做结构假设之外的容错。 */
export function readJsonVersion(path) {
  const text = readText(path);
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(`${path} 解析失败：${error.message}`);
  }
  if (typeof parsed.version !== "string") {
    throw new Error(`${path} 缺少顶层 version 字段。`);
  }
  return parsed.version;
}

/**
 * 改写 JSON 的顶层 version，保留原有缩进与行尾。
 * 不走 JSON.parse + stringify：那会重排缩进、丢掉尾随换行，让 diff 里混进
 * 与版本号无关的整文件改动。锚定「行首两空格 + "version"」只命中顶层字段，
 * 不会误伤 dependencies 里同名的键（那些缩进更深）。
 */
export function replaceJsonVersion(text, oldVersion, newVersion) {
  const pattern = new RegExp(
    `(^[ \\t]*"version"[ \\t]*:[ \\t]*")${escapeRegExp(oldVersion)}(")`,
    "m",
  );
  if (!pattern.test(text)) {
    throw new Error(`找不到值为「${oldVersion}」的 version 字段。`);
  }
  return text.replace(pattern, `$1${newVersion}$2`);
}

/** Cargo.toml 只改 [package] 段的 version，避免命中依赖项的同名键。 */
export function replaceCargoPackageVersion(text, oldVersion, newVersion) {
  const lines = text.split("\n");
  let inPackage = false;
  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim();
    if (trimmed.startsWith("[")) {
      inPackage = trimmed === "[package]";
      continue;
    }
    if (!inPackage) continue;
    const match = /^version\s*=\s*"([^"]+)"/.exec(trimmed);
    if (!match) continue;
    if (match[1] !== oldVersion) {
      throw new Error(`[package] 段的 version 是「${match[1]}」，与预期的「${oldVersion}」不一致。`);
    }
    lines[i] = lines[i].replace(`"${oldVersion}"`, `"${newVersion}"`);
    return lines.join("\n");
  }
  throw new Error("Cargo.toml 的 [package] 段里找不到 version。");
}

/** 从 Cargo.toml 的 [package] 段读 version。 */
export function readCargoPackageVersion(text) {
  const lines = text.split(/\r?\n/);
  let inPackage = false;
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith("[")) {
      inPackage = trimmed === "[package]";
      continue;
    }
    if (!inPackage) continue;
    const match = /^version\s*=\s*"([^"]+)"/.exec(trimmed);
    if (match) return match[1];
  }
  return null;
}

/**
 * Cargo.lock 里只改 provider-deck 自己那个 [[package]] 条目。
 * 锁文件里有几百个条目都带 version 键，按行号或按值全局替换必然误伤依赖 ——
 * 所以先定位 name = "provider-deck"，再只改它后面紧邻的那个 version。
 */
export function replaceCargoLockVersion(text, packageName, oldVersion, newVersion) {
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i += 1) {
    if (lines[i].trim() !== `name = "${packageName}"`) continue;
    for (let j = i + 1; j < lines.length; j += 1) {
      const trimmed = lines[j].trim();
      // 条目边界：撞到下一个 [[package]] 说明这个条目没有 version 键。
      if (trimmed.startsWith("[[")) break;
      const match = /^version\s*=\s*"([^"]+)"/.exec(trimmed);
      if (!match) continue;
      if (match[1] !== oldVersion) {
        throw new Error(
          `Cargo.lock 中 ${packageName} 的 version 是「${match[1]}」，与预期的「${oldVersion}」不一致。`,
        );
      }
      lines[j] = lines[j].replace(`"${oldVersion}"`, `"${newVersion}"`);
      return lines.join("\n");
    }
    break;
  }
  throw new Error(`Cargo.lock 里找不到 ${packageName} 的 version。`);
}

/** 从 Cargo.lock 读指定包的 version。 */
export function readCargoLockVersion(text, packageName) {
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    if (lines[i].trim() !== `name = "${packageName}"`) continue;
    for (let j = i + 1; j < lines.length; j += 1) {
      const trimmed = lines[j].trim();
      if (trimmed.startsWith("[[")) break;
      const match = /^version\s*=\s*"([^"]+)"/.exec(trimmed);
      if (match) return match[1];
    }
    break;
  }
  return null;
}

const CHANGELOG_HEADER = `# Changelog

本文件由 \`build_all.bat\` 的正式打包流程自动追加版本小节，最新版本在最上方。
补丁号由打包流程自增；MINOR 与 MAJOR 需按 HANDOFF.md 的版本升级规则手工调整。
`;

/**
 * 在 CHANGELOG 顶部插入本次版本的小节。
 *
 * 正文刻意不从 commit message 推测：那样产出的发布说明与实际改动不符的概率很高，
 * 而发布说明是给用户看的。没有 --note 就留占位，让人补 —— 空着比编错好。
 */
export function prependChangelogEntry(existingText, { version, date, commit, note }) {
  const body = note && note.trim().length > 0 ? note.trim() : "- 待补充：本次发布的变更摘要尚未填写。";
  const entry = `## ${version} - ${date}\n\n${body}\n\nGit commit: ${commit}\n`;

  if (!existingText || existingText.trim().length === 0) {
    return `${CHANGELOG_HEADER}\n${entry}`;
  }

  // 已有内容：插到第一个版本小节之前，前言（# 标题与说明段）保持在最上方。
  const firstEntry = existingText.search(/^## /m);
  if (firstEntry === -1) {
    return `${existingText.replace(/\s*$/, "")}\n\n${entry}`;
  }
  const preamble = existingText.slice(0, firstEntry);
  const rest = existingText.slice(firstEntry);
  return `${preamble}${entry}\n${rest}`;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** 本地日期（YYYY-MM-DD）。发布说明给人看，用本机时区而非 UTC 更符合直觉。 */
export function localDate(now = new Date()) {
  const pad = (value) => String(value).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

export function parseArgs(argv) {
  const options = { dryRun: false, note: "", out: "" };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--dry-run") options.dryRun = true;
    else if (arg === "--note") options.note = argv[++i] ?? "";
    else if (arg === "--out") options.out = argv[++i] ?? "";
    else throw new Error(`无法识别的参数：${arg}`);
  }
  return options;
}

const PACKAGE_JSON = "package.json";
const CARGO_TOML = "src-tauri/Cargo.toml";
const TAURI_CONF = "src-tauri/tauri.conf.json";
const CARGO_LOCK = "src-tauri/Cargo.lock";
const CHANGELOG = "CHANGELOG.md";
const CRATE_NAME = "provider-deck";

function probeCommit() {
  try {
    // 同步执行 git 只为拿一行 hash；采集失败不该让发布中断。
    return execFileSync("git", ["rev-parse", "HEAD"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    })
      .split("\n")[0]
      .trim();
  } catch {
    return "unknown";
  }
}

function main(argv) {
  const options = parseArgs(argv);

  const packageText = readText(PACKAGE_JSON);
  const cargoText = readText(CARGO_TOML);
  const tauriText = readText(TAURI_CONF);
  const lockText = readText(CARGO_LOCK);

  const current = [
    [PACKAGE_JSON, readJsonVersion(PACKAGE_JSON)],
    [CARGO_TOML, readCargoPackageVersion(cargoText)],
    [TAURI_CONF, readJsonVersion(TAURI_CONF)],
    [CARGO_LOCK, readCargoLockVersion(lockText, CRATE_NAME)],
  ];

  const missing = current.filter(([, version]) => !version);
  if (missing.length > 0) {
    console.error("读不到版本号：");
    for (const [file] of missing) console.error(`  ${file}`);
    process.exit(1);
  }

  // 自增前先确认四处一致。不一致时用自己算出的值覆盖，等于替人做了
  // "哪个才是对的"这个决定 —— 那是操作者该看一眼的分歧。
  const unique = [...new Set(current.map(([, version]) => version))];
  if (unique.length > 1) {
    console.error("版本号不一致，自增中止：");
    for (const [file, version] of current) console.error(`  ${version}  ${file}`);
    console.error("请先把各处改成同一个版本号。");
    process.exit(1);
  }

  const oldVersion = unique[0];
  const newVersion = bumpPatch(oldVersion);

  // 四份新内容全部先算出来。任一处抛错就整体中止，磁盘保持原样。
  const nextPackage = replaceJsonVersion(packageText, oldVersion, newVersion);
  const nextTauri = replaceJsonVersion(tauriText, oldVersion, newVersion);
  const nextCargo = replaceCargoPackageVersion(cargoText, oldVersion, newVersion);
  const nextLock = replaceCargoLockVersion(lockText, CRATE_NAME, oldVersion, newVersion);
  const nextChangelog = prependChangelogEntry(existsSync(CHANGELOG) ? readText(CHANGELOG) : "", {
    version: newVersion,
    date: localDate(),
    commit: probeCommit(),
    note: options.note,
  });

  if (options.dryRun) {
    console.log(`[dry-run] 旧版本：${oldVersion}，新版本：${newVersion}`);
    console.log(`[dry-run] 将改写：${PACKAGE_JSON} / ${CARGO_TOML} / ${TAURI_CONF} / ${CARGO_LOCK}`);
    console.log(`[dry-run] 将在 ${CHANGELOG} 顶部插入 ${newVersion} 小节。未写入任何文件。`);
    return;
  }

  writeFileSync(PACKAGE_JSON, nextPackage, "utf8");
  writeFileSync(CARGO_TOML, nextCargo, "utf8");
  writeFileSync(TAURI_CONF, nextTauri, "utf8");
  writeFileSync(CARGO_LOCK, nextLock, "utf8");
  writeFileSync(CHANGELOG, nextChangelog, "utf8");

  // 回读自检。写入成功不等于内容正确 —— 正则命中错位置也会"写成功"。
  const after = [
    [PACKAGE_JSON, readJsonVersion(PACKAGE_JSON)],
    [CARGO_TOML, readCargoPackageVersion(readText(CARGO_TOML))],
    [TAURI_CONF, readJsonVersion(TAURI_CONF)],
    [CARGO_LOCK, readCargoLockVersion(readText(CARGO_LOCK), CRATE_NAME)],
  ];
  const wrong = after.filter(([, version]) => version !== newVersion);
  if (wrong.length > 0) {
    console.error(`自增后回读校验失败，期望 ${newVersion}：`);
    for (const [file, version] of wrong) console.error(`  ${version}  ${file}`);
    process.exit(1);
  }

  if (options.out) writeFileSync(options.out, `${newVersion}\n`, "utf8");

  console.log(`旧版本：${oldVersion}，新版本：${newVersion}`);
  console.log(`已对齐 4 个文件，并在 ${CHANGELOG} 顶部写入 ${newVersion} 小节。`);
}

// 入口守卫：测试 import 本模块时不该触发落盘。
if (process.argv[1] && process.argv[1].replace(/\\/g, "/").endsWith("scripts/bump-version.mjs")) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(`版本自增失败：${error.message}`);
    process.exit(1);
  }
}
