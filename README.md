<div align="center">
  <img src="src-tauri/icons/icon.png" alt="Ctrl-CCCC Logo" width="96" />

  <h1>Ctrl-CCCC</h1>

  <p>轻量级跨平台剪贴板变化通知工具</p>

  [![Rust](https://img.shields.io/badge/Rust-1.77%2B-DEA584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
  [![Tauri](https://img.shields.io/badge/Tauri-v2-24C8D8?logo=tauri&logoColor=white)](https://v2.tauri.app/)
  [![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-4A90E2)](#)
  [![UI](https://img.shields.io/badge/UI-HTML%20%7C%20CSS%20%7C%20JS-E34F26)](#)
  [![Clipboard](https://img.shields.io/badge/Clipboard-clipboard--rs%20%2B%20arboard-7C3AED)](#)
  [![Version](https://img.shields.io/badge/Version-0.2.1-blue)](#)
  [![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)
</div>

---

你是否遇到复制后粘贴发现没有复制成功？

你是否遇到跨设备剪切板同步失败但误认为已复制成功？

Ctrl-CCCC 是一款基于 Rust 和 Tauri 的轻量级跨平台剪贴板变化通知工具，能在复制内容后进行通知。

> [!NOTE]
> macOS 和 Linux 未经过测试

## 行为

- 剪切板发生变化时自动弹窗提示
- 文本最多显示 200 个字符，超出以 `…` 结束
- 图片显示缩略图和分辨率
- 文件显示 `文件：[文件名] 等 N 个`
- 空内容显示 `（空）`
- 可选择开启剪切板功能
- 剪贴板历史默认保留 50 条，可设置为 5-500 条

## 设置

左键托盘图标，或托盘菜单里的 `主菜单`，可修改：

- 通知位置
- 显示时长
- 淡入淡出时间
- 开机自动启动
- 历史最多保留条数
- 快捷输入快捷键

配置保存在系统配置目录：

- Windows: `%APPDATA%\ctrl-cccc\ctrl-cccc.json`
- macOS: `~/Library/Application Support/ctrl-cccc/ctrl-cccc.json`
- Linux: `$XDG_CONFIG_HOME/ctrl-cccc/ctrl-cccc.json` 或 `~/.config/ctrl-cccc/ctrl-cccc.json`

配置格式：

```json
{
  "position": "bottom-right",
  "duration_ms": 2200,
  "fade_ms": 180,
  "autostart": false,
  "history_limit": 50,
  "quick_hotkey": "super+c"
}
```

`position` 支持 `top-left`、`top-right`、`bottom-left`、`bottom-right`。

## 开发

```powershell
cargo install tauri-cli --version "^2" --locked
cargo tauri dev
```

## 打包

```powershell
cargo tauri build
```

只输出原始可执行文件：

```powershell
cargo tauri build --no-bundle
```

## 开源协议

本项目基于 [MIT License](./LICENSE) 开源。


