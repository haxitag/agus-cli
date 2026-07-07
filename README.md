# agus-cli

这里是 Agus CLI 开源发布，详见 [Agus Agent 产品介绍](https://www.haxitag.com/articles/Agus)。

Agus CLI 与 Agus Agent 协同，实现基于 LLM、Agent 的 OPS、SRE 工作自动化与智能化，降低部署、运维、监控和数据分析中的重复操作，帮助工程师在 AI 辅助下构建数据洞察驱动的 SRE 分析能力。

## 下载安装包

Releases（含 **Agus CLI** 与 **Agus GUI** 安装包）：https://github.com/haxitag/agus-cli/releases

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
