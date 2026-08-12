// 生成 release 目录下的 README.txt 与 release-summary.txt。
//
// 为什么不写在 build_all.bat 里：README 是中文八段正文，批处理的活动代码页
// （中文 Windows 上是 936）会把 UTF-8 源码当 GBK 解，echo 出来必然乱码。
// Node 直接以 utf8 写文件，绕开代码页这一层。
//
// 用法：node scripts/write-release-docs.mjs <releaseDir> <version>
// 其余元数据（git hash / rust / node 版本）由本脚本自行采集，
// 避免在 bat 里再套一层 for /f 转义。

import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const [releaseDir, version] = process.argv.slice(2);

if (!releaseDir || !version) {
  console.error("用法：node scripts/write-release-docs.mjs <releaseDir> <version>");
  process.exit(1);
}

/** 取一行命令输出；采集失败不该让整个发布中断，退化成 unknown。 */
function probe(command, args) {
  try {
    return execFileSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] })
      .split("\n")[0]
      .trim();
  } catch {
    return "unknown";
  }
}

const PORTABLE_EXE = `ProviderDeck-Portable-${version}-x64.exe`;
const SETUP_EXE = `ProviderDeck-Setup-${version}-x64.exe`;

const README = `Provider Deck ${version} Windows x64

一、简介

Provider Deck 是本地优先的 AI Provider 管理工具，用于探测第三方模型服务、
生成客户端配置、备份与恢复设置。所有数据保存在本机，API Key 存入操作系统
凭据库，不写入状态文件、日志、导出文件或诊断信息。

二、系统要求

- Windows 10 x64 或 Windows 11 x64
- Microsoft Edge WebView2 Runtime
  Windows 11 已内置。Windows 10 若缺少该组件，程序启动会提示，
  可从 https://developer.microsoft.com/microsoft-edge/webview2/ 安装。

三、便携版使用方法

1. 下载 ${PORTABLE_EXE}
2. 放到任意目录，无需解压。
3. 双击运行。

是否单文件运行：是。便携版为单个 EXE，不需要同目录的 DLL、
资源文件或 sidecar 程序。唯一的外部依赖是第二节所述的 WebView2 Runtime，
它属于操作系统组件，不随本程序分发。

便携版不写注册表，卸载时直接删除该 EXE 即可，用户数据按第五节单独清理。

四、安装版使用方法

1. 下载 ${SETUP_EXE}
2. 双击运行，按向导操作。安装过程可自行选择安装路径。
3. 安装后在开始菜单中可以找到 Provider Deck 快捷方式。
4. 卸载：在「设置 - 应用 - 已安装的应用」中选择 Provider Deck 卸载，
   或使用安装目录下的卸载程序。

升级前请先退出正在运行的 Provider Deck。

五、数据保存位置

- 配置与状态：%APPDATA%\\ProviderDeck\\Provider Deck\\config\\state.json
  含 Provider 列表、模型信息、推理档位选择、验证历史、备份记录和应用设置。
- API Key：Windows 凭据管理器，服务名 cn.providerdeck.desktop
  不在 state.json 中，也不随导出文件一起走。
- 客户端配置备份：由程序在写入第三方客户端配置前自动创建，
  路径记录在 state.json 的 backups 字段里。

便携版与安装版共用上述位置，同一台机器上两种形态看到的是同一份数据。

六、Provider 配置备份方法

1. 备份：复制第五节的 state.json 到安全位置即可。
2. 恢复：退出 Provider Deck，将 state.json 复制回原路径，再启动程序。
3. API Key 不在 state.json 里。换机器或重装系统后需要重新填写各 Provider
   的 API Key，其余配置会随 state.json 一起回来。
4. state.json 可以直接用文本编辑器查看。程序读取时会忽略无法识别的字段，
   旧版本导出的文件可以被新版本加载。

七、Runtime Verification 功能说明

Runtime Verification 是用户主动发起的一次真实请求，用来确认某个模型在某个
endpoint 上是否真的接受推理参数。

- Discovery 与 Verification 相互分离。Discovery 探测模型声明的能力，
  Verification 记录一次真实请求的结果，两者数据独立保存，互不覆盖。
- 验证状态三种：
  Confirmed  该次请求返回了推理响应。
  Rejected   服务端明确拒绝了这组推理参数。
  Failed     请求本身没有完成，例如超时、网络错误或鉴权失败。
- Rejected 与 Failed 都不等于「不支持推理」。Rejected 说明这组参数不被接受，
  Failed 说明这次没问到结果，都不构成对模型能力的结论。
- Runtime Verification 不修改 capability confidence。验证结果只追加到验证
  历史里，不会改写 Discovery 得到的能力判定，也不会提升置信度档位。
- 验证记录只保存 endpoint、模型 ID、推理档位、参数形态、结果和时间，
  不保存 API Key、请求内容或响应全文。

八、已知问题

1. 浏览器测试后端（BrowserBackend.saveProvider）保存 Provider 时不携带
   reasoningVerifications 字段。仅影响开发期的浏览器/E2E 环境，
   桌面版走 Tauri 后端，验证历史正常保存。
2. reasoning_capability.rs 存在 dead_code 编译警告，涉及仅被测试代码调用的
   两个辅助函数。不影响运行。
`;

const SUMMARY = `Provider Deck Release Summary

产品名称: Provider Deck
版本号: ${version}
Build 时间: ${new Date().toISOString()}
Git commit: ${probe("git", ["rev-parse", "HEAD"])}
Rust version: ${probe("rustc", ["--version"])}
Cargo version: ${probe("cargo", ["--version"])}
Node version: ${probe(process.execPath, ["--version"])}
`;

writeFileSync(join(releaseDir, "README.txt"), README, "utf8");
writeFileSync(join(releaseDir, "release-summary.txt"), SUMMARY, "utf8");

console.log(`README.txt 与 release-summary.txt 已写入 ${releaseDir}`);
