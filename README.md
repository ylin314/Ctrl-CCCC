# Ctrl-CCCC

你是否遇到复制后粘贴发现没有复制成功的情况？

你是否遇到不确定是否复制成功的情况？

Ctrl-CCCC 是一款基于Rust和Tauri的轻量级跨平台剪贴板变化通知工具，能在复制内容后及时进行通知

## 行为

- 启动后收到系统托盘。
- 左键单击托盘图标：打开主菜单/设置窗口。
- 右键托盘图标：显示 `主菜单` 和 `退出`。
- 主菜单窗口显示时有任务栏图标。
- 关闭主菜单窗口不会退出软件，只是隐藏到托盘。
- 托盘 `退出` 才会真正结束程序。
- 文本最多显示 200 个字符，超出以 `…` 结束。
- 图片显示 `图片 [宽]x[高]`。
- 文件显示 `文件：[文件名] 等 N 个`。
- 空内容显示 `（空）`。

## 设置

左键托盘图标，或托盘菜单里的 `主菜单`，可修改：

- 通知位置
- 显示时长
- 淡入淡出时间
- 开机自动启动

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
  "autostart": false
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
