//! dns —— 多服务器并发查询所有 DNS 记录类型并汇总的命令行工具。

mod cli;
mod config;
mod help;
mod httpdns;
mod output;
mod record_types;
mod resolver;
mod servers;
mod xyz;

use std::net::IpAddr;

use std::io::IsTerminal;

use anyhow::{Context, Result};
use colored::Colorize;

use cli::Args;
use output::OutputOpts;
use resolver::{Protocol, ServerSpec};

fn main() -> Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();

    // 拦截 -h/--help：显示中英文双语帮助（含示例）
    help::check_help(&raw_args);

    // 拦截 serve/mcp 模式 → 走 xyz-rust 的 HTTP/MCP 接口
    let cli_args: Vec<String> = raw_args[1..].to_vec();
    if xyz::try_xyz_dispatch(&cli_args) {
        return Ok(());
    }

    // 无参数且无管道输入 → 显示帮助
    if raw_args.len() <= 1 && std::io::stdin().is_terminal() {
        if help::is_chinese() {
            help::print_zh();
        } else {
            help::print_en();
        }
        return Ok(());
    }

    let args = Args::parse_cli().map_err(anyhow::Error::msg)?;

    // --list-servers
    if args.list_servers {
        servers::print_list();
        return Ok(());
    }

    // 加载配置文件，合并默认值
    let cfg = config::load();

    // 合并配置：命令行参数优先，未指定时用配置文件
    let mut server_list: Vec<String> = args.server.clone();
    if server_list.is_empty() {
        server_list = cfg.all_servers();
    }
    let region_str = args.region.clone().or(cfg.region);
    let timeout = if args.timeout == 2 {
        cfg.timeout.unwrap_or(2)
    } else {
        args.timeout
    };
    let verbose = args.verbose || cfg.verbose.unwrap_or(false);
    let ttl = args.ttl || cfg.ttl.unwrap_or(false);
    // -m 等价于 --merge --short
    let merge = args.merge;
    let short = args.short || merge;

    // @server 语法：从位置参数提取，合并到 server_list
    server_list.extend(cli::extract_at_server(&args.args));

    // 解析位置参数（可能只有类型没域名，域名从 stdin/文件补）
    let mut targets = args.query_targets().map_err(anyhow::Error::msg)?;

    // 从文件或管道 stdin 读取域名（自动检测：stdin 非 TTY 时自动读取）
    let stdin_content = if let Some(file) = &args.file {
        if file == "-" {
            // 显式 --file -
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Some(buf)
        } else {
            Some(std::fs::read_to_string(file).with_context(|| format!("读取文件失败: `{file}`"))?)
        }
    } else if !std::io::stdin().is_terminal() {
        // 自动检测：stdin 是管道时自动读取
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        if buf.trim().is_empty() {
            None
        } else {
            Some(buf)
        }
    } else {
        None
    };

    if let Some(content) = stdin_content {
        // 从位置参数提取类型（如果有），否则用 ALL_TYPES
        let (file_types, file_hide) = if !targets.is_empty() {
            // 位置参数有域名+类型
            (targets[0].types.clone(), targets[0].hide_empty)
        } else {
            // 位置参数只有类型没域名，从 args 里解析类型
            let type_args: Vec<&str> = args
                .args
                .iter()
                .filter(|a| !a.starts_with('@') && crate::record_types::is_type_keyword(a))
                .map(|s| s.as_str())
                .collect();
            let mut types = Vec::new();
            for kw in &type_args {
                // resolve_types 是私有函数，用 ALL_TYPES 或单类型
                let upper = kw.to_ascii_uppercase();
                if upper == "ANY" || upper == "*" {
                    types.extend_from_slice(crate::record_types::ALL_TYPES);
                } else {
                    // 查 single_type_slice 表
                    if let Some(t) = cli::single_type_slice(kw) {
                        types.push(*t);
                    }
                }
            }
            if types.is_empty() {
                types = crate::record_types::ALL_TYPES.to_vec();
            }
            types.sort();
            types.dedup();
            (types, true)
        };
        // 清除位置参数产生的 targets（只保留类型信息），用 stdin 域名重建
        targets.clear();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            targets.push(cli::QueryTarget {
                domain: idna::domain_to_ascii(line).unwrap_or_else(|_| line.to_string()),
                display_domain: line.to_string(),
                types: file_types.clone(),
                hide_empty: file_hide,
                is_ptr: false,
                is_axfr: false,
            });
        }
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // 确定服务器列表
    let force_tcp = args.tcp;
    let servers: Option<Vec<ServerSpec>> = if !server_list.is_empty() {
        let mut specs = Vec::new();
        for s in &server_list {
            let mut spec = resolve_server_spec(s, &rt)?;
            spec.force_tcp = force_tcp;
            if force_tcp && spec.protocol == Protocol::Udp {
                spec.name = spec.name.replace("udp:", "tcp:");
            }
            specs.push(spec);
        }
        Some(specs)
    } else if let Some(r) = &region_str {
        let region =
            servers::parse_region(r).context(format!("无效的 region: `{r}`（可选 global/cn"))?;
        Some(
            servers::servers_by_region(Some(region))
                .into_iter()
                .map(|s| {
                    let mut spec = ServerSpec::udp(s.name, s.ip);
                    spec.force_tcp = force_tcp;
                    spec
                })
                .collect(),
        )
    } else {
        None
    };

    let servers_ref = servers.as_deref();

    // AXFR 特殊处理
    let is_axfr = targets.iter().any(|t| t.is_axfr);
    if is_axfr {
        for target in &targets {
            // 确定权威 NS：用户指定 > 自动查询域名的 NS 记录
            let ns_spec = if let Some(servers) = &servers {
                if !servers.is_empty() {
                    servers[0].clone()
                } else {
                    anyhow::bail!("AXFR 需要指定权威 NS 服务器，用法: dns axfr <域名> @<权威NS>")
                }
            } else {
                // 自动查询域名的 NS 记录，获取权威 NS
                eprintln!("正在查询 {} 的权威 NS...", target.display_domain);
                let ns_spec =
                    rt.block_on(resolver::resolve_authoritative_ns(&target.domain, timeout))?;
                eprintln!("权威 NS: {} ({})", ns_spec.name, ns_spec.desc());
                ns_spec
            };

            let result = rt.block_on(resolver::query_axfr(&target.domain, ns_spec, timeout))?;
            let opts = OutputOpts {
                json: args.json,
                verbose,
                ttl,
                short: short,
                color: args.color,
                yaml: args.yaml,
                csv: args.csv,
            };
            output::print(&result, &opts);
        }
        return Ok(());
    }

    // 多域名循环查询
    let multi = targets.len() > 1;

    // --merge 模式：合并多域名的同类型记录
    if multi && merge {
        let mut all_results = Vec::new();
        for target in &targets {
            let result = rt.block_on(resolver::query_all(
                &target.domain,
                &target.types,
                servers_ref,
                timeout,
                target.hide_empty,
                target.is_ptr,
                force_tcp,
            ))?;
            let mut result = result;
            result.domain = target.display_domain.clone();
            all_results.push(result);
        }
        let opts = OutputOpts {
            json: args.json,
            yaml: args.yaml,
            csv: args.csv,
            verbose,
            ttl,
            short: short,
            color: args.color,
        };
        output::print_merged(&all_results, &opts);
        return Ok(());
    }

    for (idx, target) in targets.iter().enumerate() {
        let result = rt.block_on(resolver::query_all(
            &target.domain,
            &target.types,
            servers_ref,
            timeout,
            target.hide_empty,
            target.is_ptr,
            force_tcp,
        ))?;

        let mut result = result;
        result.domain = target.display_domain.clone();

        let opts = OutputOpts {
            json: args.json,
            yaml: args.yaml,
            csv: args.csv,
            verbose,
            ttl,
            short: short,
            color: args.color,
        };

        // 多域名时加明显分隔（CSV/JSON/YAML/short 不加，保持纯数据）
        if multi && !args.json && !args.yaml && !short && !args.csv {
            let use_color = match args.color {
                cli::ColorMode::Always => true,
                cli::ColorMode::Never => false,
                cli::ColorMode::Auto => std::io::stdout().is_terminal(),
            };
            colored::control::set_override(use_color);
            // 非第一个域名前加空行
            if idx > 0 {
                println!();
            }
            // 每个域名都显示标题 + 分隔线
            println!("{}", target.display_domain.cyan().bold());
            println!("{}", "─".repeat(50).bright_black());
        }

        output::print(&result, &opts);
    }

    Ok(())
}

/// 解析 `--server` 参数，支持多种格式。域名会先解析为 IP。
fn resolve_server_spec(s: &str, rt: &tokio::runtime::Runtime) -> Result<ServerSpec> {
    let s = s.trim();

    if s.starts_with("http://") || s.starts_with("https://") {
        return Ok(ServerSpec::httpdns(format!("httpdns:{s}"), s));
    }
    if let Some(rest) = s
        .strip_prefix("tls://")
        .or_else(|| s.strip_prefix("dot://"))
    {
        return parse_tls(rest);
    }
    if let Some(rest) = s
        .strip_prefix("quic://")
        .or_else(|| s.strip_prefix("doq://"))
    {
        let (host_port, query) = rest.split_once('?').unwrap_or((rest, ""));
        let host_port = host_port.split('/').next().unwrap_or(host_port);
        let (ip, port) = parse_host_port(host_port, 853)?;
        let sn = parse_query_param(query, "sn");
        return Ok(ServerSpec {
            name: format!("quic:{ip}:{port}"),
            ip,
            port,
            protocol: Protocol::Quic,
            tls_server_name: sn,
            https_path: None,
            httpdns_url: None,
            force_tcp: false,
        });
    }
    if let Some(rest) = s.strip_prefix("doh://") {
        let url = format!("https://{rest}");
        return Ok(ServerSpec::httpdns(format!("httpdns:{url}"), url));
    }

    // 去掉可能存在的 :port 后缀
    let core = s.split(['/', '?']).next().unwrap_or(s);
    let (host, port_opt) = if let Some((h, p)) = core.rsplit_once(':') {
        if let Ok(port) = p.parse::<u16>() {
            (h.to_string(), Some(port))
        } else {
            (core.to_string(), None)
        }
    } else {
        (core.to_string(), None)
    };

    // 先尝试当作 IP
    if let Ok(ip) = host.parse::<IpAddr>() {
        let port = port_opt.unwrap_or(53);
        return Ok(ServerSpec {
            name: format!("udp:{ip}:{port}"),
            ip,
            port,
            protocol: Protocol::Udp,
            tls_server_name: None,
            https_path: None,
            httpdns_url: None,
            force_tcp: false,
        });
    }

    // 当作域名：用系统 DNS 解析为 IP
    let host_clone = host.clone();
    let port = port_opt.unwrap_or(53);
    let ips = rt
        .block_on(async { tokio::net::lookup_host((host_clone.as_str(), port)).await })
        .with_context(|| format!("无法解析服务器域名: `{host}`"))?;
    let ip = ips
        .into_iter()
        .next()
        .map(|sa| sa.ip())
        .ok_or_else(|| anyhow::anyhow!("域名 `{host}` 未解析到 IP"))?;

    Ok(ServerSpec {
        name: format!("udp:{host}:{port}"),
        ip,
        port,
        protocol: Protocol::Udp,
        tls_server_name: None,
        https_path: None,
        httpdns_url: None,
        force_tcp: false,
    })
}

fn parse_tls(rest: &str) -> Result<ServerSpec> {
    let (host_port, query) = rest.split_once('?').unwrap_or((rest, ""));
    let host_port = host_port.split('/').next().unwrap_or(host_port);
    let (ip, port) = parse_host_port(host_port, 853)?;
    let sn = parse_query_param(query, "sn");
    Ok(ServerSpec {
        name: format!("tls:{ip}:{port}"),
        ip,
        port,
        protocol: Protocol::Tls,
        tls_server_name: sn,
        https_path: None,
        httpdns_url: None,
        force_tcp: false,
    })
}

fn parse_host_port(s: &str, default_port: u16) -> Result<(IpAddr, u16)> {
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Ok((ip, default_port));
    }
    if let Some((ip_str, port_str)) = s.rsplit_once(':') {
        let ip: IpAddr = ip_str
            .parse()
            .with_context(|| format!("无效的 IP 地址: `{ip_str}`"))?;
        let port: u16 = port_str
            .parse()
            .with_context(|| format!("无效的端口: `{port_str}`"))?;
        return Ok((ip, port));
    }
    anyhow::bail!("无效的服务器地址: `{s}`")
}

fn parse_query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=')
            && k == key
        {
            return Some(v.to_string());
        }
    }
    None
}
