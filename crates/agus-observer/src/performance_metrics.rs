use agus_ssh::{SshClient, SshError, SshTarget};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub host_id: String,
    pub timestamp: u64,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub disk: Vec<DiskMetrics>,
    pub network: NetworkMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub usage_percent: f64,
    pub cores: u32,
    pub load_average: LoadAverage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadAverage {
    pub one_min: f64,
    pub five_min: f64,
    pub fifteen_min: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub total_mb: f64,
    pub used_mb: f64,
    pub free_mb: f64,
    pub cached_mb: f64,
    pub buffers_mb: f64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskMetrics {
    pub device: String,
    pub mount_point: String,
    pub total_gb: f64,
    pub used_gb: f64,
    pub available_gb: f64,
    pub usage_percent: f64,
    pub filesystem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub interfaces: Vec<NetworkInterface>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
}

/// 采集系统性能指标
pub fn collect_system_metrics<C: SshClient>(
    client: &C,
    target: &SshTarget,
    host_id: &str,
) -> Result<SystemMetrics, SshError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 采集CPU指标
    let cpu = collect_cpu_metrics(client, target)?;

    // 采集内存指标
    let memory = collect_memory_metrics(client, target)?;

    // 采集磁盘指标
    let disk = collect_disk_metrics(client, target)?;

    // 采集网络指标
    let network = collect_network_metrics(client, target)?;

    Ok(SystemMetrics {
        host_id: host_id.to_string(),
        timestamp,
        cpu,
        memory,
        disk,
        network,
    })
}

fn collect_cpu_metrics<C: SshClient>(
    client: &C,
    target: &SshTarget,
) -> Result<CpuMetrics, SshError> {
    // 获取CPU核心数
    let cores_cmd = "nproc";
    let cores_result = client.execute(target, cores_cmd)?;
    let cores = cores_result.stdout.trim().parse::<u32>().unwrap_or(1);

    // 获取CPU使用率（使用top命令，取1秒的平均值）
    let cpu_cmd = "top -bn1 | grep 'Cpu(s)' | sed 's/.*, *\\([0-9.]*\\)%* id.*/\\1/' | awk '{print 100 - $1}'";
    let cpu_result = client.execute(target, cpu_cmd).ok();
    let usage_percent = if let Some(result) = cpu_result {
        match result.stdout.trim().parse::<f64>() {
            Ok(v) => v,
            Err(_) => {
                let vmstat_cmd = "vmstat 1 2 | tail -1 | awk '{print 100 - $15}'";
                let vmstat_result = client.execute(target, vmstat_cmd)?;
                vmstat_result.stdout.trim().parse::<f64>().map_err(|_| SshError::Command {
                    exit_code: -1,
                    stderr: "failed to parse CPU usage from top/vmstat".to_string(),
                })?
            }
        }
    } else {
        let vmstat_cmd = "vmstat 1 2 | tail -1 | awk '{print 100 - $15}'";
        let vmstat_result = client.execute(target, vmstat_cmd)?;
        vmstat_result.stdout.trim().parse::<f64>().map_err(|_| SshError::Command {
            exit_code: -1,
            stderr: "failed to collect CPU usage (top unavailable, vmstat parse failed)"
                .to_string(),
        })?
    };

    // 获取负载平均值
    let loadavg_cmd = "cat /proc/loadavg | awk '{print $1, $2, $3}'";
    let loadavg_result = client.execute(target, loadavg_cmd)?;
    let load_parts: Vec<&str> = loadavg_result.stdout.trim().split_whitespace().collect();
    let load_average = if load_parts.len() >= 3 {
        LoadAverage {
            one_min: load_parts[0].parse().map_err(|_| SshError::Command {
                exit_code: -1,
                stderr: "failed to parse loadavg 1min".to_string(),
            })?,
            five_min: load_parts[1].parse().map_err(|_| SshError::Command {
                exit_code: -1,
                stderr: "failed to parse loadavg 5min".to_string(),
            })?,
            fifteen_min: load_parts[2].parse().map_err(|_| SshError::Command {
                exit_code: -1,
                stderr: "failed to parse loadavg 15min".to_string(),
            })?,
        }
    } else {
        return Err(SshError::Command {
            exit_code: -1,
            stderr: "failed to collect load average from /proc/loadavg".to_string(),
        });
    };

    Ok(CpuMetrics {
        usage_percent,
        cores,
        load_average,
    })
}

fn collect_memory_metrics<C: SshClient>(
    client: &C,
    target: &SshTarget,
) -> Result<MemoryMetrics, SshError> {
    // 使用free命令获取内存信息
    let free_cmd = "free -m | grep '^Mem:' | awk '{print $2, $3, $4, $6, $7}'";
    let free_result = client.execute(target, free_cmd)?;
    let parts: Vec<&str> = free_result.stdout.trim().split_whitespace().collect();

    if parts.len() >= 5 {
        let parse_f = |idx: usize, label: &str| -> Result<f64, SshError> {
            parts[idx].parse().map_err(|_| SshError::Command {
                exit_code: -1,
                stderr: format!("failed to parse memory {label} from free"),
            })
        };
        let total_mb = parse_f(0, "total")?;
        let used_mb = parse_f(1, "used")?;
        let free_mb = parse_f(2, "free")?;
        let buffers_mb = parse_f(3, "buffers")?;
        let cached_mb = parse_f(4, "cached")?;
        if total_mb <= 0.0 {
            return Err(SshError::Command {
                exit_code: -1,
                stderr: "memory total_mb is 0; refusing fake idle metrics".to_string(),
            });
        }

        Ok(MemoryMetrics {
            total_mb,
            used_mb,
            free_mb,
            cached_mb,
            buffers_mb,
            usage_percent: (used_mb / total_mb) * 100.0,
        })
    } else {
        // 备用方法：使用更简单的free命令
        let free_simple_cmd = "free -m";
        let free_simple_result = client.execute(target, free_simple_cmd)?;
        parse_memory_from_free(&free_simple_result.stdout)
    }
}

fn parse_memory_from_free(output: &str) -> Result<MemoryMetrics, SshError> {
    let mut total_mb = 0.0;
    let mut used_mb = 0.0;
    let mut free_mb = 0.0;
    let mut buffers_mb = 0.0;
    let mut cached_mb = 0.0;
    let mut found_mem = false;

    for line in output.lines() {
        if line.starts_with("Mem:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let parse_f = |idx: usize, label: &str| -> Result<f64, SshError> {
                    parts[idx].parse().map_err(|_| SshError::Command {
                        exit_code: -1,
                        stderr: format!("failed to parse memory {label} from free Mem: line"),
                    })
                };
                total_mb = parse_f(1, "total")?;
                used_mb = parse_f(2, "used")?;
                free_mb = parse_f(3, "free")?;
                if parts.len() >= 6 {
                    buffers_mb = parse_f(5, "buffers")?;
                }
                if parts.len() >= 7 {
                    cached_mb = parse_f(6, "cached")?;
                }
                found_mem = true;
            }
        }
    }

    if !found_mem {
        return Err(SshError::Command {
            exit_code: -1,
            stderr: "free output missing Mem: line; refusing fake zero memory metrics".to_string(),
        });
    }
    if total_mb <= 0.0 {
        return Err(SshError::Command {
            exit_code: -1,
            stderr: "memory total_mb is 0; refusing fake idle metrics".to_string(),
        });
    }

    Ok(MemoryMetrics {
        total_mb,
        used_mb,
        free_mb,
        cached_mb,
        buffers_mb,
        usage_percent: (used_mb / total_mb) * 100.0,
    })
}

fn collect_disk_metrics<C: SshClient>(
    client: &C,
    target: &SshTarget,
) -> Result<Vec<DiskMetrics>, SshError> {
    // df -hT：含真实文件系统类型，禁止写死 ext4
    let df_cmd =
        "df -hT | grep -vE '^Filesystem|tmpfs|cdrom|devtmpfs' | awk '{print $1, $2, $3, $4, $5, $6, $7}'";
    let df_result = client.execute(target, df_cmd)?;

    let mut disks = Vec::new();
    let mut parse_errors = 0usize;
    for line in df_result.stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 7 {
            let device = parts[0].to_string();
            let filesystem = parts[1].to_string();
            let total_str = parts[2];
            let used_str = parts[3];
            let available_str = parts[4];
            let usage_percent_str = parts[5].trim_end_matches('%');
            let mount_point = parts[6].to_string();

            let usage_percent = match usage_percent_str.parse::<f64>() {
                Ok(v) => v,
                Err(_) => {
                    parse_errors += 1;
                    continue;
                }
            };

            disks.push(DiskMetrics {
                device,
                mount_point,
                total_gb: parse_size_to_gb(total_str),
                used_gb: parse_size_to_gb(used_str),
                available_gb: parse_size_to_gb(available_str),
                usage_percent,
                filesystem,
            });
        }
    }

    if disks.is_empty() && !df_result.stdout.trim().is_empty() && parse_errors > 0 {
        return Err(SshError::Command {
            exit_code: -1,
            stderr: "failed to parse any disk usage percent from df -hT".to_string(),
        });
    }

    Ok(disks)
}

fn parse_size_to_gb(size_str: &str) -> f64 {
    let size_str = size_str.trim();
    if size_str.is_empty() {
        return 0.0;
    }

    let (number_str, unit) = if size_str.ends_with('T') || size_str.ends_with('t') {
        (&size_str[..size_str.len() - 1], 'T')
    } else if size_str.ends_with('G') || size_str.ends_with('g') {
        (&size_str[..size_str.len() - 1], 'G')
    } else if size_str.ends_with('M') || size_str.ends_with('m') {
        (&size_str[..size_str.len() - 1], 'M')
    } else if size_str.ends_with('K') || size_str.ends_with('k') {
        (&size_str[..size_str.len() - 1], 'K')
    } else {
        (size_str, 'B')
    };

    let number = number_str.parse::<f64>().unwrap_or(0.0);

    match unit {
        'T' => number * 1024.0,
        'G' => number,
        'M' => number / 1024.0,
        'K' => number / (1024.0 * 1024.0),
        _ => number / (1024.0 * 1024.0 * 1024.0),
    }
}

fn collect_network_metrics<C: SshClient>(
    client: &C,
    target: &SshTarget,
) -> Result<NetworkMetrics, SshError> {
    // 使用cat /proc/net/dev获取网络统计
    let net_cmd =
        "cat /proc/net/dev | grep -v 'lo:' | awk 'NR>2 {print $1, $2, $10, $3, $11, $4, $12}'";
    let net_result = client.execute(target, net_cmd)?;

    let mut interfaces = Vec::new();
    for line in net_result.stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 7 {
            let name = parts[0].trim_end_matches(':').to_string();
            let rx_bytes = parts[1].parse().unwrap_or(0);
            let rx_packets = parts[2].parse().unwrap_or(0);
            let rx_errors = parts[3].parse().unwrap_or(0);
            let tx_bytes = parts[4].parse().unwrap_or(0);
            let tx_packets = parts[5].parse().unwrap_or(0);
            let tx_errors = parts[6].parse().unwrap_or(0);

            interfaces.push(NetworkInterface {
                name,
                rx_bytes,
                tx_bytes,
                rx_packets,
                tx_packets,
                rx_errors,
                tx_errors,
            });
        }
    }

    Ok(NetworkMetrics { interfaces })
}

/// 网络错误率（百分比）：(rx_errors + tx_errors) / (rx_packets + tx_packets) * 100
pub fn compute_network_error_rate(network: &NetworkMetrics) -> Option<f64> {
    let total_packets: u64 = network
        .interfaces
        .iter()
        .map(|iface| iface.rx_packets + iface.tx_packets)
        .sum();
    let total_errors: u64 = network
        .interfaces
        .iter()
        .map(|iface| iface.rx_errors + iface.tx_errors)
        .sum();
    if total_packets == 0 {
        None
    } else {
        Some((total_errors as f64 / total_packets as f64) * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_to_gb() {
        assert_eq!(parse_size_to_gb("100G"), 100.0);
        assert_eq!(parse_size_to_gb("1T"), 1024.0);
        assert_eq!(parse_size_to_gb("512M"), 512.0 / 1024.0);
    }

    #[test]
    fn test_compute_network_error_rate() {
        assert_eq!(
            compute_network_error_rate(&NetworkMetrics { interfaces: vec![] }),
            None
        );
        let metrics = NetworkMetrics {
            interfaces: vec![NetworkInterface {
                name: "eth0".into(),
                rx_bytes: 0,
                tx_bytes: 0,
                rx_packets: 90,
                tx_packets: 10,
                rx_errors: 4,
                tx_errors: 1,
            }],
        };
        let rate = compute_network_error_rate(&metrics).unwrap();
        assert!((rate - 5.0).abs() < 1e-9);
    }
}
