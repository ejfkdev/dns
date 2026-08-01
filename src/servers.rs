//! 内置 DNS 服务器清单，按 region 分组。
//!
//! 默认查询时向所有内置服务器 + 用户本机 DNS 并发查询，汇总结果。
//! 可用 `--region global` 或 `--region cn` 筛选子集。

use std::net::{IpAddr, Ipv4Addr};

/// DNS 服务器 region。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// 国际服务器
    Global,
    /// 中国主流服务器
    Cn,
}

impl Region {
    pub fn as_str(self) -> &'static str {
        match self {
            Region::Global => "global",
            Region::Cn => "cn",
        }
    }
}

/// 一台内置 DNS 服务器。
#[derive(Debug, Clone)]
pub struct DnsServer {
    /// 短名，如 "google"、"alidns"
    pub name: &'static str,
    /// 服务器 IP
    pub ip: IpAddr,
    /// 所属 region
    pub region: Region,
    /// 人类可读说明
    pub desc: &'static str,
}

/// 全部内置 DNS 服务器（global + cn）。
pub const BUILTIN_SERVERS: &[DnsServer] = &[
    // ── global ──
    DnsServer {
        name: "google",
        ip: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
        region: Region::Global,
        desc: "Google Public DNS",
    },
    DnsServer {
        name: "google-2",
        ip: IpAddr::V4(Ipv4Addr::new(8, 8, 4, 4)),
        region: Region::Global,
        desc: "Google Public DNS (备用)",
    },
    DnsServer {
        name: "cloudflare",
        ip: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
        region: Region::Global,
        desc: "Cloudflare DNS",
    },
    DnsServer {
        name: "cloudflare-2",
        ip: IpAddr::V4(Ipv4Addr::new(1, 0, 0, 1)),
        region: Region::Global,
        desc: "Cloudflare DNS (备用)",
    },
    DnsServer {
        name: "quad9",
        ip: IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)),
        region: Region::Global,
        desc: "Quad9 (隐私+安全)",
    },
    DnsServer {
        name: "opendns",
        ip: IpAddr::V4(Ipv4Addr::new(208, 67, 222, 222)),
        region: Region::Global,
        desc: "OpenDNS (Cisco)",
    },
    DnsServer {
        name: "verisign",
        ip: IpAddr::V4(Ipv4Addr::new(64, 6, 64, 6)),
        region: Region::Global,
        desc: "Verisign Public DNS",
    },
    // ── cn ──
    DnsServer {
        name: "alidns",
        ip: IpAddr::V4(Ipv4Addr::new(223, 5, 5, 5)),
        region: Region::Cn,
        desc: "阿里 AliDNS",
    },
    DnsServer {
        name: "alidns-2",
        ip: IpAddr::V4(Ipv4Addr::new(223, 6, 6, 6)),
        region: Region::Cn,
        desc: "阿里 AliDNS (备用)",
    },
    DnsServer {
        name: "tencent",
        ip: IpAddr::V4(Ipv4Addr::new(119, 29, 29, 29)),
        region: Region::Cn,
        desc: "腾讯 DNSPod",
    },
    DnsServer {
        name: "tencent-2",
        ip: IpAddr::V4(Ipv4Addr::new(119, 28, 28, 28)),
        region: Region::Cn,
        desc: "腾讯 DNSPod (备用)",
    },
    DnsServer {
        name: "baidu",
        ip: IpAddr::V4(Ipv4Addr::new(180, 76, 76, 76)),
        region: Region::Cn,
        desc: "百度 BaiduDNS",
    },
    DnsServer {
        name: "114dns",
        ip: IpAddr::V4(Ipv4Addr::new(114, 114, 114, 114)),
        region: Region::Cn,
        desc: "114DNS",
    },
];

/// 按 region 筛选内置服务器。`None` 表示全部。
pub fn servers_by_region(region: Option<Region>) -> Vec<&'static DnsServer> {
    BUILTIN_SERVERS
        .iter()
        .filter(|s| region.is_none_or(|r| s.region == r))
        .collect()
}

/// 解析 region 字符串。
pub fn parse_region(s: &str) -> Option<Region> {
    match s.to_ascii_lowercase().as_str() {
        "global" | "all" => Some(Region::Global),
        "cn" | "china" | "中国" => Some(Region::Cn),
        _ => None,
    }
}

/// 打印内置服务器清单（供 `--list-servers` 使用）。
pub fn print_list() {
    println!("内置 DNS 服务器清单：\n");
    for region in [Region::Global, Region::Cn] {
        println!("【{}】", region.as_str());
        for s in servers_by_region(Some(region)) {
            println!("  {:<12} {:<16} {}", s.name, s.ip, s.desc);
        }
        println!();
    }
    println!("用法：dns <域名>                      # 默认用全部服务器");
    println!("      dns <域名> --region global       # 只用国际服务器");
    println!("      dns <域名> --region cn           # 只用中国服务器");
    println!("      dns <域名> --server 8.8.8.8      # 退回单服务器模式");
}
