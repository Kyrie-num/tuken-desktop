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
- Windows 一键安装会同时准备桌面程序和首次使用 Code 所需的运行组件，下载、校验和安装进度都在兔啃里显示。遇到“Couldn't download Claude Code”，更新兔啃后点击「修复运行组件」，或在 Claude Desktop 卡片的更多操作中选择「修复 Claude Code 运行组件」。
- 每日签到、邀请好友、查看公告。

## 会改动电脑上的哪些设置

- **Claude Desktop**：接入时在它的配置里添加一个叫「兔啃」的配置，不会覆盖其他工具（比如 CC Switch）的配置。断开时移除。
- Windows 的桌面 Code 运行组件放在 `%LOCALAPPDATA%/Claude-3p/claude-code`。版本和校验值取自已安装的官方桌面程序；镜像保留官方文件原样，兔啃校验后才写入完成标记。安装失败可以重试，不需要重新配置密钥。
- **Claude Code**：接入时在 `~/.claude/settings.json` 里写入兔啃的地址和密钥，并去掉指向其他服务的设置。断开时删掉兔啃的设置。
- 第一次修改这些文件之前，兔啃会先备份原文件。
- 登录信息加密保存在本机：Windows 上用系统自带的加密，只有你的 Windows 账户能读取；macOS 上的密钥放在「钥匙串」里。兔啃还没有 Apple 签名，每次更新后 macOS 会问一次兔啃能否使用钥匙串里的这把密钥，输入电脑密码后点「始终允许」。
- 在 macOS 上，Claude Desktop 的配置在 `~/Library/Application Support/Claude-3p`。

## 从源码构建

需要 Node.js 22、pnpm 10、Rust 1.95，以及 Visual Studio 生成工具（C++，Windows）或 Xcode 命令行工具（macOS）。

```bash
pnpm install
pnpm tauri dev
```

打包安装程序用 `pnpm tauri build`，测试用 `pnpm test`。

开发版（`pnpm tauri dev`）不改动这台电脑上 Claude Desktop 和 Claude Code 的配置。要试接入，先把 `TUKEN_HOME` 设成一个沙盒文件夹：macOS 上配置都会写进这个文件夹；Windows 上 Claude Desktop 的配置在 `%LOCALAPPDATA%`，要把 `LOCALAPPDATA` 也指过去。macOS 上开发版的账户数据和钥匙串项带 `.dev` 后缀，不碰正式版。

## 说明

兔啃不是 Anthropic 的官方产品。Claude 是 Anthropic 的商标。

## 开源协议

[MIT](LICENSE)
