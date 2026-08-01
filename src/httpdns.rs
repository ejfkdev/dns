//! HTTPDNS 智能客户端：兼容标准 DoH JSON (RFC 8444) 与厂商私有 HTTPDNS 格式。
//!
//! 当 `--server` 写成 `http://` 或 `https://` URL 时使用此模块。
//! 查询时依次尝试多种格式，自动适配：
//! 1. DoH JSON：`?name=DOMAIN&type=TYPE` + `accept: application/dns-json`
//!    （Cloudflare、Google、腾讯 doh.pub、阿里 dns.alidns.com 等均支持）
//! 2. 腾讯私有 HTTPDNS：`?dn=DOMAIN&type=TYPE` → 纯文本 IP;IP 分号分隔
//!    （119.29.29.29 等）

use std::time::{Duration, Instant};

use hickory_proto::rr::RecordType;

use crate::resolver::RecordEntry;

/// HTTPDNS 查询结果。
pub struct HttpDnsResult {
    pub records: Vec<RecordEntry>,
    pub error: Option<String>,
    #[allow(dead_code)]
    pub elapsed_ms: u128,
}

/// 向 HTTPDNS 服务器查询。
///
/// `base_url` 是 `--server` 给的完整 URL（含路径），如
/// `https://1.1.1.1/dns-query`、`http://119.29.29.29/d`。
pub async fn query(
    base_url: &str,
    domain: &str,
    record_type: RecordType,
    timeout_secs: u64,
) -> HttpDnsResult {
    let start = Instant::now();
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HttpDnsResult {
                records: vec![],
                error: Some(format!("HTTP 客户端构造失败: {e}")),
                elapsed_ms: start.elapsed().as_millis(),
            };
        }
    };

    let rt_name = record_type.to_string();

    // ── 策略 1：DoH JSON 格式（RFC 8444 Google JSON）──
    match try_doh_json(&client, base_url, domain, &rt_name).await {
        Ok(recs) => {
            return HttpDnsResult {
                records: recs,
                error: None,
                elapsed_ms: start.elapsed().as_millis(),
            };
        }
        Err(e) => {
            // DoH JSON 失败，继续尝试私有格式
            let _ = e; // 记录但不中断
        }
    }

    // ── 策略 2：腾讯私有 HTTPDNS 格式 ──
    match try_tencent_httpdns(&client, base_url, domain, &rt_name).await {
        Ok(recs) => HttpDnsResult {
            records: recs,
            error: None,
            elapsed_ms: start.elapsed().as_millis(),
        },
        Err(e) => HttpDnsResult {
            records: vec![],
            error: Some(format!("所有 HTTPDNS 格式均失败: {e}")),
            elapsed_ms: start.elapsed().as_millis(),
        },
    }
}

/// DoH JSON 格式查询（RFC 8444）。
///
/// 请求：`GET {base}?name={domain}&type={type}` + `accept: application/dns-json`
/// 响应：`{"Status":0,"Answer":[{"name":"...","type":1,"TTL":272,"data":"1.2.3.4"}]}`
async fn try_doh_json(
    client: &reqwest::Client,
    base_url: &str,
    domain: &str,
    rt_name: &str,
) -> Result<Vec<RecordEntry>, String> {
    let url = build_url(base_url, &[("name", domain), ("type", rt_name)]);
    let resp = client
        .get(&url)
        .header("accept", "application/dns-json")
        .send()
        .await
        .map_err(|e| format!("DoH JSON 请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("DoH JSON HTTP {}", resp.status()));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("DoH JSON 解析失败: {e}"))?;

    // Status=0 表示成功
    let status = body.get("Status").and_then(|v| v.as_u64()).unwrap_or(99);
    if status != 0 {
        // 可能不是 DoH JSON 格式（返回了别的 JSON）
        return Err(format!("DoH Status={status}（非标准 DoH JSON 响应）"));
    }

    let mut records = Vec::new();
    if let Some(answers) = body.get("Answer").and_then(|a| a.as_array()) {
        for ans in answers {
            let data = ans
                .get("data")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let ttl = ans.get("TTL").and_then(|t| t.as_u64()).unwrap_or(0) as u32;
            if !data.is_empty() {
                records.push(RecordEntry { value: data, ttl });
            }
        }
    }

    Ok(records)
}

/// 腾讯私有 HTTPDNS 格式查询。
///
/// 请求：`GET {base}?dn={domain}&type={type}`
/// 响应：纯文本，`IP;IP;IP` 分号分隔（仅 A/AAAA 有效）
async fn try_tencent_httpdns(
    client: &reqwest::Client,
    base_url: &str,
    domain: &str,
    rt_name: &str,
) -> Result<Vec<RecordEntry>, String> {
    // 腾讯私有 HTTPDNS 仅支持 A/AAAA，其他类型直接返回空
    if rt_name != "A" && rt_name != "AAAA" {
        return Err("腾讯私有 HTTPDNS 仅支持 A/AAAA 类型".into());
    }
    let url = build_url(base_url, &[("dn", domain), ("type", rt_name)]);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("腾讯 HTTPDNS 请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("腾讯 HTTPDNS HTTP {}", resp.status()));
    }

    let text = resp
        .text()
        .await
        .map_err(|e| format!("腾讯 HTTPDNS 响应读取失败: {e}"))?;

    // 解析纯文本：IP;IP;IP 分号分隔
    let mut records = Vec::new();
    for part in text.trim().split([';', '\n']) {
        let part = part.trim();
        if !part.is_empty() {
            // 腾讯 HTTPDNS 不返回 TTL，用 0 占位
            records.push(RecordEntry {
                value: part.to_string(),
                ttl: 0,
            });
        }
    }

    Ok(records)
}

/// 拼接 base_url 和 query 参数。
fn build_url(base: &str, params: &[(&str, &str)]) -> String {
    let separator = if base.contains('?') { "&" } else { "?" };
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect();
    format!("{base}{separator}{}", query.join("&"))
}

/// 简单的 URL 编码（仅编码域名中的特殊字符）。
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// 判断一个 `--server` 值是否为 HTTPDNS URL（http:// 或 https:// 开头）。
#[allow(dead_code)]
pub fn is_httpdns_url(s: &str) -> bool {
    let s = s.trim();
    s.starts_with("http://") || s.starts_with("https://")
}
