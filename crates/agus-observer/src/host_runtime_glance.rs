//! 终端工具栏宿主机快照。一次 SSH，间隔约 1 秒读两次 `/proc`。
//! 运行时间、负载、内存、交换来自单次读取；CPU、进程 CPU、网卡速率用两次计数的差。
use agus_ssh::{SshClient, SshError, SshTarget};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostRuntimeGlance {
    pub uptime_seconds: u64,
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
    /// `(总 jiffies - idle) / 总 jiffies × 100`，idle 是 `/proc/stat` 的第 4 列。
    pub cpu_percent: f64,
    pub cpu_jiffies: Vec<u64>,
    pub mem_total_bytes: u64,
    pub mem_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub processes: Vec<GlanceProcess>,
    pub network: Vec<GlanceNet>,
    pub collected_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlanceProcess {
    pub rss_bytes: u64,
    pub cpu_percent: f64,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlanceNet {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bps: f64,
    pub tx_bps: f64,
}

const GLANCE_SCRIPT: &str = r#"
set +e
sample_procs() {
  find /proc -mindepth 1 -maxdepth 1 -type d -name '[0-9]*' -print 2>/dev/null | awk -F/ '
  {
    pid = $NF
    stat = "/proc/" pid "/stat"
    status = "/proc/" pid "/status"
    if ((getline line < stat) <= 0) { close(stat); next }
    close(stat)
    i = index(line, ") ")
    if (i == 0) next
    rest = substr(line, i + 2)
    n = split(rest, p, " ")
    if (n < 13) next
    ut = p[12] + 0
    st = p[13] + 0
    rss = 0
    while ((getline sl < status) > 0) {
      if (sl ~ /^VmRSS:/) {
        split(sl, a, " ")
        rss = a[2] + 0
        break
      }
    }
    close(status)
    cmd = "/proc/" pid "/cmdline"
    comm = ""
    while ((getline cl < cmd) > 0) {
      gsub(/\0/, " ", cl)
      comm = comm cl
    }
    close(cmd)
    gsub(/[ \t]+$/, "", comm)
    if (comm == "") {
      cpath = "/proc/" pid "/comm"
      if ((getline comm < cpath) <= 0) comm = ""
      close(cpath)
      gsub(/[\n\r]/, "", comm)
    }
    gsub(/\t/, " ", comm)
    printf "%s\t%d\t%d\t%d\t%s\n", pid, rss, ut, st, comm
  }
  '
}
echo '###UPTIME###'
awk '{print int($1)}' /proc/uptime 2>/dev/null
echo '###LOAD###'
awk '{print $1,$2,$3}' /proc/loadavg 2>/dev/null
echo '###NCPU###'
awk '/^cpu[0-9]/{c++} END{print c+0}' /proc/stat 2>/dev/null
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
  if (a == "") a = f + b + c
  used = t - a; if (used < 0) used = 0
  su = st - sf; if (su < 0) su = 0
  print "mem_total_kb=" t
  print "mem_used_kb=" used
  print "swap_total_kb=" st
  print "swap_used_kb=" su
}
' /proc/meminfo 2>/dev/null
echo '###T1###'
awk '{print $1}' /proc/uptime 2>/dev/null
echo '###CPU1###'
awk '/^cpu /{print $2,$3,$4,$5,$6,$7,$8,$9; exit}' /proc/stat 2>/dev/null
echo '###NET1###'
awk 'NR>2 {gsub(/:/,"",$1); if ($1 != "lo") print $1,$2,$10}' /proc/net/dev 2>/dev/null
echo '###PROC1###'
sample_procs
sleep 1
echo '###T2###'
awk '{print $1}' /proc/uptime 2>/dev/null
echo '###CPU2###'
awk '/^cpu /{print $2,$3,$4,$5,$6,$7,$8,$9; exit}' /proc/stat 2>/dev/null
echo '###NET2###'
awk 'NR>2 {gsub(/:/,"",$1); if ($1 != "lo") print $1,$2,$10}' /proc/net/dev 2>/dev/null
echo '###PROC2###'
sample_procs
exit 0
"#;

pub fn collect_host_runtime_glance<C: SshClient>(
    client: &C,
    target: &SshTarget,
) -> Result<HostRuntimeGlance, SshError> {
    let result = client.execute(target, GLANCE_SCRIPT)?;
    parse_host_runtime_glance(&result.stdout).map_err(|message| SshError::Command {
        exit_code: result.exit_code,
        stderr: message,
    })
}

struct ProcSample {
    rss_kb: u64,
    utime: u64,
    stime: u64,
    command: String,
}

pub fn parse_host_runtime_glance(raw: &str) -> Result<HostRuntimeGlance, String> {
    let mut section = "";
    let mut uptime_seconds = 0u64;
    let mut load_1 = 0.0;
    let mut load_5 = 0.0;
    let mut load_15 = 0.0;
    let mut saw_load = false;
    let mut ncpu = 1u32;
    let mut mem_total_bytes = 0u64;
    let mut mem_used_bytes = 0u64;
    let mut swap_total_bytes = 0u64;
    let mut swap_used_bytes = 0u64;
    let mut t1 = 0.0;
    let mut t2 = 0.0;
    let mut saw_t1 = false;
    let mut saw_t2 = false;
    let mut cpu1 = Vec::new();
    let mut cpu2 = Vec::new();
    let mut proc1: HashMap<u32, ProcSample> = HashMap::new();
    let mut proc2: HashMap<u32, ProcSample> = HashMap::new();
    let mut net1: HashMap<String, (u64, u64)> = HashMap::new();
    let mut net2: HashMap<String, (u64, u64)> = HashMap::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("###") && line.ends_with("###") {
            section = match line {
                "###UPTIME###" => "uptime",
                "###LOAD###" => "load",
                "###NCPU###" => "ncpu",
                "###MEM###" => "mem",
                "###T1###" => "t1",
                "###T2###" => "t2",
                "###CPU1###" => "cpu1",
                "###CPU2###" => "cpu2",
                "###PROC1###" => "proc1",
                "###PROC2###" => "proc2",
                "###NET1###" => "net1",
                "###NET2###" => "net2",
                _ => "",
            };
            continue;
        }
        match section {
            "uptime" => {
                if uptime_seconds == 0 {
                    uptime_seconds = line.parse().unwrap_or(0);
                }
            }
            "load" => {
                if saw_load {
                    continue;
                }
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    load_1 = parts[0].parse().unwrap_or(0.0);
                    load_5 = parts[1].parse().unwrap_or(0.0);
                    load_15 = parts[2].parse().unwrap_or(0.0);
                    saw_load = true;
                }
            }
            "ncpu" => {
                if let Ok(n) = line.parse::<u32>() {
                    if n > 0 {
                        ncpu = n;
                    }
                }
            }
            "mem" => {
                if let Some((k, v)) = line.split_once('=') {
                    let n: u64 = v.parse().unwrap_or(0);
                    let bytes = n.saturating_mul(1024);
                    match k {
                        "mem_total_kb" => mem_total_bytes = bytes,
                        "mem_used_kb" => mem_used_bytes = bytes,
                        "swap_total_kb" => swap_total_bytes = bytes,
                        "swap_used_kb" => swap_used_bytes = bytes,
                        _ => {}
                    }
                }
            }
            "t1" => {
                if !saw_t1 {
                    t1 = line.parse().unwrap_or(0.0);
                    saw_t1 = t1 > 0.0;
                }
            }
            "t2" => {
                if !saw_t2 {
                    t2 = line.parse().unwrap_or(0.0);
                    saw_t2 = t2 > 0.0;
                }
            }
            "cpu1" => {
                if cpu1.is_empty() {
                    cpu1 = parse_u64s(line);
                }
            }
            "cpu2" => {
                if cpu2.is_empty() {
                    cpu2 = parse_u64s(line);
                }
            }
            "proc1" => {
                if let Some((pid, sample)) = parse_proc_line(line) {
                    proc1.insert(pid, sample);
                }
            }
            "proc2" => {
                if let Some((pid, sample)) = parse_proc_line(line) {
                    proc2.insert(pid, sample);
                }
            }
            "net1" => {
                if let Some((name, rx, tx)) = parse_net_line(line) {
                    net1.insert(name, (rx, tx));
                }
            }
            "net2" => {
                if let Some((name, rx, tx)) = parse_net_line(line) {
                    net2.insert(name, (rx, tx));
                }
            }
            _ => {}
        }
    }

    if !saw_load || cpu1.len() < 4 || cpu2.len() < 4 || mem_total_bytes == 0 {
        return Err("宿主机快照缺少负载、两次 CPU 采样或内存".to_string());
    }

    let total_delta = jiffy_total(&cpu1, &cpu2);
    let idle_delta = cpu2
        .get(3)
        .copied()
        .unwrap_or(0)
        .saturating_sub(cpu1.get(3).copied().unwrap_or(0));
    let cpu_percent = if total_delta == 0 {
        0.0
    } else {
        (total_delta - idle_delta) as f64 / total_delta as f64 * 100.0
    };

    let dt = if saw_t1 && saw_t2 && t2 > t1 {
        t2 - t1
    } else {
        1.0
    };

    let mut processes: Vec<GlanceProcess> = proc2
        .iter()
        .map(|(pid, now)| {
            let prev_ticks = proc1
                .get(pid)
                .map(|prev| prev.utime.saturating_add(prev.stime))
                .unwrap_or(0);
            let now_ticks = now.utime.saturating_add(now.stime);
            let delta = now_ticks.saturating_sub(prev_ticks);
            let cpu = if total_delta == 0 {
                0.0
            } else {
                delta as f64 / total_delta as f64 * 100.0 * f64::from(ncpu)
            };
            GlanceProcess {
                rss_bytes: now.rss_kb.saturating_mul(1024),
                cpu_percent: cpu,
                command: now.command.clone(),
            }
        })
        .collect();
    processes.sort_by(|a, b| b.rss_bytes.cmp(&a.rss_bytes));
    processes.truncate(8);

    let mut network = Vec::new();
    for (name, (rx2, tx2)) in &net2 {
        let (rx1, tx1) = net1.get(name).copied().unwrap_or((0, 0));
        network.push(GlanceNet {
            name: name.clone(),
            rx_bytes: *rx2,
            tx_bytes: *tx2,
            rx_bps: rx2.saturating_sub(rx1) as f64 / dt,
            tx_bps: tx2.saturating_sub(tx1) as f64 / dt,
        });
    }
    network.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(HostRuntimeGlance {
        uptime_seconds,
        load_1,
        load_5,
        load_15,
        cpu_percent,
        cpu_jiffies: cpu2,
        mem_total_bytes,
        mem_used_bytes,
        swap_total_bytes,
        swap_used_bytes,
        processes,
        network,
        collected_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

fn parse_u64s(line: &str) -> Vec<u64> {
    line.split_whitespace()
        .filter_map(|p| p.parse::<u64>().ok())
        .collect()
}

fn jiffy_total(a: &[u64], b: &[u64]) -> u64 {
    let n = a.len().min(b.len());
    (0..n)
        .map(|i| b[i].saturating_sub(a[i]))
        .sum()
}

fn parse_proc_line(line: &str) -> Option<(u32, ProcSample)> {
    let mut parts = line.splitn(5, '\t');
    let pid: u32 = parts.next()?.parse().ok()?;
    let rss_kb: u64 = parts.next()?.parse().ok()?;
    let utime: u64 = parts.next()?.parse().ok()?;
    let stime: u64 = parts.next()?.parse().ok()?;
    let command = parts.next()?.trim();
    if command.is_empty() {
        return None;
    }
    Some((
        pid,
        ProcSample {
            rss_kb,
            utime,
            stime,
            command: command.to_string(),
        },
    ))
}

fn parse_net_line(line: &str) -> Option<(String, u64, u64)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    let name = parts[0].trim_end_matches(':');
    if name.is_empty() || name == "lo" {
        return None;
    }
    Some((
        name.to_string(),
        parts[1].parse().unwrap_or(0),
        parts[2].parse().unwrap_or(0),
    ))
}

#[cfg(test)]
mod tests {
    use super::parse_host_runtime_glance;

    #[test]
    fn parses_proc_deltas_sorted_by_rss() {
        let raw = r#"
###UPTIME###
38880000
###LOAD###
0.16 0.36 0.27
###NCPU###
2
###MEM###
mem_total_kb=1998848
mem_used_kb=712704
swap_total_kb=2097152
swap_used_kb=115712
###T1###
1000.0
###CPU1###
100 0 50 800 10 0 0 0
###NET1###
eth0 1000 200
lo 9 9
###PROC1###
1	1000	10	0	lowmem
2	50000	100	0	AliYunDunMonitor
###T2###
1001.0
###CPU2###
110 0 60 880 10 0 0 0
###NET2###
eth0 1500 250
lo 10 10
###PROC2###
1	1000	10	1	lowmem
2	51000	100	4	AliYunDunMonitor
3	200	5	0	rcu_sched
"#;
        let glance = parse_host_runtime_glance(raw).expect("glance");
        assert_eq!(glance.uptime_seconds, 38_880_000);
        assert!((glance.load_1 - 0.16).abs() < 0.001);
        assert!((glance.load_5 - 0.36).abs() < 0.001);
        // delta jiffies 10+0+10+80 = 100, idle 80 → 20%
        assert!((glance.cpu_percent - 20.0).abs() < 0.01);
        assert_eq!(glance.mem_used_bytes, 712_704 * 1024);
        assert_eq!(glance.swap_used_bytes, 115_712 * 1024);
        assert_eq!(glance.processes[0].command, "AliYunDunMonitor");
        assert_eq!(glance.processes[0].rss_bytes, 51_000 * 1024);
        // 4 ticks / 100 total * 100 * 2 cores = 8%
        assert!((glance.processes[0].cpu_percent - 8.0).abs() < 0.01);
        assert!(glance.processes[0].rss_bytes >= glance.processes[1].rss_bytes);
        assert_eq!(glance.network.len(), 1);
        assert_eq!(glance.network[0].name, "eth0");
        assert!((glance.network[0].rx_bps - 500.0).abs() < 0.01);
        assert!((glance.network[0].tx_bps - 50.0).abs() < 0.01);
    }
}
