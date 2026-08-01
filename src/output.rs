//! 输出格式化：TTY 彩色 / 非 TTY 纯文本 / JSON / --short。
//!
//! 多服务器模式下：
//! - 默认：对每类型汇总去重，不标注服务器；TTY 下对有差异的值用 magenta 高亮。
//! - verbose：去重后标注一致数 (N/M)；额外显示各服务器耗时。
//! - JSON：完整多服务器明细，不去重。

use std::collections::HashMap;
use std::io::{IsTerminal, Write};

use colored::Colorize;
use serde::Serialize;

use crate::cli::ColorMode;
use crate::resolver::{QueryResult, TypeResult};

/// 输出选项。
pub struct OutputOpts {
    pub json: bool,
    pub yaml: bool,
    pub csv: bool,
    pub verbose: bool,
    pub ttl: bool,
    pub short: bool,
    pub color: ColorMode,
}

/// 合并多域名结果的入口。
pub fn print_merged(results: &[QueryResult], opts: &OutputOpts) {
    let use_color = match opts.color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => std::io::stdout().is_terminal(),
    };
    colored::control::set_override(use_color);

    let mut out = String::new();
    if opts.json {
        render_merged_json(&mut out, results);
    } else if opts.yaml {
        render_merged_yaml(&mut out, results);
    } else if opts.csv {
        render_merged_csv(&mut out, results);
    } else if opts.short {
        render_merged_short(&mut out, results);
    } else if use_color {
        render_merged_tty(&mut out, results, opts);
    } else {
        render_merged_plain(&mut out, results, opts);
    }
    print!("{out}");
}

/// 主入口：根据选项选择格式并打印。
pub fn print(result: &QueryResult, opts: &OutputOpts) {
    let use_color = match opts.color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => std::io::stdout().is_terminal(),
    };
    colored::control::set_override(use_color);

    let mut out = String::new();
    if opts.json {
        render_json(&mut out, result);
    } else if opts.yaml {
        render_yaml(&mut out, result);
    } else if opts.csv {
        render_csv(&mut out, result);
    } else if opts.short {
        render_short(&mut out, result);
    } else if use_color {
        render_tty(&mut out, result, opts);
    } else {
        render_plain(&mut out, result, opts);
    }
    print!("{out}");
}

// ───────────────────── 汇总辅助 ─────────────────────

/// 去重后的值 + 它由几个服务器返回（用于 verbose 标注 + 差异判断）。
#[derive(Clone)]
struct AggregatedEntry {
    value: String,
    /// 返回该值的服务器数
    count: usize,
    /// 该值出现过的任意 TTL（取首次）
    ttl: u32,
}

/// 对一个类型的所有服务器结果做汇总去重。
fn aggregate(type_result: &TypeResult) -> (Vec<AggregatedEntry>, usize, bool) {
    let total_servers = type_result.server_results.len();
    // value → (出现次数, ttl)
    let mut map: HashMap<String, (usize, u32)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for sr in &type_result.server_results {
        for rec in &sr.records {
            let entry = map.entry(rec.value.clone()).or_insert((0, rec.ttl));
            entry.0 += 1;
            if !order.contains(&rec.value) {
                order.push(rec.value.clone());
            }
        }
    }
    let entries: Vec<AggregatedEntry> = order
        .iter()
        .map(|v| {
            let (count, ttl) = map[v];
            AggregatedEntry {
                value: v.clone(),
                count,
                ttl,
            }
        })
        .collect();

    // 差异判断：是否所有返回了记录的服务器，其值集都相同。
    let mut value_sets: Vec<std::collections::HashSet<String>> = Vec::new();
    for sr in &type_result.server_results {
        if sr.error.is_none() {
            let set: std::collections::HashSet<String> =
                sr.records.iter().map(|r| r.value.clone()).collect();
            value_sets.push(set);
        }
    }
    let has_diff = !value_sets.is_empty() && value_sets.iter().any(|s| *s != value_sets[0]);

    (entries, total_servers, has_diff)
}

/// 是否应该隐藏某类型（默认模式下无记录或全出错）。
fn should_hide(type_result: &TypeResult, hide_empty: bool) -> bool {
    if !hide_empty {
        return false;
    }
    // 如果有服务器返回了非 NoError 的状态码（如 NXDOMAIN/SERVFAIL），不隐藏
    let has_error_status = type_result.server_results.iter().any(|sr| {
        !sr.response_code.is_empty()
            && !sr.response_code.contains("NOERROR")
            && !sr.response_code.contains("NOERROR")
    });
    if has_error_status {
        return false;
    }
    type_result
        .server_results
        .iter()
        .all(|sr| sr.records.is_empty() && sr.error.is_some())
        || type_result
            .server_results
            .iter()
            .all(|sr| sr.records.is_empty())
}

// ───────────────────── --merge（多域名合并） ─────────────────────

/// 合并后的一条记录（值 + 来源域名 + TTL）。
struct MergedEntry {
    value: String,
    domain: String,
    ttl: u32,
}

/// 收集所有域名的同类型记录，按类型分组。
fn collect_merged(results: &[QueryResult]) -> Vec<(String, Vec<MergedEntry>)> {
    use std::collections::BTreeMap;
    let mut by_type: BTreeMap<String, Vec<MergedEntry>> = BTreeMap::new();
    for result in results {
        for tr in &result.results {
            if should_hide(tr, result.hide_empty) {
                continue;
            }
            let entries = aggregate(tr).0;
            if entries.is_empty() {
                continue;
            }
            let type_name = tr.record_type.to_string();
            for e in &entries {
                by_type
                    .entry(type_name.clone())
                    .or_default()
                    .push(MergedEntry {
                        value: e.value.clone(),
                        domain: result.domain.clone(),
                        ttl: e.ttl,
                    });
            }
        }
    }
    by_type.into_iter().collect()
}

fn render_merged_short(out: &mut String, results: &[QueryResult]) {
    let merged = collect_merged(results);
    for (type_name, entries) in &merged {
        for e in entries {
            out.push_str(&format!("{} {}\n", type_name, e.value));
        }
    }
}

fn render_merged_plain(out: &mut String, results: &[QueryResult], opts: &OutputOpts) {
    let merged = collect_merged(results);
    let domains: Vec<&str> = results.iter().map(|r| r.domain.as_str()).collect();
    let _ = domains;
    for (type_name, entries) in &merged {
        for e in entries {
            let val = if opts.ttl {
                format!("{} (ttl={})", e.value, e.ttl)
            } else {
                e.value.clone()
            };
            out.push_str(&format!("{} {}\n", type_name, val));
        }
    }
}

fn render_merged_tty(out: &mut String, results: &[QueryResult], opts: &OutputOpts) {
    let merged = collect_merged(results);
    let mut first = true;
    for (type_name, entries) in &merged {
        if !first {
            out.push('\n');
        }
        first = false;
        out.push_str(&format!("{}\n", type_name.bold().cyan()));
        for e in entries {
            let val = if opts.ttl {
                format!("{} (ttl={})", e.value, e.ttl)
            } else {
                e.value.clone()
            };
            let line = colorize_record(type_name, &val);
            out.push_str(&format!("  {}\n", line));
        }
    }
}

fn render_merged_json(out: &mut String, results: &[QueryResult]) {
    let merged = collect_merged(results);
    out.push('[');
    for (i, (type_name, entries)) in merged.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"type\":\"");
        out.push_str(type_name);
        out.push_str("\",\"records\":[");
        for (j, e) in entries.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"domain\":\"{}\",\"value\":{},\"ttl\":{}}}",
                json_escape(&e.domain),
                serde_json::to_string(&e.value).unwrap_or_else(|_| "\"\"".into()),
                e.ttl
            ));
        }
        out.push_str("]}");
    }
    out.push(']');
}

fn json_escape(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap_or_else(|_| "\"\"".into())
        // to_string 返回带引号的字符串，去掉外层引号
        .trim_matches('"')
        .to_string()
}

fn render_merged_yaml(out: &mut String, results: &[QueryResult]) {
    let merged = collect_merged(results);
    out.push_str("merged:\n");
    for (type_name, entries) in &merged {
        out.push_str(&format!("- type: {type_name}\n"));
        out.push_str("  records:\n");
        for e in entries {
            out.push_str(&format!("    - domain: {}\n", e.domain));
            out.push_str(&format!(
                "      value: \"{}\"\n",
                e.value.replace('"', "\\\"")
            ));
            out.push_str(&format!("      ttl: {}\n", e.ttl));
        }
    }
}

// ───────────────────── YAML ─────────────────────

fn render_yaml(out: &mut String, result: &QueryResult) {
    out.push_str(&format!("domain: {}\n", result.domain));
    out.push_str(&format!("server_desc: {}\n", result.server_desc));
    out.push_str(&format!("elapsed_ms: {}\n", result.elapsed_ms));
    out.push_str("entries:\n");
    for tr in &result.results {
        let type_name = tr.record_type.to_string();
        out.push_str(&format!("- type: {type_name}\n"));
        out.push_str("  servers:\n");
        for sr in &tr.server_results {
            out.push_str(&format!("    - server: {}\n", sr.server_name));
            out.push_str(&format!("      desc: {}\n", sr.server_desc));
            out.push_str(&format!("      response_code: {}\n", sr.response_code));
            out.push_str(&format!("      elapsed_ms: {}\n", sr.elapsed_ms));
            if let Some(err) = &sr.error {
                out.push_str(&format!("      error: {err}\n"));
            }
            if sr.records.is_empty() {
                out.push_str("      records: []\n");
            } else {
                out.push_str("      records:\n");
                for rec in &sr.records {
                    out.push_str(&format!(
                        "        - value: \"{v}\"\n",
                        v = rec.value.replace('"', "\\\"")
                    ));
                    out.push_str(&format!("          ttl: {}\n", rec.ttl));
                }
            }
        }
    }
}

// ───────────────────── CSV ─────────────────────

fn render_csv(out: &mut String, result: &QueryResult) {
    // 表头：domain,type,value,ttl
    out.push_str("domain,type,value,ttl\n");
    for tr in &result.results {
        if should_hide(tr, result.hide_empty) {
            continue;
        }
        let entries = aggregate(tr).0;
        let type_name = tr.record_type.to_string();
        for e in &entries {
            out.push_str(&format!(
                "{},{},{},{}\n",
                csv_escape(&result.domain),
                csv_escape(&type_name),
                csv_escape(&e.value),
                e.ttl
            ));
        }
    }
}

fn render_merged_csv(out: &mut String, results: &[QueryResult]) {
    out.push_str("domain,type,value,ttl\n");
    let merged = collect_merged(results);
    for (type_name, entries) in &merged {
        for e in entries {
            out.push_str(&format!(
                "{},{},{},{}\n",
                csv_escape(&e.domain),
                csv_escape(type_name),
                csv_escape(&e.value),
                e.ttl
            ));
        }
    }
}

/// CSV 字段转义：含逗号/引号/换行时用双引号包裹，内部引号加倍。
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ───────────────────── --short ─────────────────────

fn render_short(out: &mut String, result: &QueryResult) {
    // 多域名时用域名注释分隔
    out.push_str(&format!("# {}\n", result.domain));
    for tr in &result.results {
        if should_hide(tr, result.hide_empty) {
            continue;
        }
        let (entries, _, _) = aggregate(tr);
        for e in &entries {
            out.push_str(&e.value);
            out.push('\n');
        }
    }
}

// ───────────────────── JSON ─────────────────────

#[derive(Serialize)]
struct JsonServerResult<'a> {
    server: &'a str,
    desc: &'a str,
    records: Vec<JsonRecord<'a>>,
    response_code: &'a str,
    error: Option<&'a str>,
    elapsed_ms: u128,
}

#[derive(Serialize)]
struct JsonRecord<'a> {
    value: &'a str,
    ttl: u32,
}

#[derive(Serialize)]
struct JsonEntry<'a> {
    r#type: &'a str,
    servers: Vec<JsonServerResult<'a>>,
}

#[derive(Serialize)]
struct JsonOutput<'a> {
    domain: &'a str,
    server_desc: &'a str,
    elapsed_ms: u128,
    entries: Vec<JsonEntry<'a>>,
}

fn render_json(out: &mut String, result: &QueryResult) {
    let mut entries: Vec<JsonEntry> = Vec::with_capacity(result.results.len());
    for tr in &result.results {
        let type_str: String = tr.record_type.to_string();
        let mut servers: Vec<JsonServerResult> = Vec::new();
        for sr in &tr.server_results {
            let recs: Vec<JsonRecord> = sr
                .records
                .iter()
                .map(|r| JsonRecord {
                    value: &r.value,
                    ttl: r.ttl,
                })
                .collect();
            servers.push(JsonServerResult {
                server: &sr.server_name,
                desc: &sr.server_desc,
                records: recs,
                response_code: &sr.response_code,
                error: sr.error.as_deref(),
                elapsed_ms: sr.elapsed_ms,
            });
        }
        // type_str 需要存活到序列化时；用 Box::leak 让其 'static，或改用 owned。
        // 这里用 leak 简单处理（进程退出即回收）。
        let leaked: &'static str = Box::leak(type_str.into_boxed_str());
        entries.push(JsonEntry {
            r#type: leaked,
            servers,
        });
    }
    let payload = JsonOutput {
        domain: &result.domain,
        server_desc: &result.server_desc,
        elapsed_ms: result.elapsed_ms,
        entries,
    };
    match serde_json::to_string_pretty(&payload) {
        Ok(s) => out.push_str(&s),
        Err(e) => out.push_str(&format!("JSON 序列化失败: {e}")),
    }
}

// ───────────────────── TTY（彩色分组） ─────────────────────

fn render_tty(out: &mut String, result: &QueryResult, opts: &OutputOpts) {
    // verbose 头部
    if opts.verbose {
        let header = format!(
            "{} {}  {} {}  {} {}",
            "查询:".bold(),
            result.domain.cyan(),
            "服务器:".bold(),
            result.server_desc.yellow(),
            "耗时:".bold(),
            format!("{}ms", result.elapsed_ms).green()
        );
        out.push_str(&header);
        out.push('\n');
    }

    let mut first = true;
    let mut total_records = 0usize;
    let mut total_types = 0usize;

    // 如果所有类型都返回 NXDOMAIN，显示一条提示而非逐类型展开
    let all_nxdomain = result.results.iter().all(|tr| {
        tr.server_results
            .iter()
            .any(|sr| !sr.response_code.is_empty() && sr.response_code.contains("NXDOMAIN"))
    });
    if all_nxdomain && result.hide_empty {
        out.push_str(&format!("  {}\n", "域名不存在 (NXDOMAIN)".red()));
        return;
    }

    // 服务器总数：从首个类型的 server_results 取（各类型服务器列表一致）
    let total_servers_count = result
        .results
        .first()
        .map(|tr| tr.server_results.len())
        .unwrap_or(0);

    // 收集不可用服务器信息：(name, desc, error) 用于末尾汇总
    // "完全不可用"= 所有查询类型都出错且无记录
    let mut dead_servers: Vec<(String, String, String)> = Vec::new();
    if opts.verbose
        && total_servers_count > 1
        && let Some(first_tr) = result.results.first()
    {
        for sr in &first_tr.server_results {
            // 判断该服务器是否在所有类型中都出错
            let all_failed = result.results.iter().all(|tr| {
                tr.server_results
                    .iter()
                    .find(|s| s.server_name == sr.server_name && s.server_desc == sr.server_desc)
                    .is_some_and(|s| s.error.is_some() && s.records.is_empty())
            });
            if all_failed && let Some(err) = &sr.error {
                dead_servers.push((sr.server_name.clone(), sr.server_desc.clone(), err.clone()));
            }
        }
    }

    for tr in &result.results {
        if should_hide(tr, result.hide_empty) {
            continue;
        }
        let (entries, total_servers, has_diff) = aggregate(tr);
        if entries.is_empty() {
            // 检查是否有状态码（NXDOMAIN/SERVFAIL 等）
            let rcode = tr.server_results.iter().find_map(|sr| {
                if !sr.response_code.is_empty()
                    && !sr.response_code.contains("NOERROR")
                    && !sr.response_code.contains("NOERROR")
                {
                    Some(sr.response_code.clone())
                } else {
                    None
                }
            });
            if let Some(code) = rcode {
                // 有非 NoError 状态码：显示之（如 NXDOMAIN、SERVFAIL）
                if !first {
                    out.push('\n');
                }
                first = false;
                let type_line = format!("{}", tr.record_type.to_string().bold().cyan(),);
                out.push_str(&type_line);
                out.push('\n');
                out.push_str(&format!("  {}\n", code.bright_black()));
                continue;
            }
            // 全部服务器都出错且无记录：any 模式下显示首条错误
            if tr.server_results.iter().all(|sr| sr.error.is_some()) {
                if !result.hide_empty {
                    let first_err = tr
                        .server_results
                        .iter()
                        .find_map(|sr| sr.error.as_ref())
                        .cloned()
                        .unwrap_or_else(|| "(无记录)".into());
                    out.push_str(&format!("  {}\n", first_err.red()));
                    continue;
                }
                continue;
            }
            // 普通无记录
            if !result.hide_empty {
                if !first {
                    out.push('\n');
                }
                first = false;
                let type_line = format!("{}", tr.record_type.to_string().bold().cyan(),);
                out.push_str(&type_line);
                out.push('\n');
                out.push_str(&format!("  {}\n", "(无记录)".bright_black()));
                continue;
            }
            continue;
        }
        if !first {
            out.push('\n');
        }
        first = false;
        total_types += 1;
        total_records += entries.len();

        // 类型名
        let type_line = format!("{}", tr.record_type.to_string().bold().cyan(),);
        out.push_str(&type_line);
        out.push('\n');

        // 部分服务器出错（非完全不可用）的，verbose 下在记录后显示
        // （完全不可用的服务器在末尾汇总，不在此处重复）

        if entries.is_empty() {
            out.push_str(&format!("  {}\n", "(无记录)".bright_black()));
            continue;
        }

        for e in &entries {
            let mut line = format_value_with_ttl(&e.value, e.ttl, opts);
            // verbose 标注一致数
            if opts.verbose && total_servers > 1 {
                line.push_str(&format!(
                    "  {}",
                    format!("({}/{})", e.count, total_servers).bright_black()
                ));
            }
            // 差异高亮：TTY 下 magenta 加粗
            let line = if has_diff && e.count < total_servers {
                line.magenta().bold().to_string()
            } else {
                colorize_record(&tr.record_type.to_string(), &line)
            };
            out.push_str(&format!("  {line}\n"));
        }

        // verbose 模式：显示标志位、报文大小、Authority/Additional 段
        if opts.verbose {
            for sr in &tr.server_results {
                if sr.records.is_empty() && sr.error.is_some() {
                    continue; // 跳过出错的服务器
                }
                if !sr.flags.is_empty() {
                    let dnssec_tag = if sr.dnssec_validated {
                        " DNSSEC✓"
                    } else {
                        ""
                    };
                    out.push_str(&format!(
                        "  {} flags={} size={}B{}\n",
                        sr.server_name.bright_black(),
                        sr.flags.bright_black(),
                        sr.msg_size,
                        dnssec_tag.green(),
                    ));
                }
                // Authority 段
                if !sr.authorities.is_empty() {
                    out.push_str(&format!("  {} Authority:\n", sr.server_name.bright_black()));
                    for auth in &sr.authorities {
                        out.push_str(&format!("    {}\n", auth.value.bright_black()));
                    }
                }
                // Additional 段
                if !sr.additionals.is_empty() {
                    out.push_str(&format!(
                        "  {} Additional:\n",
                        sr.server_name.bright_black()
                    ));
                    for add in &sr.additionals {
                        out.push_str(&format!("    {}\n", add.value.bright_black()));
                    }
                }
                break; // 只显示第一个成功服务器的详细信息
            }
        }
    }

    // TTY 汇总行
    if total_types > 0 {
        out.push_str(&format!(
            "\n{}\n",
            format!(
                "共 {total_records} 条记录，{total_types} 个类型，{total_servers_count} 个 DNS 服务器"
            )
            .bright_black()
        ));
    }

    // 不可用服务器汇总（verbose 模式）：按错误类型分类
    if !dead_servers.is_empty() {
        // 分类：超时 vs 连接错误
        let (timeouts, errors): (Vec<_>, Vec<_>) = dead_servers.iter().partition(|(_, _, err)| {
            err.contains("超时") || err.contains("timeout") || err.contains("Timeout")
        });

        out.push_str(&format!(
            "{}\n",
            format!("{} 个 DNS 服务器不可用：", dead_servers.len()).red()
        ));

        if !timeouts.is_empty() {
            out.push_str(&format!(
                "  {}（响应慢，可调大 --timeout）：\n",
                "超时".yellow()
            ));
            for (name, desc, _err) in &timeouts {
                out.push_str(&format!(
                    "    {} {}\n",
                    name.bright_black(),
                    desc.bright_black()
                ));
            }
        }
        if !errors.is_empty() {
            out.push_str(&format!("  {}（连接失败）：\n", "连接错误".red()));
            for (name, desc, err) in &errors {
                out.push_str(&format!(
                    "    {} {} — {}\n",
                    name.bright_black(),
                    desc.bright_black(),
                    err.red()
                ));
            }
        }
    }
}

/// 把值和可选 TTL 拼接。
fn format_value_with_ttl(value: &str, ttl: u32, opts: &OutputOpts) -> String {
    if opts.ttl || opts.verbose {
        format!("{value} (ttl={ttl})")
    } else {
        value.to_string()
    }
}

/// 根据记录类型给记录值上色：IP 绿色，域名/主机名黄色，其余默认。
fn colorize_record(type_str: &str, value: &str) -> String {
    match type_str {
        "A" | "AAAA" => value.green().to_string(),
        "NS" | "CNAME" | "PTR" | "ANAME" | "SRV" => value.yellow().to_string(),
        "MX" => {
            if let Some((pref, host)) = value.split_once(' ') {
                format!("{} {}", pref.green(), host.yellow())
            } else {
                value.normal().to_string()
            }
        }
        _ => value.normal().to_string(),
    }
}

// ───────────────────── 非 TTY（纯文本） ─────────────────────

fn render_plain(out: &mut String, result: &QueryResult, opts: &OutputOpts) {
    // 如果所有类型都返回 NXDOMAIN，显示一条提示而非逐类型展开
    let all_nxdomain = result.results.iter().all(|tr| {
        tr.server_results
            .iter()
            .any(|sr| !sr.response_code.is_empty() && sr.response_code.contains("NXDOMAIN"))
    });
    if all_nxdomain && result.hide_empty {
        out.push_str("域名不存在 (NXDOMAIN)\n");
        return;
    }

    if opts.verbose {
        out.push_str(&format!(
            "# {} @ {} ({}ms)\n",
            result.domain, result.server_desc, result.elapsed_ms
        ));
    }

    let total_servers_count = result
        .results
        .first()
        .map(|tr| tr.server_results.len())
        .unwrap_or(0);

    // 收集完全不可用的服务器
    let mut dead_servers: Vec<(String, String, String)> = Vec::new();
    if opts.verbose
        && total_servers_count > 1
        && let Some(first_tr) = result.results.first()
    {
        for sr in &first_tr.server_results {
            let all_failed = result.results.iter().all(|tr| {
                tr.server_results
                    .iter()
                    .find(|s| s.server_name == sr.server_name && s.server_desc == sr.server_desc)
                    .is_some_and(|s| s.error.is_some() && s.records.is_empty())
            });
            if all_failed && let Some(err) = &sr.error {
                dead_servers.push((sr.server_name.clone(), sr.server_desc.clone(), err.clone()));
            }
        }
    }

    for tr in &result.results {
        if should_hide(tr, result.hide_empty) {
            continue;
        }
        let (entries, total_servers, _has_diff) = aggregate(tr);
        if entries.is_empty() && tr.server_results.iter().all(|sr| sr.error.is_some()) {
            if !result.hide_empty {
                let first_err = tr
                    .server_results
                    .iter()
                    .find_map(|sr| sr.error.as_ref())
                    .cloned()
                    .unwrap_or_else(|| "(none)".into());
                out.push_str(&format!("{}: {}\n", tr.record_type, first_err));
                continue;
            }
            continue;
        }

        if entries.is_empty() {
            out.push_str(&format!("{}: (none)\n", tr.record_type));
            continue;
        }

        for e in &entries {
            let val = format_value_with_ttl(&e.value, e.ttl, opts);
            if opts.verbose && total_servers > 1 {
                out.push_str(&format!(
                    "{}: {} ({}/{})\n",
                    tr.record_type, val, e.count, total_servers
                ));
            } else {
                out.push_str(&format!("{}: {}\n", tr.record_type, val));
            }
        }

        // verbose：标志位、报文大小、Authority/Additional 段
        if opts.verbose {
            for sr in &tr.server_results {
                if sr.records.is_empty() && sr.error.is_some() {
                    continue;
                }
                if !sr.flags.is_empty() {
                    let dnssec_tag = if sr.dnssec_validated {
                        " DNSSEC✓"
                    } else {
                        ""
                    };
                    out.push_str(&format!(
                        "{}: flags={} size={}B{}\n",
                        tr.record_type, sr.flags, sr.msg_size, dnssec_tag
                    ));
                }
                if !sr.authorities.is_empty() {
                    out.push_str(&format!("{} Authority:\n", tr.record_type));
                    for auth in &sr.authorities {
                        out.push_str(&format!("  {}\n", auth.value));
                    }
                }
                if !sr.additionals.is_empty() {
                    out.push_str(&format!("{} Additional:\n", tr.record_type));
                    for add in &sr.additionals {
                        out.push_str(&format!("  {}\n", add.value));
                    }
                }
                break;
            }
        }
    }

    // 不可用服务器汇总（verbose 模式）：按错误类型分类
    if !dead_servers.is_empty() {
        let (timeouts, errors): (Vec<_>, Vec<_>) = dead_servers.iter().partition(|(_, _, err)| {
            err.contains("超时") || err.contains("timeout") || err.contains("Timeout")
        });

        out.push_str(&format!("# {} 个 DNS 服务器不可用：\n", dead_servers.len()));

        if !timeouts.is_empty() {
            out.push_str("#   超时（响应慢，可调大 --timeout）：\n");
            for (name, desc, _err) in &timeouts {
                out.push_str(&format!("#     {name} {desc}\n"));
            }
        }
        if !errors.is_empty() {
            out.push_str("#   连接失败：\n");
            for (name, desc, err) in &errors {
                out.push_str(&format!("#     {name} {desc} — {err}\n"));
            }
        }
    }
}

/// 兼容写入任意 `Write` sink（暂未用，预留）。
#[allow(dead_code)]
pub fn write_to<W: Write>(writer: &mut W, result: &QueryResult, opts: &OutputOpts) {
    let mut s = String::new();
    let use_color = match opts.color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => std::io::stdout().is_terminal(),
    };
    if opts.json {
        render_json(&mut s, result);
    } else if opts.short {
        render_short(&mut s, result);
    } else if use_color {
        render_tty(&mut s, result, opts);
    } else {
        render_plain(&mut s, result, opts);
    }
    let _ = writer.write_all(s.as_bytes());
}
