# agus-cli

## 下载安装包

Releases（含 **Agus CLI** 与 **Agus GUI** 安装包）：https://github.com/haxitag/agus-cli/releases

### CLI 安装

```bash
# 拉取 CLI 包（版本号以 Releases 页面为准）
tar -xzf agus-cli-0.2.1-macos-aarch64.tar.gz
cd agus-cli-0.2.1-macos-aarch64
bash install_cli.sh
```

### GUI 安装

下载 `Agus_<version>_aarch64.dmg`，双击挂载后将 `Agus.app` 拖入「应用程序」。

## 激活与配额

通过 https://www.haxitag.com/articles/Agus 获取说明，或关注哈希泰格公众号获取激活码（在公众号发送 `agus`）。

## 常用命令

```bash
agus --help
agus host list
agus host check --id <host-id>
agus exec <host-id> "uptime"
agus --format json host list
```

| 功能 | 命令 |
|-----|------|
| 帮助 | `agus --help` |
| 主机管理 | `agus host list/show/check` |
| 执行命令 | `agus exec <host-id> "命令"` |
| 查看日志 | `agus logs <host-id>` |
| 监控 | `agus monitor <host-id>` |
