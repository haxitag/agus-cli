# Agus CLI

Agus CLI 开源仓库（从主仓库 `zhyr/Agus` 同步 CLI 相关代码）。

## 下载

Releases: https://github.com/haxitag/agus-cli/releases

```bash
# 示例：下载并安装（以 macOS aarch64 为例）
tar -xzf agus-cli-0.2.1-macos-aarch64.tar.gz
cd agus-cli-0.2.1-macos-aarch64
bash install_cli.sh
```

## 常用命令

```bash
agus --help
agus host list
agus host check --id <host-id>
agus exec <host-id> "uptime"
agus --format json host list
```
