//! DNS resolver 封装：多服务器并发查询、TTL、超时控制、RData 格式化。

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures::future::join_all;
use hickory_proto::rr::{Name, RData, RecordType};
use hickory_resolver::TokioResolver;
use hickory_resolver::config::{NameServerConfig, ResolverConfig};
use hickory_resolver::net::runtime::TokioRuntimeProvider;

/// DNS 协议类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Protocol {
    Udp,
    Tls,
    Https,
    /// DNS-over-QUIC
    Quic,
    /// 私有 HTTPDNS（http:// 或 https:// URL，智能兼容多格式）
    Httpdns,
}

/// 一台查询目标的协议规格（用于构造 resolver）。
#[derive(Debug, Clone)]
pub struct ServerSpec {
    pub name: String,
    pub ip: IpAddr,
    pub port: u16,
    pub protocol: Protocol,
    /// TLS/HTTPS 的 SNI server_name（UDP 时忽略）
    pub tls_server_name: Option<String>,
    /// HTTPS 的 path（None 时用默认 /dns-query）
    pub https_path: Option<String>,
    /// HTTPDNS 完整 URL（仅 protocol==Httpdns 时有效）
    pub httpdns_url: Option<String>,
    /// 强制 TCP 查询（忽略 UDP）
    pub force_tcp: bool,
}

impl ServerSpec {
    /// UDP 便捷构造。
    pub fn udp(name: impl Into<String>, ip: IpAddr) -> Self {
        Self {
            name: name.into(),
            ip,
            port: 53,
            protocol: Protocol::Udp,
            tls_server_name: None,
            https_path: None,
            httpdns_url: None,
            force_tcp: false,
        }
    }

    /// HTTPDNS 便捷构造。
    pub fn httpdns(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ip: IpAddr::from([0u8, 0, 0, 0]), // 占位，HTTPDNS 用 URL 不用 IP
            port: 0,
            protocol: Protocol::Httpdns,
            tls_server_name: None,
            https_path: None,
            httpdns_url: Some(url.into()),
            force_tcp: false,
        }
    }

    /// 可读描述，如 "8.8.8.8:53" 或 "1.1.1.1:443 (tls)" 或 "https://... (httpdns)"。
    pub fn desc(&self) -> String {
        match self.protocol {
            Protocol::Udp => format!("{}:{}", self.ip, self.port),
            Protocol::Tls => format!("{}:{} (tls)", self.ip, self.port),
            Protocol::Https => format!("{}:{} (https)", self.ip, self.port),
            Protocol::Quic => format!("{}:{} (quic)", self.ip, self.port),
            Protocol::Httpdns => {
                format!("{} (httpdns)", self.httpdns_url.as_deref().unwrap_or("?"))
            }
        }
    }
}

/// 单条记录值 + TTL。
#[derive(Debug, Clone)]
pub struct RecordEntry {
    pub value: String,
    pub ttl: u32,
}

/// 单个服务器对单个类型的查询结果。
#[derive(Debug, Clone)]
pub struct ServerTypeResult {
    /// 服务器名
    pub server_name: String,
    /// 服务器描述
    pub server_desc: String,
    /// 返回的记录（含 TTL）
    pub records: Vec<RecordEntry>,
    /// Authority 段记录（SOA、NS 等）
    pub authorities: Vec<RecordEntry>,
    /// Additional 段记录（glue records 等）
    pub additionals: Vec<RecordEntry>,
    /// DNS 响应状态码（RFC 标准短码：NOERROR/NXDOMAIN/SERVFAIL 等）
    pub response_code: String,
    /// DNS 标志位（如 "RD,RA" 或 "RD,AA,RA,AD"）
    pub flags: String,
    /// 响应报文大小（字节）
    pub msg_size: usize,
    /// DNSSEC 验证结果（true=AD 标志置位，表示已验证）
    pub dnssec_validated: bool,
    /// 查询错误（超时、SERVFAIL 等）
    pub error: Option<String>,
    /// 该服务器此类型查询耗时（毫秒）
    pub elapsed_ms: u128,
}

/// 每个类型所有服务器的汇总结果。
#[derive(Debug, Clone)]
pub struct TypeResult {
    pub record_type: RecordType,
    pub server_results: Vec<ServerTypeResult>,
}

/// 整体查询结果。
pub struct QueryResult {
    pub domain: String,
    /// 查询目标列表（"all builtin + local" 或 单服务器描述）
    pub server_desc: String,
    pub results: Vec<TypeResult>,
    pub elapsed_ms: u128,
    pub hide_empty: bool,
    /// 是否为 IP 反查（PTR）。若为 true，domain 已转为 arpa 名。
    #[allow(dead_code)]
    pub is_ptr: bool,
}

/// 构造一个指向指定 ServerSpec 的 resolver（支持 UDP / DoT / DoH）。
/// HTTPDNS 协议不构造 resolver，返回 None。
fn build_resolver_for(spec: &ServerSpec) -> Result<Option<TokioResolver>> {
    if spec.protocol == Protocol::Httpdns {
        return Ok(None);
    }
    let ns = match spec.protocol {
        // 用纯 UDP：某些网络环境下 TCP 53 会被 DPI 重置（connection reset），
        // udp_and_tcp 的 TCP fallback 反而导致 NoConnections。
        // 截断响应（偶发，主要影响 TXT 大记录）宁可丢失也不要触发 TCP fallback。
        Protocol::Udp => {
            if spec.force_tcp {
                NameServerConfig::tcp(spec.ip)
            } else {
                NameServerConfig::udp(spec.ip)
            }
        }
        Protocol::Tls => {
            let sni: Arc<str> = spec
                .tls_server_name
                .clone()
                .unwrap_or_else(|| spec.ip.to_string())
                .into();
            NameServerConfig::tls(spec.ip, sni)
        }
        Protocol::Https => {
            let sni: Arc<str> = spec
                .tls_server_name
                .clone()
                .unwrap_or_else(|| spec.ip.to_string())
                .into();
            let path: Option<Arc<str>> = spec.https_path.as_ref().map(|p| p.as_str().into());
            NameServerConfig::https(spec.ip, sni, path)
        }
        Protocol::Quic => {
            let sni: Arc<str> = spec
                .tls_server_name
                .clone()
                .unwrap_or_else(|| spec.ip.to_string())
                .into();
            NameServerConfig::quic(spec.ip, sni)
        }
        Protocol::Httpdns => unreachable!(),
    };
    let config = ResolverConfig::from_parts(None, vec![], vec![ns]);
    let mut builder = TokioResolver::builder_with_config(config, TokioRuntimeProvider::default());
    // 每个 resolver 只配 1 个 nameserver，设 num_concurrent_reqs=1 减少内部并发，
    // 避免多 resolver 并发时 UDP socket 资源竞争导致 NoConnections。
    let opts = builder.options_mut();
    opts.num_concurrent_reqs = 1;
    opts.attempts = 1; // 不重试，超时即失败（多服务器汇总会兜底）
    let resolver = builder.build().context("构造 resolver 失败")?;
    Ok(Some(resolver))
}

/// 对给定域名的多个记录类型，向多个服务器并行查询。
///
/// `servers` 为 None 时使用所有内置服务器 + 本机 DNS。
pub async fn query_all(
    domain: &str,
    types: &[RecordType],
    servers: Option<&[ServerSpec]>,
    timeout_secs: u64,
    hide_empty: bool,
    is_ptr: bool,
    force_tcp: bool,
) -> Result<QueryResult> {
    let name: Name = domain
        .parse()
        .with_context(|| format!("无效的域名: `{domain}`"))?;

    let start = Instant::now();

    // 构造服务器列表
    let server_list: Vec<ServerSpec> = match servers {
        Some(s) => s.to_vec(),
        None => {
            // 所有内置服务器（UDP）+ 本机 DNS
            let mut list: Vec<ServerSpec> = crate::servers::BUILTIN_SERVERS
                .iter()
                .map(|s| {
                    let mut spec = ServerSpec::udp(s.name, s.ip);
                    spec.force_tcp = force_tcp;
                    spec
                })
                .collect();
            // 尝试加入本机 DNS（与内置服务器 IP 去重）
            if let Ok(local_ips) = read_local_dns() {
                let builtin_ips: std::collections::HashSet<IpAddr> =
                    crate::servers::BUILTIN_SERVERS
                        .iter()
                        .map(|s| s.ip)
                        .collect();
                for (i, ip) in local_ips.into_iter().enumerate() {
                    if builtin_ips.contains(&ip) {
                        continue; // 跳过与内置服务器重复的本机 DNS
                    }
                    let label = if i == 0 {
                        "local".to_string()
                    } else {
                        format!("local-{}", i + 1)
                    };
                    let mut spec = ServerSpec::udp(label, ip);
                    spec.force_tcp = force_tcp;
                    list.push(spec);
                }
            }
            list
        }
    };

    let total_servers = server_list.len();
    let server_desc = if servers.is_some() && total_servers == 1 {
        format!("{} (single)", server_list[0].name)
    } else if servers.is_some() {
        let names: Vec<&str> = server_list.iter().map(|s| s.name.as_str()).collect();
        names.join(", ")
    } else {
        format!("{total_servers} servers (builtin + local)")
    };

    // 两层并发：外层每服务器（全部同时开始），内层每类型。
    // 全局 Semaphore 限制并发查询总数（防止极端峰值但基本不排队）。
    let gsem = Arc::new(tokio::sync::Semaphore::new(128));

    let server_futures: Vec<_> = server_list
        .iter()
        .map(|spec| {
            let spec = spec.clone();
            let types_vec = types.to_vec();
            let n = name.clone();
            let dur = Duration::from_secs(timeout_secs);
            let gsem = gsem.clone();
            async move {
                let sdesc = spec.desc();
                let sname = spec.name.clone();

                // HTTPDNS 协议走独立客户端，不构造 hickory resolver
                if spec.protocol == Protocol::Httpdns {
                    let url = spec.httpdns_url.clone().unwrap_or_default();
                    let type_futures: Vec<_> = types_vec
                        .iter()
                        .map(|&rt| {
                            let url = url.clone();
                            let n2 = n.clone();
                            async move {
                                let t_start = Instant::now();
                                let n2_str = n2.to_string();
                                let fut = crate::httpdns::query(&url, &n2_str, rt, dur.as_secs());
                                match tokio::time::timeout(dur, fut).await {
                                    Ok(res) => ServerTypeResult {
                                        server_name: String::new(),
                                        server_desc: String::new(),
                                        records: res.records,
                                        authorities: Vec::new(),
                                        additionals: Vec::new(),
                                        response_code: String::new(),
                                        flags: String::new(),
                                        msg_size: 0,
                                        dnssec_validated: false,
                                        error: res.error,
                                        elapsed_ms: t_start.elapsed().as_millis(),
                                    },
                                    Err(_) => ServerTypeResult {
                                        server_name: String::new(),
                                        server_desc: String::new(),
                                        records: vec![],
                                        authorities: Vec::new(),
                                        additionals: Vec::new(),
                                        response_code: String::new(),
                                        flags: String::new(),
                                        msg_size: 0,
                                        dnssec_validated: false,
                                        error: Some(format!("超时 ({}s)", dur.as_secs())),
                                        elapsed_ms: dur.as_millis(),
                                    },
                                }
                            }
                        })
                        .collect();
                    let type_results = join_all(type_futures).await;
                    let filled: Vec<ServerTypeResult> = type_results
                        .into_iter()
                        .map(|mut str_res| {
                            str_res.server_name = sname.clone();
                            str_res.server_desc = sdesc.clone();
                            str_res
                        })
                        .collect();
                    return (sname, sdesc, filled);
                }

                // 非 HTTPDNS：走 hickory resolver
                let resolver = match build_resolver_for(&spec) {
                    Ok(Some(r)) => r,
                    Ok(None) => unreachable!(), // HTTPDNS 已处理
                    Err(e) => {
                        let filled: Vec<ServerTypeResult> = types_vec
                            .iter()
                            .map(|_| ServerTypeResult {
                                server_name: String::new(),
                                server_desc: String::new(),
                                records: vec![],
                                authorities: vec![],
                                additionals: vec![],
                                response_code: String::new(),
                                flags: String::new(),
                                msg_size: 0,
                                dnssec_validated: false,
                                error: Some(format!("构造 resolver 失败: {e}")),
                                elapsed_ms: 0,
                            })
                            .collect();
                        return (sname, sdesc, filled);
                    }
                };
                // 对该服务器的所有类型并发查询。用全局 Semaphore 限制并发查询总数，
                // 避免 13×28=364 个并发 UDP socket 资源竞争导致 NoConnections。
                // 关键：超时计时在获取许可后开始，避免等待许可消耗超时预算。
                let type_futures: Vec<_> = types_vec
                    .iter()
                    .map(|&rt| {
                        let r = resolver.clone();
                        let n2 = n.clone();
                        let gsem = gsem.clone();
                        async move {
                            let _permit = gsem.acquire_owned().await.unwrap();
                            let t_start = Instant::now();
                            let lookup_fut = async {
                                match r.lookup(n2.clone(), rt).await {
                                    Ok(lookup) => {
                                        let msg = lookup.message();
                                        let recs: Vec<RecordEntry> = lookup
                                            .answers()
                                            .iter()
                                            .filter(|rec| rdata_matches_type(&rec.data, rt))
                                            .map(|rec| RecordEntry {
                                                value: format_rdata(&rec.data, rt),
                                                ttl: rec.ttl,
                                            })
                                            .collect();
                                        let auths: Vec<RecordEntry> = lookup
                                            .authorities()
                                            .iter()
                                            .map(|rec| RecordEntry {
                                                value: format_rdata(&rec.data, rec.record_type()),
                                                ttl: rec.ttl,
                                            })
                                            .collect();
                                        let adds: Vec<RecordEntry> = lookup
                                            .additionals()
                                            .iter()
                                            .map(|rec| RecordEntry {
                                                value: format_rdata(&rec.data, rec.record_type()),
                                                ttl: rec.ttl,
                                            })
                                            .collect();
                                        let rcode = rcode_to_rfc(msg.response_code);
                                        let flags = msg.metadata.flags().to_string();
                                        let msg_size = msg.to_vec().map(|v| v.len()).unwrap_or(0);
                                        let dnssec_validated = flags.contains("AD");
                                        ServerTypeResult {
                                            server_name: String::new(),
                                            server_desc: String::new(),
                                            records: recs,
                                            authorities: auths,
                                            additionals: adds,
                                            response_code: rcode,
                                            flags,
                                            msg_size,
                                            dnssec_validated,
                                            error: None,
                                            elapsed_ms: t_start.elapsed().as_millis(),
                                        }
                                    }
                                    Err(e) => {
                                        let (records, error, rcode) = if e.is_no_records_found() {
                                            use hickory_net::{DnsError, NetError};
                                            let rcode = match &e {
                                                NetError::Dns(DnsError::NoRecordsFound(nr)) => {
                                                    rcode_to_rfc(nr.response_code)
                                                }
                                                _ => String::new(),
                                            };
                                            (Vec::new(), None, rcode)
                                        } else {
                                            (Vec::new(), Some(e.to_string()), String::new())
                                        };
                                        ServerTypeResult {
                                            server_name: String::new(),
                                            server_desc: String::new(),
                                            records,
                                            authorities: Vec::new(),
                                            additionals: Vec::new(),
                                            response_code: rcode,
                                            flags: String::new(),
                                            msg_size: 0,
                                            dnssec_validated: false,
                                            error,
                                            elapsed_ms: t_start.elapsed().as_millis(),
                                        }
                                    }
                                }
                            };
                            match tokio::time::timeout(dur, lookup_fut).await {
                                Ok(str_res) => str_res,
                                Err(_) => ServerTypeResult {
                                    server_name: String::new(),
                                    server_desc: String::new(),
                                    records: vec![],
                                    authorities: Vec::new(),
                                    additionals: Vec::new(),
                                    response_code: String::new(),
                                    flags: String::new(),
                                    msg_size: 0,
                                    dnssec_validated: false,
                                    error: Some(format!("超时 ({}s)", dur.as_secs())),
                                    elapsed_ms: dur.as_millis(),
                                },
                            }
                        }
                    })
                    .collect();

                let type_results = join_all(type_futures).await;
                // 填充 server_name / server_desc
                let filled: Vec<ServerTypeResult> = type_results
                    .into_iter()
                    .map(|mut str_res| {
                        str_res.server_name = sname.clone();
                        str_res.server_desc = sdesc.clone();
                        str_res
                    })
                    .collect();
                (sname, sdesc, filled)
            }
        })
        .collect();

    // 外层并发：所有服务器同时跑
    let server_outputs = join_all(server_futures).await;

    // 按类型重组：每个服务器返回的 type_results 与 types 顺序一致，直接按索引归桶。
    let mut type_map: Vec<(RecordType, Vec<ServerTypeResult>)> =
        types.iter().map(|rt| (*rt, Vec::new())).collect();
    for (_sname, _sdesc, type_results) in server_outputs {
        for (i, str_res) in type_results.into_iter().enumerate() {
            if i < type_map.len() {
                type_map[i].1.push(str_res);
            }
        }
    }

    let results: Vec<TypeResult> = type_map
        .into_iter()
        .map(|(rt, server_results)| TypeResult {
            record_type: rt,
            server_results,
        })
        .collect();

    let elapsed_ms = start.elapsed().as_millis();

    Ok(QueryResult {
        domain: domain.to_string(),
        server_desc,
        results,
        elapsed_ms,
        hide_empty,
        is_ptr,
    })
}

/// 读取本机 DNS 服务器（来自 /etc/resolv.conf）。
fn read_local_dns() -> Result<Vec<IpAddr>> {
    let mut ips = Vec::new();
    let content =
        std::fs::read_to_string("/etc/resolv.conf").context("读取 /etc/resolv.conf 失败")?;
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("nameserver") {
            let ip_str = rest.trim();
            if let Ok(ip) = ip_str.parse::<IpAddr>() {
                ips.push(ip);
            }
        }
    }
    if ips.is_empty() {
        return Err(anyhow::anyhow!("未找到 nameserver"));
    }
    Ok(ips)
}

// ───────────────────── AXFR 区传送 ─────────────────────

/// 自动查询域名的权威 NS：先查 NS 记录得到权威 NS 域名，再解析为 IP。
/// 用于 AXFR 时省略 @server 参数。
pub async fn resolve_authoritative_ns(domain: &str, timeout_secs: u64) -> Result<ServerSpec> {
    // 用系统 DNS 查 NS 记录
    let resolver = TokioResolver::builder_tokio()
        .context("读取系统 DNS 配置失败")?
        .build()
        .context("构造 resolver 失败")?;

    let name: Name = domain
        .parse()
        .with_context(|| format!("无效的域名: `{domain}`"))?;

    let dur = Duration::from_secs(timeout_secs);
    let lookup = tokio::time::timeout(dur, resolver.lookup(name.clone(), RecordType::NS))
        .await
        .context("查询 NS 记录超时")?
        .context("查询 NS 记录失败")?;

    // 提取 NS 记录中的域名
    let ns_names: Vec<String> = lookup
        .answers()
        .iter()
        .filter(|rec| rec.record_type() == RecordType::NS)
        .filter_map(|rec| match &rec.data {
            RData::NS(ns) => Some(ns.0.to_string()),
            _ => None,
        })
        .collect();

    if ns_names.is_empty() {
        anyhow::bail!("未找到 {domain} 的 NS 记录");
    }

    // 取第一个 NS 域名，解析为 IP
    let ns_domain = &ns_names[0];
    let ns_name = ns_domain.trim_end_matches('.');

    // 用系统 DNS 解析 NS 域名的 IP，优先 IPv4
    let lookup_result = tokio::time::timeout(dur, resolver.lookup_ip(ns_name))
        .await
        .context("解析权威 NS 的 IP 超时")?
        .with_context(|| format!("解析权威 NS `{ns_name}` 的 IP 失败"))?;

    // 优先 IPv4
    let ns_ip = lookup_result
        .iter()
        .find(|ip| matches!(ip, IpAddr::V4(_)))
        .or_else(|| lookup_result.iter().next())
        .ok_or_else(|| anyhow::anyhow!("权威 NS `{ns_name}` 未解析到 IP"))?;

    Ok(ServerSpec {
        name: ns_domain.clone(),
        ip: ns_ip,
        port: 53,
        protocol: Protocol::Udp,
        tls_server_name: None,
        https_path: None,
        httpdns_url: None,
        force_tcp: false,
    })
}

/// 执行 AXFR 区传送。需要指定权威 NS 服务器（TCP 连接）。
pub async fn query_axfr(domain: &str, spec: ServerSpec, timeout_secs: u64) -> Result<QueryResult> {
    use hickory_proto::rr::RecordType;

    let name: Name = domain
        .parse()
        .with_context(|| format!("无效的域名: `{domain}`"))?;
    let start = Instant::now();
    let dur = Duration::from_secs(timeout_secs);

    // AXFR 必须用 TCP。构造 TCP 连接的 NameServerConfig。
    let tcp_ns = NameServerConfig::tcp(spec.ip);
    let config = ResolverConfig::from_parts(None, vec![], vec![tcp_ns]);
    let resolver = TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
        .build()
        .context("构造 TCP resolver 失败")?;

    // 用 lookup 查 AXFR
    let result =
        match tokio::time::timeout(dur, resolver.lookup(name.clone(), RecordType::AXFR)).await {
            Ok(Ok(lookup)) => {
                let recs: Vec<RecordEntry> = lookup
                    .answers()
                    .iter()
                    .map(|rec| RecordEntry {
                        value: format_rdata(&rec.data, rec.record_type()),
                        ttl: rec.ttl,
                    })
                    .collect();
                let sr = ServerTypeResult {
                    server_name: spec.name.clone(),
                    server_desc: spec.desc(),
                    records: recs,
                    authorities: Vec::new(),
                    additionals: Vec::new(),
                    response_code: rcode_to_rfc(lookup.message().response_code),
                    flags: String::new(),
                    msg_size: 0,
                    dnssec_validated: false,
                    error: None,
                    elapsed_ms: start.elapsed().as_millis(),
                };
                TypeResult {
                    record_type: RecordType::AXFR,
                    server_results: vec![sr],
                }
            }
            Ok(Err(e)) => {
                let sr = ServerTypeResult {
                    server_name: spec.name.clone(),
                    server_desc: spec.desc(),
                    records: Vec::new(),
                    authorities: Vec::new(),
                    additionals: Vec::new(),
                    response_code: String::new(),
                    flags: String::new(),
                    msg_size: 0,
                    dnssec_validated: false,
                    error: Some(e.to_string()),
                    elapsed_ms: start.elapsed().as_millis(),
                };
                TypeResult {
                    record_type: RecordType::AXFR,
                    server_results: vec![sr],
                }
            }
            Err(_) => {
                let sr = ServerTypeResult {
                    server_name: spec.name.clone(),
                    server_desc: spec.desc(),
                    records: Vec::new(),
                    authorities: Vec::new(),
                    additionals: Vec::new(),
                    response_code: String::new(),
                    flags: String::new(),
                    msg_size: 0,
                    dnssec_validated: false,
                    error: Some(format!("超时 ({}s)", dur.as_secs())),
                    elapsed_ms: dur.as_millis(),
                };
                TypeResult {
                    record_type: RecordType::AXFR,
                    server_results: vec![sr],
                }
            }
        };

    Ok(QueryResult {
        domain: domain.to_string(),
        server_desc: spec.desc(),
        results: vec![result],
        elapsed_ms: start.elapsed().as_millis(),
        hide_empty: false,
        is_ptr: false,
    })
}

// ───────────────────── RData 格式化 ─────────────────────

/// 把 hickory 的 ResponseCode 转为 RFC 标准短码（NOERROR/NXDOMAIN/SERVFAIL 等）。
fn rcode_to_rfc(rc: hickory_proto::op::ResponseCode) -> String {
    use hickory_proto::op::ResponseCode;
    match rc {
        ResponseCode::NoError => "NOERROR",
        ResponseCode::FormErr => "FORMERR",
        ResponseCode::ServFail => "SERVFAIL",
        ResponseCode::NXDomain => "NXDOMAIN",
        ResponseCode::NotImp => "NOTIMP",
        ResponseCode::Refused => "REFUSED",
        ResponseCode::YXDomain => "YXDOMAIN",
        ResponseCode::YXRRSet => "YXRRSET",
        ResponseCode::NXRRSet => "NXRRSET",
        ResponseCode::NotAuth => "NOTAUTH",
        ResponseCode::NotZone => "NOTZONE",
        ResponseCode::BADVERS | ResponseCode::BADSIG => "BADSIG",
        ResponseCode::BADKEY => "BADKEY",
        ResponseCode::BADTIME => "BADTIME",
        ResponseCode::BADMODE => "BADMODE",
        ResponseCode::BADNAME => "BADNAME",
        _ => "UNKNOWN",
    }
    .to_string()
}

/// 判断 RData 的实际类型是否与查询的 RecordType 一致。
/// DNS 服务器返回 A 查询的 answer 时可能混入 CNAME 记录（域名→CNAME→IP 链），
/// 需过滤掉只保留与查询类型匹配的记录。
fn rdata_matches_type(rdata: &RData, rt: RecordType) -> bool {
    match (rdata, rt) {
        (RData::A(_), RecordType::A) => true,
        (RData::AAAA(_), RecordType::AAAA) => true,
        (RData::CNAME(_), RecordType::CNAME) => true,
        (RData::NS(_), RecordType::NS) => true,
        (RData::MX(_), RecordType::MX) => true,
        (RData::TXT(_), RecordType::TXT) => true,
        (RData::SOA(_), RecordType::SOA) => true,
        (RData::SRV(_), RecordType::SRV) => true,
        (RData::CAA(_), RecordType::CAA) => true,
        (RData::PTR(_), RecordType::PTR) => true,
        (RData::HINFO(_), RecordType::HINFO) => true,
        (RData::ANAME(_), RecordType::ANAME) => true,
        (RData::NAPTR(_), RecordType::NAPTR) => true,
        (RData::SSHFP(_), RecordType::SSHFP) => true,
        (RData::TLSA(_), RecordType::TLSA) => true,
        (RData::SMIMEA(_), RecordType::SMIMEA) => true,
        (RData::DNSSEC(_), _) => rt.is_dnssec(),
        // 查 A/AAAA 时，CNAME/ANAME 是链式解析的中间记录，不是最终 IP，过滤掉
        (RData::CNAME(_), RecordType::A | RecordType::AAAA) => false,
        (RData::ANAME(_), RecordType::A | RecordType::AAAA) => false,
        _ => true, // 未知类型不过滤
    }
}

/// 把 `RData` 格式化成单行可读字符串。
fn format_rdata(rdata: &RData, rt: RecordType) -> String {
    match (rdata, rt) {
        (RData::A(a), _) => a.to_string(),
        (RData::AAAA(aaaa), _) => aaaa.to_string(),
        (RData::CNAME(c), _) => c.0.to_string(),
        (RData::NS(ns), _) => ns.0.to_string(),
        (RData::PTR(p), _) => p.0.to_string(),
        (RData::ANAME(a), _) => a.0.to_string(),
        (RData::MX(mx), _) => format!("{} {}", mx.preference, mx.exchange),
        (RData::TXT(txt), _) => txt
            .txt_data
            .iter()
            .map(|seg| {
                let s = String::from_utf8_lossy(seg);
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            })
            .collect::<Vec<_>>()
            .join(" "),
        (RData::SRV(srv), _) => format!(
            "{} {} {} {}",
            srv.priority, srv.weight, srv.port, srv.target
        ),
        (RData::SOA(soa), _) => format!(
            "{} {} {} {} {} {} {}",
            soa.mname, soa.rname, soa.serial, soa.refresh, soa.retry, soa.expire, soa.minimum
        ),
        (RData::CAA(caa), _) => {
            let flag = if caa.issuer_critical { 1 } else { 0 };
            let value = String::from_utf8_lossy(&caa.value);
            format!("{} {} \"{}\"", flag, caa.tag, value)
        }
        (RData::HINFO(h), _) => {
            let cpu = String::from_utf8_lossy(&h.cpu);
            let os = String::from_utf8_lossy(&h.os);
            format!("{cpu} {os}")
        }
        (RData::NAPTR(n), _) => {
            let flags = String::from_utf8_lossy(&n.flags);
            let services = String::from_utf8_lossy(&n.services);
            let regexp = String::from_utf8_lossy(&n.regexp);
            format!(
                "{} {} \"{flags}\" \"{services}\" \"{regexp}\" {}",
                n.order, n.preference, n.replacement
            )
        }
        (RData::SSHFP(s), _) => {
            let alg: u8 = s.algorithm.into();
            let ftype: u8 = s.fingerprint_type.into();
            format!("{alg} {ftype} {}", hex(&s.fingerprint))
        }
        (RData::TLSA(t), _) => format!(
            "{:?} {:?} {:?} {}",
            t.cert_usage,
            t.selector,
            t.matching,
            hex(&t.cert_data)
        ),
        (RData::SMIMEA(s), _) => {
            let t = &s.0;
            format!(
                "{:?} {:?} {:?} {}",
                t.cert_usage,
                t.selector,
                t.matching,
                hex(&t.cert_data)
            )
        }
        (RData::DNSSEC(_), _) => rdata.to_string(),
        _ => rdata.to_string(),
    }
}

/// 把字节切片编码为十六进制小写字符串。
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{b:02x}").unwrap();
    }
    s
}
