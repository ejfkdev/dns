//! 命令行参数定义与解析。
//!
//! 支持：
//! - 多域名批量：dns a.com b.com
//! - @server 语法：dns @8.8.8.8 example.com
//! - IDN Punycode：中文域名自动转码
//! - --trace / --subnet / --yaml 等高级选项

use std::net::IpAddr;

use clap::Parser;
use std::io::IsTerminal;

use crate::record_types;

/// 颜色控制。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl std::str::FromStr for ColorMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(ColorMode::Auto),
            "always" | "yes" | "on" | "true" => Ok(ColorMode::Always),
            "never" | "no" | "off" | "false" => Ok(ColorMode::Never),
            _ => Err(format!("无效的 color 值: `{s}`（可选 auto/always/never")),
        }
    }
}

/// 解析后的命令行参数。
#[derive(Parser, Debug)]
#[command(
    name = "dns",
    version,
    about = "向多个 DNS 服务器并发查询所有记录类型并汇总（区别于 dig/dog）",
    long_about = "dns 命令行工具：默认向所有内置 DNS 服务器 + 本机 DNS 并发查询所有记录类型，\n\
                  汇总去重后展示，可直观看出不同服务器返回结果的差异。\n\n\
                  支持多域名批量、IDN 中文域名、AXFR 区传送、DoT/DoH/DoQ 等。",
    override_usage = "dns [TYPE|DOMAIN|@SERVER] ... [OPTIONS]"
)]
pub struct Args {
    /// 位置参数：类型关键字 / any / 域名 / IP / @server / axfr
    #[arg(help = "记录类型、any、域名、IP、或 @server")]
    pub args: Vec<String>,

    /// 指定 DNS 服务器（可多个，省略时用内置服务器）。支持 IP/IP:port/域名/tls://https://
    #[arg(long, value_name = "ADDR")]
    pub server: Vec<String>,

    /// 筛选内置服务器 region：global / cn
    #[arg(long, value_name = "REGION")]
    pub region: Option<String>,

    /// 显示 TTL
    #[arg(long)]
    pub ttl: bool,

    /// 极简输出：仅记录值
    #[arg(long)]
    pub short: bool,

    /// 合并 + 极简（等价于 --merge --short，便于管道使用）
    #[arg(short = 'm', long)]
    pub merge: bool,

    /// JSON 输出
    #[arg(long)]
    pub json: bool,

    /// YAML 输出
    #[arg(long)]
    pub yaml: bool,

    /// CSV 输出
    #[arg(long)]
    pub csv: bool,

    /// 详细：服务器、耗时、一致数、authority/additional 段
    #[arg(short, long)]
    pub verbose: bool,

    /// 查询超时秒数（默认 2）
    #[arg(long, default_value_t = 2)]
    pub timeout: u64,

    /// 颜色控制：auto / always / never
    #[arg(long, value_name = "WHEN", default_value = "auto")]
    pub color: ColorMode,

    /// 列出内置 DNS 服务器后退出
    #[arg(long)]
    pub list_servers: bool,

    /// 强制 TCP 查询（默认 UDP）
    #[arg(long)]
    pub tcp: bool,

    /// 从文件读取域名（每行一个），- 读 stdin
    #[arg(long, value_name = "FILE")]
    pub file: Option<String>,

    /// 强制 IPv4 查询传输
    #[arg(short = '4')]
    pub ipv4: bool,

    /// 强制 IPv6 查询传输
    #[arg(short = '6')]
    pub ipv6: bool,
}

/// normalize 后的查询目标。
#[derive(Debug, Clone)]
pub struct QueryTarget {
    /// 查询的域名（已 Punycode 转码）
    pub domain: String,
    /// 显示用的原始域名
    pub display_domain: String,
    /// 查询类型列表
    pub types: Vec<hickory_proto::rr::RecordType>,
    /// 是否隐藏无记录项
    pub hide_empty: bool,
    /// 是否为 IP 反查（PTR）
    pub is_ptr: bool,
    /// 是否为 AXFR 区传送
    pub is_axfr: bool,
}

impl Args {
    /// 解析命令行。
    pub fn parse_cli() -> Result<Self, String> {
        let mut cli = Args::parse();
        if cli.list_servers {
            return Ok(cli);
        }
        cli.normalize()?;
        Ok(cli)
    }

    /// 把位置参数解析为多个 QueryTarget（支持多域名）。
    fn normalize(&mut self) -> Result<(), String> {
        // --file 模式或 stdin 管道时可以无位置参数
        if self.args.is_empty() && self.file.is_none() && std::io::stdin().is_terminal() {
            return Err("缺少域名参数。用法: dns [类型] <域名>  (例: dns example.com)".into());
        }
        Ok(())
    }

    /// 从位置参数提取查询目标列表。
    pub fn query_targets(&self) -> Result<Vec<QueryTarget>, String> {
        if self.args.is_empty() {
            return Ok(Vec::new());
        }
        parse_positional_args(&self.args)
    }
}

/// 解析位置参数，返回多个查询目标。
///
/// 支持格式：
/// - dns example.com              # 单域名，默认查全部
/// - dns mx example.com           # 单类型 + 域名
/// - dns A CNAME example.com      # 多类型 + 域名
/// - dns mx example.com a.com     # 类型 + 多域名
/// - dns A AAAA example.com a.com # 多类型 + 多域名
/// - dns @8.8.8.8 example.com     # @server 语法
/// - dns 8.8.8.8                   # IP 反查 PTR
/// - dns axfr example.com          # AXFR 区传送
fn parse_positional_args(args: &[String]) -> Result<Vec<QueryTarget>, String> {
    let mut filtered = Vec::new();
    for a in args {
        if a.starts_with('@') {
            continue; // @server 在 main.rs 处理，这里跳过
        }
        filtered.push(a.clone());
    }

    if filtered.is_empty() {
        return Err("缺少域名参数。".into());
    }

    // 收集开头的连续类型关键字
    let mut type_keywords: Vec<String> = Vec::new();
    for arg in &filtered {
        if record_types::is_type_keyword(arg) || arg.eq_ignore_ascii_case("axfr") {
            type_keywords.push(arg.clone());
        } else {
            break;
        }
    }

    if type_keywords.is_empty() {
        // 无类型关键字：每个含点的参数当域名，默认查全部
        filtered
            .iter()
            .map(|d| build_target(d, record_types::ALL_TYPES.to_vec(), true))
            .collect()
    } else {
        // 有类型关键字：解析为类型列表
        let mut types = Vec::new();
        for kw in &type_keywords {
            let resolved = resolve_types(kw)?;
            types.extend_from_slice(resolved);
        }
        // 去重（any 会展开为 ALL_TYPES，与其他类型组合时去重）
        types.sort();
        types.dedup();

        let domains = &filtered[type_keywords.len()..];
        if domains.is_empty() {
            // 有类型但无域名：返回空 Vec，由 main.rs 从 stdin/文件补域名
            return Ok(Vec::new());
        }
        domains
            .iter()
            .map(|d| build_target(d, types.clone(), false))
            .collect()
    }
}

/// 构建单个查询目标（含 IDN 转码和 IP 反查）。
fn build_target(
    input: &str,
    types: Vec<hickory_proto::rr::RecordType>,
    hide_empty: bool,
) -> Result<QueryTarget, String> {
    let trimmed = input.trim();

    // AXFR 标记
    let is_axfr = types.contains(&hickory_proto::rr::RecordType::AXFR);

    // IP 反查
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Ok(QueryTarget {
            domain: reverse_arpa(&ip),
            display_domain: trimmed.to_string(),
            types: vec![hickory_proto::rr::RecordType::PTR],
            hide_empty: false,
            is_ptr: true,
            is_axfr: false,
        });
    }

    // IDN Punycode 转码
    let (domain, display_domain) = if is_ascii(trimmed) {
        (trimmed.to_string(), trimmed.to_string())
    } else {
        let ascii = idna::domain_to_ascii(trimmed).unwrap_or_else(|_| trimmed.to_string());
        (ascii, trimmed.to_string())
    };

    if !domain.contains('.') && !domain.contains(':') {
        return Err(format!(
            "`{domain}` 看起来不像域名（应含点）。若要查询本地后缀可加 `.`，如 {domain}."
        ));
    }

    Ok(QueryTarget {
        domain,
        display_domain,
        types,
        hide_empty,
        is_ptr: false,
        is_axfr,
    })
}

/// 判断字符串是否全 ASCII。
fn is_ascii(s: &str) -> bool {
    s.is_ascii()
}

/// 把 IP 转为反向 arpa 域名。
fn reverse_arpa(ip: &IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(v6) => {
            let s = v6.octets();
            let mut hex = String::with_capacity(32);
            for b in s {
                use std::fmt::Write;
                write!(&mut hex, "{b:02x}").unwrap();
            }
            let nibbles: Vec<char> = hex.chars().rev().collect();
            let parts: Vec<String> = nibbles.iter().map(|c| c.to_string()).collect();
            format!("{}.ip6.arpa", parts.join("."))
        }
    }
}

/// 从位置参数提取所有 @server（如 `@8.8.8.8`、`@1.1.1.1`）。
pub fn extract_at_server(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|a| a.starts_with('@'))
        .map(|a| a[1..].to_string())
        .collect()
}

/// 把类型关键字解析为记录类型列表。
fn resolve_types(keyword: &str) -> Result<&'static [hickory_proto::rr::RecordType], String> {
    let upper = keyword.to_ascii_uppercase();
    match upper.as_str() {
        "ANY" | "*" => Ok(record_types::ALL_TYPES),
        "AXFR" => Ok(std::slice::from_ref(&hickory_proto::rr::RecordType::AXFR)),
        _ => match single_type_slice(keyword) {
            Some(t) => Ok(std::slice::from_ref(t)),
            None => Err(format!("不支持的记录类型: `{keyword}`")),
        },
    }
}

/// 将已知类型关键字映射到其 `&'static RecordType` 引用。
pub fn single_type_slice(keyword: &str) -> Option<&'static hickory_proto::rr::RecordType> {
    use hickory_proto::rr::RecordType as R;
    const TABLE: &[R] = &[
        R::A,
        R::AAAA,
        R::CNAME,
        R::MX,
        R::NS,
        R::TXT,
        R::SOA,
        R::SRV,
        R::CAA,
        R::PTR,
        R::HINFO,
        R::ANAME,
        R::DNSKEY,
        R::DS,
        R::RRSIG,
        R::NSEC,
        R::NSEC3,
        R::NSEC3PARAM,
        R::TLSA,
        R::SMIMEA,
        R::SSHFP,
        R::NAPTR,
        R::OPENPGPKEY,
        R::KEY,
        R::CERT,
        R::CSYNC,
        R::SVCB,
        R::HTTPS,
        R::AXFR,
    ];
    let upper = keyword.to_ascii_uppercase();
    TABLE
        .iter()
        .position(|t| t.to_string() == upper)
        .map(|i| &TABLE[i])
}
