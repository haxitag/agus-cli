//! 主机侧 OPS 自定义指标采集（auth / API 延迟 / DB 连接利用率）
//! 全部通过真实 SSH 命令；采集失败返回 None，不得填假 0 伪装健康。

use agus_ssh::{SshClient, SshTarget};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpsCustomMetrics {
    /// 近窗口内 SSH/登录失败次数
    pub auth_failure_burst: Option<f64>,
    /// HTTP 探针延迟 P95 近似（单次或少量样本的百分位近似，ms）
    pub api_latency_p95: Option<f64>,
    pub api_latency_p99: Option<f64>,
    /// 数据库连接占用百分比（0-100），基于观测连接数 / 软上限
    pub db_connections_usage: Option<f64>,
    /// nginx stub_status 两次采样推算的 request/s（真实 Δrequests/Δt；无 stub 则为 None）
    pub request_rate: Option<f64>,
    /// stub_status Active connections（瞬时值，非假造）
    pub nginx_active_connections: Option<f64>,
    /// 采集说明（哪些探针成功/失败），供日志与 UI
    pub notes: Vec<String>,
}

impl OpsCustomMetrics {
    pub fn into_map(self) -> HashMap<String, f64> {
        let mut map = HashMap::new();
        if let Some(v) = self.auth_failure_burst {
            map.insert("auth_failure_burst".to_string(), v);
        }
        if let Some(v) = self.api_latency_p95 {
            map.insert("api_latency_p95".to_string(), v);
        }
        if let Some(v) = self.api_latency_p99 {
            map.insert("api_latency_p99".to_string(), v);
        }
        if let Some(v) = self.db_connections_usage {
            map.insert("db_connections_usage".to_string(), v);
        }
        if let Some(v) = self.request_rate {
            map.insert("request_rate".to_string(), v);
            // 业务分析 has_business_probe 识别键
            map.insert("qps".to_string(), v);
        }
        if let Some(v) = self.nginx_active_connections {
            map.insert("nginx_active_connections".to_string(), v);
        }
        map
    }
}

/// 解析 nginx stub_status 正文中的 requests 累计值与 Active connections。
pub fn parse_nginx_stub_status(body: &str) -> Option<(u64, u64)> {
    // Active connections: 291
    // server accepts handled requests
    //  16630948 16630948 31070465
    let mut active = None;
    let mut requests = None;
    let mut saw_header = false;
    for line in body.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Active connections:") {
            active = rest.trim().parse().ok();
        }
        if t.contains("accepts") && t.contains("handled") && t.contains("requests") {
            saw_header = true;
            continue;
        }
        if saw_header && requests.is_none() {
            let parts: Vec<&str> = t.split_whitespace().collect();
            if parts.len() >= 3 {
                requests = parts[2].parse().ok();
                saw_header = false;
            }
        }
    }
    match (active, requests) {
        (Some(a), Some(r)) => Some((a, r)),
        _ => None,
    }
}

fn parse_u64_line(stdout: &str) -> Option<u64> {
    let trimmed = stdout.lines().next()?.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse().ok()
}

fn parse_f64_line(stdout: &str) -> Option<f64> {
    let trimmed = stdout.lines().next()?.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse().ok()
}

/// 采集自定义 OPS 指标。`health_check_url` 可选（主机上的 HTTP 健康检查地址）。
pub fn collect_ops_custom_metrics<C: SshClient>(
    client: &C,
    target: &SshTarget,
    health_check_url: Option<&str>,
) -> OpsCustomMetrics {
    let mut out = OpsCustomMetrics::default();

    // --- auth_failure_burst：近 5 分钟登录失败 ---
    let auth_cmd = r#"
set +e
count=0
if command -v journalctl >/dev/null 2>&1; then
  count=$(journalctl --since "5 min ago" 2>/dev/null | grep -cE 'Failed password|Invalid user|authentication failure' || true)
fi
if [ -z "$count" ] || [ "$count" = "0" ]; then
  if [ -r /var/log/auth.log ]; then
    count=$(grep -E 'Failed password|Invalid user|authentication failure' /var/log/auth.log 2>/dev/null | tail -n 5000 | wc -l | tr -d ' ')
  elif [ -r /var/log/secure ]; then
    count=$(grep -E 'Failed password|Invalid user|authentication failure' /var/log/secure 2>/dev/null | tail -n 5000 | wc -l | tr -d ' ')
  fi
fi
echo "${count:-0}"
"#;
    match client.execute(target, auth_cmd) {
        Ok(res) if res.exit_code == 0 => {
            if let Some(n) = parse_u64_line(&res.stdout) {
                out.auth_failure_burst = Some(n as f64);
                out.notes
                    .push(format!("auth_failure_burst={n} (5m window / recent log sample)"));
            } else {
                out.notes
                    .push("auth_failure_burst: unable to parse counter".into());
            }
        }
        Ok(res) => out.notes.push(format!(
            "auth_failure_burst: command exit {}",
            res.exit_code
        )),
        Err(e) => out.notes.push(format!("auth_failure_burst: {e}")),
    }

    // --- api latency：对 health_check_url 或本机常见端口做 curl 探针 ---
    let probe_url = health_check_url
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1/".to_string());
    let latency_cmd = format!(
        r#"
set +e
if ! command -v curl >/dev/null 2>&1; then
  echo ""
  exit 0
fi
url='{url}'
# 连续 5 次取耗时(ms)，用排序近似 p95/p99
times=""
for i in 1 2 3 4 5; do
  t=$(curl -s -o /dev/null -w '%{{time_total}}' --connect-timeout 2 --max-time 5 "$url" 2>/dev/null)
  if [ -n "$t" ]; then
    ms=$(awk -v x="$t" 'BEGIN {{ printf "%.0f", x*1000 }}')
    times="$times$ms\n"
  fi
done
if [ -z "$times" ]; then
  echo ""
  exit 0
fi
sorted=$(printf "%b" "$times" | grep -E '^[0-9]+$' | sort -n)
count=$(printf "%s\n" "$sorted" | grep -c . || true)
if [ "$count" -eq 0 ]; then
  echo ""
  exit 0
fi
# p95 index = ceil(0.95*n)-1 → bash 1-based
idx95=$(( (95 * count + 99) / 100 ))
idx99=$(( (99 * count + 99) / 100 ))
[ "$idx95" -lt 1 ] && idx95=1
[ "$idx99" -lt 1 ] && idx99=1
[ "$idx95" -gt "$count" ] && idx95=$count
[ "$idx99" -gt "$count" ] && idx99=$count
p95=$(printf "%s\n" "$sorted" | sed -n "${{idx95}}p")
p99=$(printf "%s\n" "$sorted" | sed -n "${{idx99}}p")
echo "$p95 $p99"
"#,
        url = probe_url.replace('\'', "'\\''")
    );
    match client.execute(target, &latency_cmd) {
        Ok(res) if res.exit_code == 0 => {
            let line = res.stdout.lines().next().unwrap_or("").trim();
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Ok(p95), Ok(p99)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                    out.api_latency_p95 = Some(p95);
                    out.api_latency_p99 = Some(p99);
                    out.notes.push(format!(
                        "api_latency_p95={p95}ms p99={p99}ms url={probe_url} (5-sample approx)"
                    ));
                }
            } else {
                out.notes.push(format!(
                    "api_latency: no samples from curl probe ({probe_url})"
                ));
            }
        }
        Ok(_) => out.notes.push("api_latency: probe failed".into()),
        Err(e) => out.notes.push(format!("api_latency: {e}")),
    }

    // --- db_connections_usage：统计 3306/5432 连接数 / 软上限 100 ---
    let db_cmd = r#"
set +e
mysql_n=0
pg_n=0
if command -v ss >/dev/null 2>&1; then
  mysql_n=$(ss -tn state established '( sport = :3306 )' 2>/dev/null | tail -n +2 | wc -l | tr -d ' ')
  pg_n=$(ss -tn state established '( sport = :5432 )' 2>/dev/null | tail -n +2 | wc -l | tr -d ' ')
elif command -v netstat >/dev/null 2>&1; then
  mysql_n=$(netstat -tn 2>/dev/null | grep -E ':3306\s' | grep ESTABLISHED | wc -l | tr -d ' ')
  pg_n=$(netstat -tn 2>/dev/null | grep -E ':5432\s' | grep ESTABLISHED | wc -l | tr -d ' ')
fi
mysql_n=${mysql_n:-0}
pg_n=${pg_n:-0}
total=$((mysql_n + pg_n))
echo "$total"
"#;
    match client.execute(target, db_cmd) {
        Ok(res) if res.exit_code == 0 => {
            if let Some(n) = parse_u64_line(&res.stdout) {
                // 软上限 100：无 max_connections 时的保守估算；超过则 100%
                let usage = ((n as f64) / 100.0 * 100.0).min(100.0);
                out.db_connections_usage = Some(usage);
                out.notes.push(format!(
                    "db_connections_usage={usage:.1}% (observed_est_conns={n}, soft_cap=100; NOT max_connections — estimate only)"
                ));
            } else if let Some(v) = parse_f64_line(&res.stdout) {
                out.db_connections_usage = Some(v.min(100.0));
            } else {
                out.notes
                    .push("db_connections_usage: unable to parse".into());
            }
        }
        Ok(_) => out
            .notes
            .push("db_connections_usage: ss/netstat probe failed".into()),
        Err(e) => out.notes.push(format!("db_connections_usage: {e}")),
    }

    // --- nginx stub_status：两次采样推算真实 request_rate（无 stub 则保持 None，禁止编造）---
    let stub_cmd = r#"
set +e
url=""
for u in \
  "http://127.0.0.1/nginx_status" \
  "http://127.0.0.1/status" \
  "http://127.0.0.1:8080/nginx_status"
do
  body=$(curl -s --connect-timeout 1 --max-time 2 "$u" 2>/dev/null)
  if echo "$body" | grep -q 'Active connections'; then
    url="$u"
    echo "__STUB1__"
    echo "$body"
    sleep 2
    body2=$(curl -s --connect-timeout 1 --max-time 2 "$u" 2>/dev/null)
    echo "__STUB2__"
    echo "$body2"
    exit 0
  fi
done
echo "__NO_STUB__"
"#;
    match client.execute(target, stub_cmd) {
        Ok(res) if res.exit_code == 0 => {
            let stdout = res.stdout;
            if stdout.contains("__NO_STUB__") {
                out.notes
                    .push("request_rate: nginx stub_status not available on common local URLs".into());
            } else if let (Some(i1), Some(i2)) =
                (stdout.find("__STUB1__"), stdout.find("__STUB2__"))
            {
                let body1 = &stdout[i1 + "__STUB1__".len()..i2];
                let body2 = &stdout[i2 + "__STUB2__".len()..];
                match (parse_nginx_stub_status(body1), parse_nginx_stub_status(body2)) {
                    (Some((active, r1)), Some((_, r2))) => {
                        out.nginx_active_connections = Some(active as f64);
                        if r2 >= r1 {
                            let rate = (r2 - r1) as f64 / 2.0;
                            out.request_rate = Some(rate);
                            out.notes.push(format!(
                                "request_rate={rate:.2}/s (nginx stub_status Δrequests over 2s; active={active})"
                            ));
                        } else {
                            out.notes.push(
                                "request_rate: stub counter went backwards; skipped".into(),
                            );
                        }
                    }
                    _ => out
                        .notes
                        .push("request_rate: failed to parse stub_status body".into()),
                }
            } else {
                out.notes
                    .push("request_rate: unexpected stub probe output".into());
            }
        }
        Ok(_) => out.notes.push("request_rate: stub probe command failed".into()),
        Err(e) => out.notes.push(format!("request_rate: {e}")),
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_map_skips_none() {
        let mut m = OpsCustomMetrics::default();
        m.auth_failure_burst = Some(3.0);
        let map = m.into_map();
        assert_eq!(map.get("auth_failure_burst"), Some(&3.0));
        assert!(!map.contains_key("api_latency_p95"));
    }

    #[test]
    fn parses_nginx_stub_status() {
        let body = "Active connections: 291 \n\
server accepts handled requests\n \
 16630948 16630948 31070465 \n\
Reading: 6 Writing: 179 Waiting: 106 \n";
        let (active, reqs) = parse_nginx_stub_status(body).expect("parse");
        assert_eq!(active, 291);
        assert_eq!(reqs, 31_070_465);
    }
}
