<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="96" alt="">
</p>

<h1 align="center">兔啃</h1>

<p align="center">兔啃的 Windows 和 macOS 客户端：登录、充值、管理密钥，一键接入 Claude Desktop。</p>

## 下载

到 [Releases](https://github.com/Kyrie-num/tuken-desktop/releases/latest) 下载安装包：

- **Windows**：`Tuken_版本号_x64-setup.exe`，双击安装。支持 Windows 10 和 11（64 位）。安装包还没有代码签名，Windows 可能会提示「Windows 已保护你的电脑」：点「更多信息」，再点「仍要运行」。
- **macOS**：`Tuken_版本号_universal.dmg`，打开后把兔啃拖进「应用程序」。支持 macOS 13 及以上，Apple 芯片和 Intel 都能用。安装包还没有经过 Apple 公证，第一次打开时 macOS 会拦下：到「系统设置 → 隐私与安全性」，在最下面点「仍要打开」。

有新版本时兔啃会提示，点一下就能更新。

也可以在 PowerShell 里运行下面这行，自动下载最新版并打开安装程序：

```powershell
irm https://download.tuken.site/install.ps1 | iex
```

## 能做什么

- 用邮箱登录、注册，多个账户随时切换。
- 查看余额、用量和充值记录；扫码充值，到账后自动显示。
- 新建和管理密钥。
- 一键接入 Claude Desktop，选好默认模型和推理程度；也能接入 Claude Code。
- 每日签到、邀请好友、查看公告。

## 会改动电脑上的哪些设置

- **Claude Desktop**：接入时在它的配置里添加一个叫「兔啃」的配置，不会覆盖其他工具（比如 CC Switch）的配置。断开时移除。
- **Claude Code**：接入时在 `~/.claude/settings.json` 里写入兔啃的地址和密钥，并去掉指向其他服务的设置。断开时删掉兔啃的设置。
- 第一次修改这些文件之前，兔啃会先备份原文件。
- 登录信息加密保存在本机：Windows 上用系统自带的加密，只有你的 Windows 账户能读取；macOS 上的密钥放在「钥匙串」里。
- 在 macOS 上，Claude Desktop 的配置在 `~/Library/Application Support/Claude-3p`。

## 从源码构建

需要 Node.js 22、pnpm 10、Rust 1.95，以及 Visual Studio 生成工具（C++，Windows）或 Xcode 命令行工具（macOS）。

```bash
pnpm install
pnpm tauri dev
```

打包安装程序用 `pnpm tauri build`，测试用 `pnpm test`。

## 说明

兔啃不是 Anthropic 的官方产品。Claude 是 Anthropic 的商标。

## 开源协议

[MIT](LICENSE)
