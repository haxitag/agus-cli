//! 主机系统面板：对齐 FinalShell「系统信息」视图（基础/CPU/内存交换/网卡/文件系统）。
use agus_ssh::{SshClient, SshError, SshTarget};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostSystemPanel {
    pub hostname: String,
    pub os_name: String,
    pub kernel_name: String,
    pub kernel_release: String,
    pub arch: String,
    pub cpu: HostCpuDetail,
    pub cpu_usage: HostCpuUsageBreakdown,
    pub memory: HostMemBlock,
    pub swap: HostMemBlock,
    pub network: Vec<HostNetIface>,
    pub filesystems: Vec<HostFilesystem>,
    pub collected_at: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostCpuDetail {
    pub model: String,
    pub cores: u32,
    pub mhz: f64,
    pub cache_kb: f64,
    pub bogomips: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostCpuUsageBreakdown {
    pub user: f64,
    pub system: f64,
    pub nice: f64,
    pub idle: f64,
    pub iowait: f64,
    pub irq: f64,
    pub softirq: f64,
    pub steal: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostMemBlock {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostNetIface {
    pub name: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_bps: f64,
    pub rx_bps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostFilesystem {
    pub device: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
    pub mount_point: String,
}

/// 远端一次脚本采集图1所需字段（含 1s CPU/网卡差分），避免多路 SSH 风暴。
const PANEL_SCRIPT: &str = r#"
set +e
echo '###BASIC###'
echo "hostname=$(hostname 2>/dev/null)"
if [ -f /etc/os-release ]; then
  . /etc/os-release
  echo "os_name=${PRETTY_NAME:-$NAME}"
else
  echo "os_name=$(uname -s 2>/dev/null)"
fi
echo "kernel_name=$(uname -s 2>/dev/null)"
echo "kernel_release=$(uname -r 2>/dev/null)"
echo "arch=$(uname -m 2>/dev/null)"
echo '###CPU###'
model=$(awk -F: '/model name|Hardware|Processor/{gsub(/^ +/,"",$2); print $2; exit}' /proc/cpuinfo 2>/dev/null)
echo "model=${model:-Unknown}"
echo "cores=$(nproc 2>/dev/null || echo 1)"
mhz=$(awk -F: '/cpu MHz/{gsub(/^ +/,"",$2); print $2; exit}' /proc/cpuinfo 2>/dev/null)
echo "mhz=${mhz:-0}"
cache=$(awk -F: '/cache size/{gsub(/^ +| KB/,"",$2); print $2; exit}' /proc/cpuinfo 2>/dev/null)
echo "cache_kb=${cache:-0}"
bogo=$(awk -F: '/bogomips|BogoMIPS/{gsub(/^ +/,"",$2); print $2; exit}' /proc/cpuinfo 2>/dev/null)
echo "bogomips=${bogo:-0}"
echo '###CPU1###'
awk '/^cpu /{print $2,$3,$4,$5,$6,$7,$8,$9; exit}' /proc/stat 2>/dev/null
echo '###NET1###'
awk 'NR>2{gsub(/:/,"",$1); print $1,$2,$10}' /proc/net/dev 2>/dev/null
sleep 1
echo '###CPU2###'
awk '/^cpu /{print $2,$3,$4,$5,$6,$7,$8,$9; exit}' /proc/stat 2>/dev/null
echo '###NET2###'
awk 'NR>2{gsub(/:/,"",$1); print $1,$2,$10}' /proc/net/dev 2>/dev/null
echo '###MEM###'
awk '
/^MemTotal:/{t=$2}
/^MemAvailable:/{a=$2}
/^MemFree:/{f=$2}
/^Buffers:/{b=$2}
/^Cached:/{c=$2}
/^SwapTotal:/{st=$2}
/^SwapFree:/{sf=$2}
END{
  if(a=="") a=f+b+c;
  used=t-a; if(used<0) used=0;
  su=st-sf; if(su<0) su=0;
  print "mem_total_kb=" t;
  print "mem_used_kb=" used;
  print "mem_free_kb=" a;
  print "swap_total_kb=" st;
  print "swap_used_kb=" su;
  print "swap_free_kb=" sf;
}' /proc/meminfo 2>/dev/null
echo '###DISK###'
df -PB1 2>/dev/null | awk 'NR>1 && $1 !~ /^(tmpfs|devtmpfs|overlay)$/ {print $1,$2,$3,$4,$5,$6}'
"#;

pub fn collect_host_system_panel<C: SshClient>(
    client: &C,
    target: &SshTarget,
) -> Result<HostSystemPanel, SshError> {
    let output = client.execute(target, PANEL_SCRIPT)?;
    let panel = parse_host_system_panel(&output.stdout);
    if panel.hostname.is_empty() && panel.os_name.is_empty() {
        return Err(SshError::Command {
            exit_code: -1,
            stderr: "host system panel empty after SSH: refusing all-zero default as success"
                .to_string(),
        });
    }
    Ok(panel)
}

pub fn parse_host_system_panel(raw: &str) -> HostSystemPanel {
    let mut panel = HostSystemPanel::default();
    let mut section = "";
    let mut cpu1: Option<[u64; 8]> = None;
    let mut cpu2: Option<[u64; 8]> = None;
    let mut net1: Vec<(String, u64, u64)> = Vec::new();
    let mut net2: Vec<(String, u64, u64)> = Vec::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    panel.collected_at = now;

    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with("###") && line.ends_with("###") {
            section = line;
            continue;
        }
        if line.is_empty() {
            continue;
        }
        match section {
            "###BASIC###" => {
                if let Some((k, v)) = line.split_once('=') {
                    match k {
                        "hostname" => panel.hostname = v.to_string(),
                        "os_name" => panel.os_name = v.to_string(),
                        "kernel_name" => panel.kernel_name = v.to_string(),
                        "kernel_release" => panel.kernel_release = v.to_string(),
                        "arch" => panel.arch = v.to_string(),
                        _ => {}
                    }
                }
            }
            "###CPU###" => {
                if let Some((k, v)) = line.split_once('=') {
                    match k {
                        "model" => panel.cpu.model = v.to_string(),
                        "cores" => panel.cpu.cores = v.parse().unwrap_or(1),
                        "mhz" => panel.cpu.mhz = v.parse().unwrap_or(0.0),
                        "cache_kb" => panel.cpu.cache_kb = v.parse().unwrap_or(0.0),
                        "bogomips" => panel.cpu.bogomips = v.parse().unwrap_or(0.0),
                        _ => {}
                    }
                }
            }
            "###CPU1###" => {
                cpu1 = parse_cpu_counters(line);
            }
            "###CPU2###" => {
                cpu2 = parse_cpu_counters(line);
            }
            "###NET1###" => {
                if let Some(row) = parse_net_row(line) {
                    net1.push(row);
                }
            }
            "###NET2###" => {
                if let Some(row) = parse_net_row(line) {
                    net2.push(row);
                }
            }
            "###MEM###" => {
                if let Some((k, v)) = line.split_once('=') {
                    let n: u64 = v.parse().unwrap_or(0);
                    let bytes = n.saturating_mul(1024);
                    match k {
                        "mem_total_kb" => panel.memory.total_bytes = bytes,
                        "mem_used_kb" => panel.memory.used_bytes = bytes,
                        "mem_free_kb" => panel.memory.free_bytes = bytes,
                        "swap_total_kb" => panel.swap.total_bytes = bytes,
                        "swap_used_kb" => panel.swap.used_bytes = bytes,
                        "swap_free_kb" => panel.swap.free_bytes = bytes,
                        _ => {}
                    }
                }
            }
            "###DISK###" => {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 6 {
                    let total: u64 = parts[1].parse().unwrap_or(0);
                    let used: u64 = parts[2].parse().unwrap_or(0);
                    let available: u64 = parts[3].parse().unwrap_or(0);
                    let pct = parts[4].trim_end_matches('%').parse().unwrap_or(0.0);
                    panel.filesystems.push(HostFilesystem {
                        device: parts[0].to_string(),
                        filesystem: String::new(),
                        total_bytes: total,
                        used_bytes: used,
                        available_bytes: available,
                        usage_percent: pct,
                        mount_point: parts[5..].join(" "),
                    });
                }
            }
            _ => {}
        }
    }

    if let (Some(a), Some(b)) = (cpu1, cpu2) {
        panel.cpu_usage = cpu_usage_from_delta(a, b);
    } else {
        panel
            .warnings
            .push("CPU 占用差分采样失败".to_string());
    }

    panel.memory.usage_percent = pct(panel.memory.used_bytes, panel.memory.total_bytes);
    panel.swap.usage_percent = pct(panel.swap.used_bytes, panel.swap.total_bytes);

    let net2_map: std::collections::HashMap<&str, (u64, u64)> = net2
        .iter()
        .map(|(n, rx, tx)| (n.as_str(), (*rx, *tx)))
        .collect();
    for (name, rx1, tx1) in net1 {
        let (rx2, tx2) = net2_map.get(name.as_str()).copied().unwrap_or((rx1, tx1));
        panel.network.push(HostNetIface {
            name,
            rx_bytes: rx2,
            tx_bytes: tx2,
            rx_bps: (rx2.saturating_sub(rx1)) as f64,
            tx_bps: (tx2.saturating_sub(tx1)) as f64,
        });
    }

    if panel.hostname.is_empty() && panel.os_name.is_empty() {
        panel
            .warnings
            .push("系统基础信息为空，请检查 SSH 输出".to_string());
    }

    panel
}

fn parse_cpu_counters(line: &str) -> Option<[u64; 8]> {
    let parts: Vec<u64> = line
        .split_whitespace()
        .filter_map(|p| p.parse().ok())
        .collect();
    if parts.len() < 8 {
        return None;
    }
    Some([
        parts[0], parts[1], parts[2], parts[3], parts[4], parts[5], parts[6], parts[7],
    ])
}

fn cpu_usage_from_delta(a: [u64; 8], b: [u64; 8]) -> HostCpuUsageBreakdown {
    let d: Vec<f64> = (0..8)
        .map(|i| b[i].saturating_sub(a[i]) as f64)
        .collect();
    let total: f64 = d.iter().sum();
    let pct = |v: f64| {
        if total > 0.0 {
            (v / total) * 100.0
        } else {
            0.0
        }
    };
    HostCpuUsageBreakdown {
        user: pct(d[0]),
        nice: pct(d[1]),
        system: pct(d[2]),
        idle: pct(d[3]),
        iowait: pct(d[4]),
        irq: pct(d[5]),
        softirq: pct(d[6]),
        steal: pct(d[7]),
    }
}

fn parse_net_row(line: &str) -> Option<(String, u64, u64)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    Some((
        parts[0].trim_end_matches(':').to_string(),
        parts[1].parse().unwrap_or(0),
        parts[2].parse().unwrap_or(0),
    ))
}

fn pct(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_finalshell_like_panel() {
        let raw = r#"
###BASIC###
hostname=VM-24-2-opencloudos
os_name=OpenCloudOS release 9.2
kernel_name=Linux
kernel_release=6.6.47-12.oc9.x86_64
arch=x86_64
###CPU###
model=AMD EPYC 7K62 48-Core Processor
cores=2
mhz=2595.112
cache_kb=512
bogomips=5190.22
###CPU1###
100 0 50 1000 10 0 0 0
###NET1###
eth0 1000 2000
lo 10 10
###CPU2###
112 0 70 1967 12 0 0 0
###NET2###
eth0 1250 2164
lo 20 20
###MEM###
mem_total_kb=1782579
mem_used_kb=960512
mem_free_kb=788480
swap_total_kb=2097152
swap_used_kb=597000
swap_free_kb=1500152
###DISK###
/dev/vda1 53582299136 40802189312 12780109824 76 /
"#;
        let panel = parse_host_system_panel(raw);
        assert_eq!(panel.hostname, "VM-24-2-opencloudos");
        assert!(panel.os_name.contains("OpenCloudOS"));
        assert_eq!(panel.cpu.cores, 2);
        assert!((panel.cpu.mhz - 2595.112).abs() < 0.01);
        assert!(panel.cpu_usage.idle > 80.0);
        assert_eq!(panel.network[0].name, "eth0");
        assert!(panel.network[0].rx_bps > 0.0);
        assert_eq!(panel.filesystems[0].mount_point, "/");
        assert!((panel.filesystems[0].usage_percent - 76.0).abs() < 0.1);
    }
}
