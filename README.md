# dns

[English](README_EN.md) | 中文

v0.1.0 — 向多个 DNS 服务器并发查询所有记录类型并汇总的命令行工具，用 Rust 编写。

仓库：https://github.com/ejfkdev/dns

与 `dig`、`dog` 等工具不同，`dns` **默认向所有内置 DNS 服务器 + 本机 DNS 并发查询所有记录类型**，汇总去重后展示——可直观看出不同服务器返回结果的差异（DNS 分流、地理差异、污染等）。

## 示例

<details>
<summary><code>dns example.com</code> — 默认查询（多服务器汇总去重，TTY 彩色）</summary>

```text
$ dns example.com

A
  172.66.147.243
  104.20.23.154

AAAA
  2606:4700:10::ac42:93f3
  2606:4700:10::6814:179a

MX
  0 .

NS
  elliott.ns.cloudflare.com.
  hera.ns.cloudflare.com.

TXT
  "v=spf1 -all"
  "_k2n1y4vw3qtb4skdx9e7dxt97qrmmq9"

SOA
  elliott.ns.cloudflare.com. dns.cloudflare.com. 2410849323 10000 2400 604800 1800

DNSKEY
  257 3 13 mdsswUyr3DPW132mOi8V9xESWE8jTo0dxCjjnopKl+GqJxpVXckHAeF+KkxLbxILfDLUT0rAK9iUzy1L53eKGQ==
  256 3 13 oJMRESz5E4gYzS/q6XDrvU1qMPYIjCWzJaOau8XNEZeqCYKD5ar0IRd8KqXXFJkqmVfRvMGPmM1x8fGAa2XhSA==

HTTPS
  1 . alpn=h2, ipv4hint=104.20.23.154,172.66.147.243, ipv6hint=2606:4700:10::6814:179a,2606:4700:10::ac42:93f3,

共 19 条记录，10 个类型，14 个 DNS 服务器
```

</details>

<details>
<summary><code>dns example.com -v</code> — 详细模式（TTL、标志位、报文大小）</summary>

```text
$ dns example.com --server 8.8.8.8 -v

查询: example.com  服务器: udp:8.8.8.8:53 (single)  耗时: 416ms
A
  172.66.147.243 (ttl=300)
  104.20.23.154 (ttl=300)
  udp:8.8.8.8:53 flags=RD,RA size=72B

AAAA
  2606:4700:10::ac42:93f3 (ttl=300)
  2606:4700:10::6814:179a (ttl=300)
  udp:8.8.8.8:53 flags=RD,RA size=96B

MX
  0 . (ttl=280)
  udp:8.8.8.8:53 flags=RD,RA size=55B

NS
  hera.ns.cloudflare.com. (ttl=21600)
  elliott.ns.cloudflare.com. (ttl=21600)
  udp:8.8.8.8:53 flags=RD,RA size=95B

TXT
  "v=spf1 -all" (ttl=300)
  "_k2n1y4vw3qtb4skdx9e7dxt97qrmmq9" (ttl=300)
  udp:8.8.8.8:53 flags=RD,RA size=109B

SOA
  elliott.ns.cloudflare.com. dns.cloudflare.com. 2410849323 10000 2400 604800 1800 (ttl=1224)
  udp:8.8.8.8:53 flags=RD,RA size=102B
```

</details>

<details>
<summary><code>dns example.com github.com --merge</code> — 多域名合并查询</summary>

```text
$ dns example.com github.com --server 8.8.8.8 --merge

A
  104.20.23.154
  172.66.147.243
  20.205.243.166

AAAA
  2606:4700:10::ac42:93f3
  2606:4700:10::6814:179a

CAA
  0 issue "digicert.com"
  0 issue "letsencrypt.org"

MX
  0 .

NS
  elliott.ns.cloudflare.com.
  hera.ns.cloudflare.com.
```

</details>

<details>
<summary><code>cat domains.txt | dns A --merge</code> — 从管道读取域名</summary>

```text
$ cat domains.txt
example.com
github.com

$ cat domains.txt | dns A --merge

A
  172.66.147.243
  104.20.23.154
  20.205.243.166
```

</details>

## 安装

```sh
cargo build --release
cp target/release/dns ~/.local/bin/   # 或加入 PATH 的任意目录
```

## 常用命令

```sh
dns example.com                    # 向所有内置服务器查询全部记录类型
dns any example.com                # 查询 RFC 全量类型，显示全部（含无记录与错误）
dns mx example.com                 # 只查 MX 记录
dns A CNAME example.com            # 查多个指定类型
dns A AAAA example.com github.com  # 多类型 + 多域名
dns example.com github.com         # 批量查询多个域名
dns A AAAA a.com b.com @8.8.8.8 @1.1.1.1  # 多类型 + 多域名 + 多服务器
dns @8.8.8.8 example.com           # 指定 DNS 服务器（@ 语法）
dns 8.8.8.8                        # IP 反查 PTR
dns 中文.com                        # 中文域名自动 Punycode 转码
dns axfr example.com               # AXFR 区传送（自动获取权威 NS）
dns example.com --region cn        # 只用中国 DNS 服务器
dns example.com -v                 # 详细模式：耗时、一致数、状态码、标志位
dns example.com --ttl              # 显示 TTL
dns example.com --json             # JSON 输出（便于 | jq）
dns example.com --short            # 极简输出（仅值，便于 | xargs）
dns a.com b.com --merge            # 合并多域名结果（同类型合并展示）
cat domains.txt | dns A --merge    # 从管道读取域名 + 合并
cat domains.txt | dns --csv        # 从管道读取域名 + CSV 输出
cat domains.txt | dns --json       # 从管道读取域名 + JSON 输出
```

### 特殊关键字（与 RFC 记录类型区分）

| 关键字 | 说明 |
|--------|------|
| `any` | 查询全部 RFC 记录类型（显示所有，含无记录与错误） |
| `axfr` | AXFR 区传送（自动获取域名权威 NS，需授权） |

### RFC 记录类型

可任意组合，如 `dns A AAAA MX example.com`：

```
A AAAA CNAME MX NS TXT SOA SRV CAA PTR HINFO
DNSKEY DS RRSIG NSEC TLSA SVCB HTTPS ... 全部 RFC 类型
```

## 内置 DNS 服务器

默认向以下所有服务器 + 本机 DNS 并发查询。用 `--region global` 或 `--region cn` 筛选子集。

| Region | 名称 | IP | 说明 |
|--------|------|-----|------|
| global | google | 8.8.8.8 / 8.8.4.4 | Google Public DNS |
| global | cloudflare | 1.1.1.1 / 1.0.0.1 | Cloudflare DNS |
| global | quad9 | 9.9.9.9 | Quad9（隐私+安全）|
| global | opendns | 208.67.222.222 | OpenDNS (Cisco) |
| global | verisign | 64.6.64.6 | Verisign |
| cn | alidns | 223.5.5.5 / 223.6.6.6 | 阿里 AliDNS |
| cn | tencent | 119.29.29.29 / 119.28.28.28 | 腾讯 DNSPod |
| cn | baidu | 180.76.76.76 | 百度 BaiduDNS |
| cn | 114dns | 114.114.114.114 | 114DNS |

`dns --list-servers` 查看完整清单。

## `--server` 协议格式

`--server` 和 `@server` 支持通过写法指定 DNS 协议，可多个：

| 写法 | 协议 | 说明 |
|------|------|------|
| `8.8.8.8` | UDP | 普通 DNS，默认端口 53 |
| `8.8.8.8:5353` | UDP | 指定端口 |
| `a.gtld-servers.net` | UDP | 域名（自动解析为 IP）|
| `tls://1.1.1.1` | DoT | DNS-over-TLS，默认 853 |
| `tls://1.1.1.1?sn=cloudflare-dns.com` | DoT | 指定 SNI |
| `quic://1.1.1.1?sn=cloudflare-dns.com` | DoQ | DNS-over-QUIC，默认 853 |
| `https://doh.pub/dns-query` | DoH | 标准 DoH（RFC 8444）|
| `http://119.29.29.29/d` | HTTPDNS | 腾讯私有格式（自动适配）|
| `doh://8.8.8.8` | DoH | https:// 的别名 |
| `dot://1.1.1.1` | DoT | tls:// 的别名 |

```sh
dns example.com --server tls://1.1.1.1?sn=cloudflare-dns.com   # Cloudflare DoT
dns example.com @quic://1.1.1.1                                 # Cloudflare DoQ
dns example.com --server https://doh.pub/dns-query              # 腾讯 DoH
dns example.com --server http://119.29.29.29/d                  # 腾讯私有 HTTPDNS
```

省略 `--server` 时用内置服务器。`--tcp` 强制 TCP 查询（默认 UDP）。

### 智能兼容（HTTPDNS）

`http://` 或 `https://` URL 自动依次尝试多种格式：

1. **标准 DoH JSON**（RFC 8444）— 覆盖 Cloudflare、Google、腾讯 doh.pub、阿里 dns.alidns.com
2. **腾讯私有 HTTPDNS** — 覆盖 119.29.29.29 等私有端点（仅 A/AAAA）

一种格式失败自动切换，用户只需写厂商给出的访问地址。

## 输出格式

根据 stdout 是否为终端自动切换：

- **交互式终端（TTY）**：彩色分组，IP 绿色、域名黄色，差异值 magenta 加粗
- **管道 / 重定向（非 TTY）**：纯文本，`类型: 值` 每行一条，便于 `grep`
- **`--json`**：完整多服务器明细，含状态码、标志位
- **`--yaml`**：YAML 格式
- **`--csv`**：CSV 格式（`domain,type,value,ttl`）
- **`--short`**：极简输出，仅值

### verbose 模式（-v）

显示：查询耗时、一致数 `(N/M)`、响应状态码（NOERROR/NXDOMAIN/SERVFAIL 等 RFC 标准）、
DNS 标志位（RD/RA/AA/AD/TC）、响应报文大小、DNSSEC 验证结果（AD 标志）、
Authority/Additional 段。

```sh
$ dns example.com -v --server 8.8.8.8
# example.com @ udp:8.8.8.8:53 (single) (273ms)
A: 172.66.147.243 (ttl=300)
A: 104.20.23.154 (ttl=300)
A: flags=RD,RA size=72B
AAAA: 2606:4700:10::6814:179a (ttl=300)
...
```

### 多域名合并（--merge）

```sh
$ dns example.com github.com --merge
A 172.66.147.243
A 104.20.23.154
A 20.205.243.166
AAAA 2606:4700:10::6814:179a
```

### 从文件或管道读取域名

```sh
dns --file domains.txt               # 从文件读取（每行一个域名，# 注释跳过）
dns --file -                         # 从 stdin 读取
cat domains.txt | dns A --merge      # 自动检测 stdin 管道
echo example.com | dns MX            # 单域名管道
```

## 配置文件

路径（系统标准）：
- Linux: `~/.config/ejfkdev/dns/config.toml`
- macOS: `~/Library/Application Support/ejfkdev/dns/config.toml`
- Windows: `%APPDATA%\ejfkdev\dns\config.toml`

```toml
# 默认 DNS 服务器（单个）
server = "8.8.8.8"

# 或多个服务器（数组形式）
servers = [
  "8.8.8.8",
  "1.1.1.1",
  "tls://1.1.1.1?sn=cloudflare-dns.com",  # 支持 DoT/DoH/DoQ
  "https://doh.pub/dns-query",
]

# server 和 servers 可同时使用，会合并

region = "cn"       # 筛选内置服务器：global / cn（省略时用全部）
timeout = 3          # 查询超时秒数（默认 2）
color = "auto"        # 颜色：auto / always / never（默认 auto）
ttl = true            # 显示 TTL（默认 false）
verbose = false       # 详细输出（默认 false）
```

命令行参数优先于配置文件。未设置的选项回退到配置文件，再回退到默认值。

## 帮助语言

自动检测系统语言（中文/英文），`dns -h` 或直接运行 `dns` 显示对应语言的帮助。

## 依赖

- [hickory-resolver](https://crates.io/crates/hickory-resolver) — DNS 解析（UDP/TCP/DoT/DoH/DoQ）
- [clap](https://crates.io/crates/clap) — 命令行参数
- [colored](https://crates.io/crates/colored) — 终端彩色
- [tokio](https://crates.io/crates/tokio) — 异步运行时
- [reqwest](https://crates.io/crates/reqwest) — HTTPDNS 客户端
- [idna](https://crates.io/crates/idna) — Punycode 转码
- [dirs](https://crates.io/crates/dirs) — 系统标准路径
- [toml](https://crates.io/crates/toml) — 配置文件解析

## 下载预编译二进制

从 [Releases](https://github.com/ejfkdev/dns/releases) 下载对应平台的二进制：

| 平台 | 文件 |
|------|------|
| Linux x86_64 | `dns-v*-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `dns-v*-aarch64-unknown-linux-gnu.tar.gz` |
| macOS x86_64 | `dns-v*-x86_64-apple-darwin.tar.gz` |
| macOS arm64 (Apple Silicon) | `dns-v*-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `dns-v*-x86_64-pc-windows-msvc.zip` |

二进制已 strip 符号 + UPX 压缩，体积最小化。

打 tag（如 `v0.2.0`）自动触发 CI 构建并发布 Release。

## 从源码编译

```sh
git clone https://github.com/ejfkdev/dns.git
cd dns
cargo build --release
cp target/release/dns ~/.local/bin/
```

Release profile 已配置 LTO + strip + opt-level=z 优化体积。

## License

MIT
