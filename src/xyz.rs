//! xyz-rust 集成：提供 HTTP REST API 和 MCP 工具服务器。
//!
//! 当用户运行 `dns serve` 或 `dns mcp` 时，走 xyz-rust 的三种接口。
//! 其他所有命令保持原有 dns 查询行为不变。

use serde::Serialize;
use xyz_rust::errs;
use xyz_rust::{CliFieldHint, CliHints, Ctx, HTTPHints, MCPHints, XyzArgs, XyzOutput, define};

use crate::resolver;

/// 检查是否是 serve/mcp 模式，如果是则走 xyz-rust 并返回 true。
pub fn try_xyz_dispatch(args: &[String]) -> bool {
    if args.is_empty() {
        return false;
    }
    let first = args[0].as_str();
    if first != "serve" && first != "mcp" {
        return false;
    }

    // 走 xyz-rust 的 define...run() 链
    define("dns.query", query)
        .summary("Query DNS records for a domain")
        .description("Query all or specific DNS record types from multiple servers. Returns structured results with TTL, response codes, and server details.")
        .cli(CliHints {
            usage: "query <domain>".into(),
            fields: [
                ("record_type".to_string(), CliFieldHint { shorthand: Some("t".to_string()), ..Default::default() }),
                ("server".to_string(), CliFieldHint { shorthand: Some("s".to_string()), ..Default::default() }),
                ("timeout".to_string(), CliFieldHint { shorthand: Some("T".to_string()), ..Default::default() }),
            ].into_iter().collect(),
            ..Default::default()
        })
        .http(HTTPHints { method: "GET".into(), path: "/dns/query".into(), ..Default::default() })
        .mcp(MCPHints { annotations: vec!["read".into(), "title:DNS Query".into()], ..Default::default() })
        .also(&[
            &define("dns.lookup", lookup)
                .summary("Fast single-type DNS lookup")
                .description("Query a specific record type and return pure values (IPs, domains, etc.). Ideal for pipelines and automation.")
                .cli(CliHints {
                    usage: "lookup <domain>".into(),
                    fields: [
                        ("record_type".to_string(), CliFieldHint { shorthand: Some("t".to_string()), ..Default::default() }),
                        ("server".to_string(), CliFieldHint { shorthand: Some("s".to_string()), ..Default::default() }),
                        ("timeout".to_string(), CliFieldHint { shorthand: Some("T".to_string()), ..Default::default() }),
                    ].into_iter().collect(),
                    ..Default::default()
                })
                .http(HTTPHints { method: "GET".into(), path: "/dns/lookup".into(), ..Default::default() })
                .mcp(MCPHints { annotations: vec!["read".into(), "title:DNS Lookup".into()], ..Default::default() }),
            &define("dns.axfr", axfr)
                .summary("AXFR zone transfer")
                .description("Perform an AXFR zone transfer. Automatically resolves authoritative NS if not specified.")
                .cli(CliHints {
                    usage: "axfr <domain>".into(),
                    fields: [
                        ("server".to_string(), CliFieldHint { shorthand: Some("s".to_string()), ..Default::default() }),
                        ("timeout".to_string(), CliFieldHint { shorthand: Some("T".to_string()), ..Default::default() }),
                    ].into_iter().collect(),
                    ..Default::default()
                })
                .http(HTTPHints { method: "POST".into(), path: "/dns/axfr".into(), ..Default::default() })
                .mcp(MCPHints { annotations: vec!["read".into(), "title:DNS AXFR".into()], ..Default::default() }),
        ])
        .run();
    #[allow(unreachable_code)]
    true
}

// ---- 参数结构体 ----

#[derive(XyzArgs)]
struct QueryArgs {
    #[xyz(
        desc = "Domain name to query",
        required,
        validate = "min=3",
        cli = "positional",
        http = "query"
    )]
    domain: String,
    #[xyz(
        desc = "Record type (A, AAAA, MX, NS, TXT, etc.). Omit for all types",
        http = "query"
    )]
    record_type: String,
    #[xyz(desc = "DNS server (IP, domain, tls://, https://)", http = "query")]
    server: String,
    #[xyz(desc = "Query timeout in seconds", default = "5", http = "query")]
    timeout: i64,
}

#[derive(Serialize, XyzOutput)]
struct QueryResult {
    domain: String,
    server_desc: String,
    elapsed_ms: u64,
    entries: Vec<QueryEntry>,
}

#[derive(Serialize, XyzOutput)]
struct QueryEntry {
    #[serde(rename = "type")]
    rtype: String,
    servers: Vec<QueryServer>,
}

#[derive(Serialize, XyzOutput)]
struct QueryServer {
    server: String,
    desc: String,
    records: Vec<QueryRecord>,
    response_code: String,
    elapsed_ms: u64,
}

#[derive(Serialize, XyzOutput)]
struct QueryRecord {
    value: String,
    ttl: u32,
}

#[derive(XyzArgs)]
struct LookupArgs {
    #[xyz(
        desc = "Domain name",
        required,
        validate = "min=3",
        cli = "positional",
        http = "query"
    )]
    domain: String,
    #[xyz(desc = "Record type (A, AAAA, MX, etc.)", required, http = "query")]
    record_type: String,
    #[xyz(desc = "DNS server", http = "query")]
    server: String,
    #[xyz(desc = "Timeout in seconds", default = "5", http = "query")]
    timeout: i64,
}

#[derive(Serialize, XyzOutput)]
struct LookupResult {
    domain: String,
    records: Vec<String>,
}

#[derive(XyzArgs)]
struct AxfrArgs {
    #[xyz(
        desc = "Domain to transfer",
        required,
        validate = "min=3",
        cli = "positional"
    )]
    domain: String,
    #[xyz(desc = "Authoritative NS server", http = "query")]
    server: String,
    #[xyz(desc = "Timeout in seconds", default = "10", http = "query")]
    timeout: i64,
}

// ---- handlers ----

fn query(_ctx: &Ctx, in_: &QueryArgs) -> errs::Result<QueryResult> {
    let types = if in_.record_type.is_empty() {
        crate::record_types::ALL_TYPES.to_vec()
    } else {
        vec![parse_record_type(&in_.record_type)]
    };

    let server_spec = if in_.server.is_empty() {
        None
    } else {
        Some(crate::resolver::ServerSpec::udp(
            "custom",
            in_.server
                .parse()
                .map_err(|_| errs::new(errs::Kind::InvalidInput, "invalid server IP"))?,
        ))
    };

    let domain = in_.domain.clone();
    let timeout = in_.timeout as u64;
    let server_ref = server_spec.map(|s| s);

    let result = run_async(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let spec_slice = server_ref.as_ref().map(|s| std::slice::from_ref(s));
        rt.block_on(resolver::query_all(
            &domain, &types, spec_slice, timeout, true, false, false,
        ))
    })
    .map_err(|e| errs::new(errs::Kind::Internal, e.to_string()))?;

    Ok(to_query_result(result))
}

fn lookup(_ctx: &Ctx, in_: &LookupArgs) -> errs::Result<LookupResult> {
    let types = vec![parse_record_type(&in_.record_type)];

    let server_spec = if in_.server.is_empty() {
        None
    } else {
        Some(crate::resolver::ServerSpec::udp(
            "custom",
            in_.server
                .parse()
                .map_err(|_| errs::new(errs::Kind::InvalidInput, "invalid server IP"))?,
        ))
    };

    let domain = in_.domain.clone();
    let timeout = in_.timeout as u64;
    let server_ref = server_spec;

    let result = run_async(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let spec_slice = server_ref.as_ref().map(|s| std::slice::from_ref(s));
        rt.block_on(resolver::query_all(
            &domain, &types, spec_slice, timeout, false, false, false,
        ))
    })
    .map_err(|e| errs::new(errs::Kind::Internal, e.to_string()))?;

    // 提取纯值
    let mut records = Vec::new();
    for tr in &result.results {
        for sr in &tr.server_results {
            for rec in &sr.records {
                records.push(rec.value.clone());
            }
        }
    }

    Ok(LookupResult {
        domain: in_.domain.clone(),
        records,
    })
}

fn axfr(_ctx: &Ctx, in_: &AxfrArgs) -> errs::Result<QueryResult> {
    let domain = in_.domain.clone();
    let timeout = in_.timeout as u64;
    let server = in_.server.clone();

    let result = run_async(move || -> anyhow::Result<resolver::QueryResult> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        // AXFR 需要权威 NS
        let ns_spec = if server.is_empty() {
            rt.block_on(resolver::resolve_authoritative_ns(&domain, timeout))?
        } else {
            crate::resolver::ServerSpec::udp("custom", server.parse()?)
        };

        Ok(rt.block_on(resolver::query_axfr(&domain, ns_spec, timeout))?)
    })
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not found") || msg.contains("NotFound") {
            errs::new(errs::Kind::NotFound, msg)
        } else if msg.contains("invalid") || msg.contains("InvalidInput") {
            errs::new(errs::Kind::InvalidInput, msg)
        } else {
            errs::new(errs::Kind::Internal, msg)
        }
    })?;

    Ok(to_query_result(result))
}

// ---- helpers ----

/// 在独立线程里运行 async 代码，避免 tokio runtime 嵌套问题。
/// xyz-rust 的 handler 是同步的，但我们的查询逻辑是 async 的。
fn run_async<F, T>(f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = f();
        let _ = tx.send(result);
    });
    rx.recv()
        .map_err(|e| anyhow::anyhow!("thread panicked: {}", e))?
}

fn parse_record_type(s: &str) -> hickory_proto::rr::RecordType {
    use hickory_proto::rr::RecordType;
    let upper = s.to_ascii_uppercase();
    match upper.as_str() {
        "ANY" | "*" => return RecordType::ANY,
        _ => {}
    }
    // 从 ALL_TYPES 表里找
    for &rt in crate::record_types::ALL_TYPES {
        if rt.to_string() == upper {
            return rt;
        }
    }
    RecordType::A
}

fn to_query_result(result: resolver::QueryResult) -> QueryResult {
    let mut entries = Vec::new();
    for tr in result.results {
        let mut servers = Vec::new();
        for sr in tr.server_results {
            servers.push(QueryServer {
                server: sr.server_name,
                desc: sr.server_desc,
                records: sr
                    .records
                    .into_iter()
                    .map(|r| QueryRecord {
                        value: r.value,
                        ttl: r.ttl,
                    })
                    .collect(),
                response_code: sr.response_code,
                elapsed_ms: sr.elapsed_ms as u64,
            });
        }
        entries.push(QueryEntry {
            rtype: tr.record_type.to_string(),
            servers,
        });
    }
    QueryResult {
        domain: result.domain,
        server_desc: result.server_desc,
        elapsed_ms: result.elapsed_ms as u64,
        entries,
    }
}
