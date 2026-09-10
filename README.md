# agus-cli

这里是 Agus CLI 开源发布，详见 [Agus Agent 产品介绍](https://www.haxitag.com/articles/Agus)。

Agus CLI 与 Agus Agent 协同，实现基于 LLM、Agent 的 OPS、SRE 工作自动化与智能化，降低部署、运维、监控和数据分析中的重复操作，帮助工程师在 AI 辅助下构建数据洞察驱动的 SRE 分析能力。

## 下载安装包

Releases（含 **Agus CLI**、**Agus GUI** 与 **HaxiTAG Base** 安装包）：https://github.com/haxitag/agus-cli/releases

### CLI 安装

```bash
# 拉取 CLI 包（版本号以 Releases 页面为准）
tar -xzf agus-cli-0.2.1-macos-aarch64.tar.gz
cd agus-cli-0.2.1-macos-aarch64
bash install_cli.sh
```

CLI 安装成功后，可使用以下命令：

```bash
# 查看帮助
agus --help

# 查看主机列表
agus host list

# 连通性检查
agus host check --id <host-id>

# 执行命令
agus exec <host-id> "uptime"

# JSON 输出
agus --format json host list
```

### GUI 安装

下载 `Agus_<version>_aarch64.dmg`，双击挂载后将 `Agus.app` 拖入「应用程序」。

### HaxiTAG Base 安装

HaxiTAG Base 是轻量、性能优先的 macOS 多工作区 / 多标签 / 多面板终端，面向开发者并行多任务场景，与 Agus 一起在同一 Releases 页面独立发版：在 Releases 页面下载 HaxiTAG Base 的 `.dmg` 安装包（当前 v0.1.2，Apple Silicon），双击挂载后将 `HaxiTAG Base.app` 拖入「应用程序」。安装包已用 Developer ID 签名并通过 Apple 公证，首次打开无需额外放行。

## HaxiTAG Base 使用说明

- **工作区**：左侧栏为工作区列表（可拖动调整宽度，范围 100–320px）。用「新建工作区」将不同项目 / 任务隔离，点右上角按钮切换窗口排布与打开方式。
- **标签页与面板**：每个工作区内可开多个标签页，标签页可再分屏为多个可调大小的终端面板，并支持多种布局预设。
- **工具箱与历史命令**：`⌘⇧P` 打开工具箱抽屉，内置按类别整理的高频命令（系统 / 文件 / Git / 构建 / 网络 / 远程 / AI-MCP / 清理），支持自定义；`⌘⇧H` 打开历史命令以便快速复用。

| 功能 | 快捷键 |
|------|--------|
| 工具箱 / 快捷命令 | `⌘⇧P` |
| 历史命令 | `⌘⇧H` |
| 设置 | `⌘,` |
| 帮助（全部快捷键） | `⌘/` |
| 新建标签页 | `⌘T` |
| 关闭标签页 | `⌘W` |
| 上一个 / 下一个标签页 | `⌘⇧[` / `⌘⇧]` |
| 直接切换到第 1–9 个标签页 | `⌃1–9` |
| 复制 / 粘贴 | `⌘C` / `⌘V` |

## 激活与配额

通过 https://www.haxitag.com/articles/Agus 获取说明，或关注哈希泰格公众号获取激活码（在公众号发送 `agus`）。

## 常用命令速查

| 功能 | 命令 |
|-----|------|
| 帮助 | `agus --help` 或 `agus <command> --help` |
| 主机管理 | `agus host list/show/check` |
| 执行命令 | `agus exec <host-id> "命令"` |
| 查看日志 | `agus logs <host-id>` |
| 监控 | `agus monitor <host-id>` |
