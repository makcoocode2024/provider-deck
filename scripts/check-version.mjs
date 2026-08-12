// 交叉核对三处版本号，并把结果打给 build_all.bat。
//
// 为什么要三处都核：package.json 决定发布文件名，tauri.conf.json 决定安装包内
// 写进注册表的版本，Cargo.toml 决定 exe 的文件属性。任一处漏改，发出去的包就会
// 自称两个版本号 —— 安装版覆盖不掉旧版、或者文件名与「关于」页对不上。
// 这种错在打完包之后极难发现，所以在编译前就停下。
//
// 输出：一致时 stdout 只有一行版本号（bat 用 for /f 取），退出码 0。
//       不一致时 stderr 写明差异，退出码 1。

import { readFileSync } from "node:fs";

/** 读文本并剥掉 BOM：带 BOM 的 JSON 会让 JSON.parse 直接抛，报错指不到真正的原因。 */
function readText(path) {
  return readFileSync(path, "utf8").replace(/^﻿/, "");
}

/** 解析失败时给出文件名，而不是让 JSON.parse 的栈冒到 bat 里。 */
function readJson(path) {
  try {
    return JSON.parse(readText(path));
  } catch (error) {
    console.error(`${path} 解析失败：${error.message}`);
    process.exit(1);
  }
}

/** Cargo.toml 只取 [package] 段的 version，避免命中依赖项的同名键。 */
function cargoVersion(text) {
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

const sources = [
  ["package.json", readJson("package.json").version],
  ["src-tauri/Cargo.toml", cargoVersion(readText("src-tauri/Cargo.toml"))],
  ["src-tauri/tauri.conf.json", readJson("src-tauri/tauri.conf.json").version],
];

const missing = sources.filter(([, version]) => !version);
if (missing.length > 0) {
  console.error("读不到版本号：");
  for (const [file] of missing) console.error(`  ${file}`);
  process.exit(1);
}

const unique = [...new Set(sources.map(([, version]) => version))];
if (unique.length > 1) {
  console.error("版本号不一致，发布中止：");
  for (const [file, version] of sources) console.error(`  ${version}  ${file}`);
  console.error("请把三处改成同一个版本号后重新运行。");
  process.exit(1);
}

console.log(unique[0]);
