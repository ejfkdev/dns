//! 多语言帮助信息（中英文），含常用命令示例。
//!
//! 运行时检测系统语言，中文环境显示中文帮助，否则英文。

use std::process::exit;

/// 检测系统是否为中文环境。
pub fn is_chinese() -> bool {
    // 1. 环境变量（优先级最高，显式设置覆盖系统偏好）
    for var in &["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            let lower = val.to_lowercase();
            if lower.contains("zh") || lower.contains("chinese") {
                return true;
            }
            // 明确设了非中文语言（如 en_US、ja_JP），以环境变量为准
            if lower.len() >= 2 && !lower.starts_with("c") && !lower.starts_with("posix") {
                return false;
            }
        }
    }
    // 2. macOS: 读 AppleLanguages（环境变量未明确时用系统偏好）
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if text.contains("zh") {
                return true;
            }
        }
    }
    false
}

/// 如果参数含 -h 或 --help，打印帮助并退出。
/// 但跳过 serve/mcp 开头的参数（由 xyz-rust 处理自己的 help）。
pub fn check_help(args: &[String]) {
    let has_help = args.iter().any(|a| a == "-h" || a == "--help");
    if !has_help {
        return;
    }
    // serve/mcp 的 help 由 xyz-rust 处理
    if args.iter().take(2).any(|a| a == "serve" || a == "mcp") {
        return;
    }
    if is_chinese() {
        print_zh();
    } else {
        print_en();
    }
    exit(0);
}

/// 打印当前系统的配置文件路径（中文）。
fn print_config_path_zh() {
    match dirs::config_dir() {
        Some(base) => {
            let path = base.join("ejfkdev").join("dns").join("config.toml");
            println!("配置文件: {}", path.display());
        }
        None => println!("配置文件: 无法确定系统配置目录"),
    }
}

/// 打印当前系统的配置文件路径（英文）。
fn print_config_path_en() {
    match dirs::config_dir() {
        Some(base) => {
            let path = base.join("ejfkdev").join("dns").join("config.toml");
            println!("Config: {}", path.display());
        }
        None => println!("Config: cannot determine system config directory"),
    }
}

pub fn print_zh() {
    println!(
        "\x1b[1mdns\x1b[0m v{} — 向多个 DNS 服务器并发查询所有记录类型并汇总",
        env!("CARGO_PKG_VERSION")
    );
    println!("仓库: https://github.com/ejfkdev/dns");
    println!();
    println!("\x1b[1m用法\x1b[0m:  dns [类型|域名|@服务器] ... [选项]");
    println!();
    println!("\x1b[1m常用命令\x1b[0m:");
    let cmds = [
        (
            "dns example.com",
            "向所有内置服务器查询全部记录类型（默认隐藏无记录项）",
        ),
        (
            "dns any example.com",
            "查询 RFC 全量类型，显示全部（含无记录与错误）",
        ),
        ("dns mx example.com", "只查 MX 记录"),
        ("dns A CNAME example.com", "查多个指定类型"),
        ("dns A AAAA example.com github.com", "多类型 + 多域名"),
        ("dns example.com github.com", "批量查询多个域名"),
        (
            "dns A AAAA a.com b.com @8.8.8.8 @1.1.1.1",
            "多类型 + 多域名 + 多服务器",
        ),
        ("dns @8.8.8.8 example.com", "指定 DNS 服务器（@ 语法）"),
        ("dns 8.8.8.8", "IP 反查 PTR"),
        ("dns 中文.com", "中文域名自动 Punycode 转码"),
        ("dns axfr example.com", "AXFR 区传送（自动获取权威 NS）"),
        ("dns example.com --region cn", "只用中国 DNS 服务器"),
        ("dns example.com -v", "详细模式：耗时、一致数、状态码"),
        ("dns example.com --ttl", "显示 TTL"),
        ("dns example.com --json", "JSON 输出（便于 | jq）"),
        ("dns example.com --short", "极简输出（仅值，便于 | xargs）"),
        (
            "dns a.com b.com --merge",
            "合并多域名结果（同类型合并展示）",
        ),
        ("cat domains.txt | dns A --merge", "从管道读取域名 + 合并"),
        ("cat domains.txt | dns --csv", "从管道读取域名 + CSV 输出"),
        ("cat domains.txt | dns --json", "从管道读取域名 + JSON 输出"),
        (
            "dns serve --xyz.addr 127.0.0.1:8080",
            "HTTP REST API + OpenAPI",
        ),
        ("dns mcp stdio", "MCP 工具服务器（AI 客户端可调用）"),
    ];
    let max = cmds
        .iter()
        .map(|(c, _)| c.chars().count())
        .max()
        .unwrap_or(0);
    for (cmd, desc) in &cmds {
        let pad = max - cmd.chars().count();
        println!("  \x1b[36m{}\x1b[0m{}  {}", cmd, " ".repeat(pad), desc);
    }
    println!();
    println!("\x1b[1m特殊关键字\x1b[0m（与 RFC 记录类型区分）:");
    let keywords = [
        ("any", "查询全部 RFC 记录类型（显示所有，含无记录与错误）"),
        ("axfr", "AXFR 区传送（自动获取域名权威 NS，需授权）"),
    ];
    let max_k = keywords
        .iter()
        .map(|(c, _)| c.chars().count())
        .max()
        .unwrap_or(0);
    for (kw, desc) in &keywords {
        let pad = max_k - kw.chars().count();
        println!("  \x1b[33m{}\x1b[0m{}  {}", kw, " ".repeat(pad), desc);
    }
    println!();
    println!("\x1b[1mRFC 记录类型\x1b[0m（可任意组合，如：dns A AAAA MX example.com）:");
    println!("  A AAAA CNAME MX NS TXT SOA SRV CAA PTR HINFO");
    println!("  DNSKEY DS RRSIG NSEC TLSA SVCB HTTPS ... 等全部 RFC 类型");
    println!();
    println!("\x1b[1m--server 服务器格式\x1b[0m:");
    let srvs = [
        ("8.8.8.8", "UDP（默认端口 53）"),
        ("8.8.8.8:5353", "UDP 指定端口"),
        ("a.gtld-servers.net", "域名（自动解析为 IP）"),
        ("tls://1.1.1.1", "DNS-over-TLS（默认 853）"),
        ("tls://1.1.1.1?sn=cloudflare-dns.com", "DoT 指定 SNI"),
        (
            "quic://1.1.1.1?sn=cloudflare-dns.com",
            "DNS-over-QUIC（默认 853）",
        ),
        ("https://doh.pub/dns-query", "DoH（标准 RFC 8444）"),
        ("http://119.29.29.29/d", "腾讯私有 HTTPDNS（自动适配）"),
    ];
    let max_s = srvs
        .iter()
        .map(|(c, _)| c.chars().count())
        .max()
        .unwrap_or(0);
    for (cmd, desc) in &srvs {
        let pad = max_s - cmd.chars().count();
        println!("  {}{}  {}", cmd, " ".repeat(pad), desc);
    }
    println!();
    println!("\x1b[1m选项\x1b[0m:");
    let opts = [
        ("--server <ADDR>", "指定 DNS 服务器（可多个，省略时用内置）"),
        (
            "--region <REGION>",
            "筛选内置服务器：global / cn（省略时用全部）",
        ),
        ("--ttl", "显示 TTL"),
        ("--short", "极简输出"),
        ("--json", "JSON 输出"),
        ("--yaml", "YAML 输出"),
        ("--csv", "CSV 输出"),
        ("--merge", "合并多域名结果（同类型记录合并）"),
        ("-v, --verbose", "详细输出"),
        ("--timeout <SECS>", "超时秒数（默认 2）"),
        ("--color <WHEN>", "颜色：auto / always / never（默认 auto）"),
        ("--list-servers", "列出内置 DNS 服务器"),
        ("--tcp", "强制 TCP 查询（默认 UDP）"),
        ("--file <FILE>", "从文件读取域名（每行一个，- 读 stdin）"),
        ("-4", "强制 IPv4"),
        ("-6", "强制 IPv6"),
        ("-h, --help", "显示此帮助"),
        ("-V, --version", "显示版本"),
    ];
    let max_o = opts
        .iter()
        .map(|(c, _)| c.chars().count())
        .max()
        .unwrap_or(0);
    for (cmd, desc) in &opts {
        let pad = max_o - cmd.chars().count();
        println!("      {}{}  {}", cmd, " ".repeat(pad), desc);
    }
    println!();
    print_config_path_zh();
}

pub fn print_en() {
    println!(
        "\x1b[1mdns\x1b[0m v{} — Query all DNS record types from multiple servers concurrently",
        env!("CARGO_PKG_VERSION")
    );
    println!("Repo: https://github.com/ejfkdev/dns");
    println!();
    println!("\x1b[1mUSAGE\x1b[0m:  dns [TYPE|DOMAIN|@SERVER] ... [OPTIONS]");
    println!();
    println!("\x1b[1mCOMMON COMMANDS\x1b[0m:");
    let cmds = [
        (
            "dns example.com",
            "Query all record types from all built-in servers",
        ),
        (
            "dns any example.com",
            "Query all RFC types, show everything (incl. empty/errors)",
        ),
        ("dns mx example.com", "Query only MX records"),
        ("dns A CNAME example.com", "Query multiple record types"),
        (
            "dns A AAAA example.com github.com",
            "Multiple types + multiple domains",
        ),
        ("dns example.com github.com", "Batch query multiple domains"),
        (
            "dns A AAAA a.com b.com @8.8.8.8 @1.1.1.1",
            "Multi-type + multi-domain + multi-server",
        ),
        ("dns @8.8.8.8 example.com", "Specify DNS server (@ syntax)"),
        ("dns 8.8.8.8", "Reverse lookup (PTR)"),
        (
            "dns axfr example.com",
            "AXFR zone transfer (auto-resolves authoritative NS)",
        ),
        (
            "dns example.com --region cn",
            "Use only Chinese DNS servers",
        ),
        (
            "dns example.com -v",
            "Verbose: timing, consensus, status codes",
        ),
        ("dns example.com --ttl", "Show TTL"),
        ("dns example.com --json", "JSON output (for | jq)"),
        (
            "dns example.com --short",
            "Short output (only values, for | xargs)",
        ),
        ("dns a.com b.com --merge", "Merge multiple domains' results"),
        (
            "cat domains.txt | dns A --merge",
            "Read domains from pipe + merge",
        ),
        (
            "cat domains.txt | dns --csv",
            "Read domains from pipe + CSV output",
        ),
        (
            "cat domains.txt | dns --json",
            "Read domains from pipe + JSON output",
        ),
        (
            "dns serve --xyz.addr 127.0.0.1:8080",
            "HTTP REST API + OpenAPI",
        ),
        ("dns mcp stdio", "MCP tool server (for AI clients)"),
    ];
    let max = cmds
        .iter()
        .map(|(c, _)| c.chars().count())
        .max()
        .unwrap_or(0);
    for (cmd, desc) in &cmds {
        let pad = max - cmd.chars().count();
        println!("  \x1b[36m{}\x1b[0m{}  {}", cmd, " ".repeat(pad), desc);
    }
    println!();
    println!("\x1b[1mSPECIAL KEYWORDS\x1b[0m (distinct from RFC record types):");
    let keywords = [
        (
            "any",
            "Query all RFC record types (show everything, incl. empty/errors)",
        ),
        (
            "axfr",
            "AXFR zone transfer (auto-resolves authoritative NS, needs auth)",
        ),
    ];
    let max_k = keywords
        .iter()
        .map(|(c, _)| c.chars().count())
        .max()
        .unwrap_or(0);
    for (kw, desc) in &keywords {
        let pad = max_k - kw.chars().count();
        println!("  \x1b[33m{}\x1b[0m{}  {}", kw, " ".repeat(pad), desc);
    }
    println!();
    println!("\x1b[1mRFC RECORD TYPES\x1b[0m (can be combined, e.g.: dns A AAAA MX example.com):");
    println!("  A AAAA CNAME MX NS TXT SOA SRV CAA PTR HINFO");
    println!("  DNSKEY DS RRSIG NSEC TLSA SVCB HTTPS ... all RFC types supported");
    println!();
    println!("\x1b[1m--server FORMATS\x1b[0m:");
    let srvs = [
        ("8.8.8.8", "UDP (default port 53)"),
        ("8.8.8.8:5353", "UDP with custom port"),
        ("a.gtld-servers.net", "Domain name (auto-resolved to IP)"),
        ("tls://1.1.1.1", "DNS-over-TLS (default 853)"),
        ("tls://1.1.1.1?sn=cloudflare-dns.com", "DoT with SNI"),
        (
            "quic://1.1.1.1?sn=cloudflare-dns.com",
            "DNS-over-QUIC (default 853)",
        ),
        ("https://doh.pub/dns-query", "DoH (RFC 8444)"),
        ("http://119.29.29.29/d", "Tencent HTTPDNS (auto-detected)"),
    ];
    let max_s = srvs
        .iter()
        .map(|(c, _)| c.chars().count())
        .max()
        .unwrap_or(0);
    for (cmd, desc) in &srvs {
        let pad = max_s - cmd.chars().count();
        println!("  {}{}  {}", cmd, " ".repeat(pad), desc);
    }
    println!();
    println!("\x1b[1mOPTIONS\x1b[0m:");
    let opts = [
        (
            "--server <ADDR>",
            "Specify DNS server(s), repeatable (default: built-in)",
        ),
        (
            "--region <REGION>",
            "Filter built-in servers: global / cn (default: all)",
        ),
        ("--ttl", "Show TTL"),
        ("--short", "Short output"),
        ("--json", "JSON output"),
        ("--yaml", "YAML output"),
        ("--csv", "CSV output"),
        (
            "--merge",
            "Merge multiple domains' results (by record type)",
        ),
        ("-v, --verbose", "Verbose output"),
        ("--timeout <SECS>", "Timeout in seconds (default 2)"),
        (
            "--color <WHEN>",
            "Color: auto / always / never (default auto)",
        ),
        ("--list-servers", "List built-in DNS servers"),
        ("--tcp", "Force TCP query (default UDP)"),
        (
            "--file <FILE>",
            "Read domains from file (one per line, - for stdin)",
        ),
        ("-4", "Force IPv4"),
        ("-6", "Force IPv6"),
        ("-h, --help", "Show this help"),
        ("-V, --version", "Show version"),
    ];
    let max_o = opts
        .iter()
        .map(|(c, _)| c.chars().count())
        .max()
        .unwrap_or(0);
    for (cmd, desc) in &opts {
        let pad = max_o - cmd.chars().count();
        println!("      {}{}  {}", cmd, " ".repeat(pad), desc);
    }
    println!();
    print_config_path_en();
}
