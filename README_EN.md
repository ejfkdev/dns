# dns

English | [中文](README.md)

A command-line DNS tool that queries all record types from multiple servers concurrently, written in Rust.

Repo: https://github.com/ejfkdev/dns

Unlike `dig` or `dog`, `dns` **queries all built-in DNS servers + local DNS concurrently for all record types by default**, deduplicates and displays results — making it easy to spot differences across servers (DNS splitting, geo-routing, pollution, etc.).

## Examples

<details>
<summary><code>dns example.com</code> — Default query (multi-server, TTY colored)</summary>

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

19 records, 10 types, 14 DNS servers
```

</details>

<details>
<summary><code>dns example.com -v</code> — Verbose mode (TTL, flags, message size)</summary>

```text
$ dns example.com --server 8.8.8.8 -v

Query: example.com  Server: udp:8.8.8.8:53 (single)  Elapsed: 416ms
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
<summary><code>dns example.com github.com --merge</code> — Multi-domain merge</summary>

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
<summary><code>cat domains.txt | dns A --merge</code> — Read domains from pipe</summary>

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

## Install

```sh
cargo build --release
cp target/release/dns ~/.local/bin/   # or any PATH directory
```

## Common Commands

```sh
dns example.com                    # Query all record types from all built-in servers
dns any example.com                # Query all RFC types, show everything (incl. empty/errors)
dns mx example.com                 # Query only MX records
dns A CNAME example.com            # Query multiple record types
dns A AAAA example.com github.com  # Multiple types + multiple domains
dns example.com github.com         # Batch query multiple domains
dns A AAAA a.com b.com @8.8.8.8 @1.1.1.1  # Multi-type + multi-domain + multi-server
dns @8.8.8.8 example.com           # Specify DNS server (@ syntax)
dns 8.8.8.8                        # Reverse lookup (PTR)
dns 中文.com                        # IDN Punycode auto-conversion
dns axfr example.com               # AXFR zone transfer (auto-resolves authoritative NS)
dns example.com --region cn        # Use only Chinese DNS servers
dns example.com -v                 # Verbose: timing, consensus, status codes, flags
dns example.com --ttl              # Show TTL
dns example.com --json             # JSON output (for | jq)
dns example.com --short            # Short output (for | xargs)
dns a.com b.com --merge            # Merge multiple domains' results
cat domains.txt | dns A --merge    # Read domains from pipe + merge
cat domains.txt | dns --csv        # Read domains from pipe + CSV output
cat domains.txt | dns --json       # Read domains from pipe + JSON output
```

### Special Keywords (distinct from RFC record types)

| Keyword | Description |
|---------|-------------|
| `any` | Query all RFC record types (show everything, incl. empty/errors) |
| `axfr` | AXFR zone transfer (auto-resolves authoritative NS, needs auth) |

### RFC Record Types

Can be combined freely, e.g. `dns A AAAA MX example.com`:

```
A AAAA CNAME MX NS TXT SOA SRV CAA PTR HINFO
DNSKEY DS RRSIG NSEC TLSA SVCB HTTPS ... all RFC types
```

## Built-in DNS Servers

By default, queries all servers below + local DNS concurrently. Use `--region global` or `--region cn` to filter.

| Region | Name | IP | Description |
|--------|------|-----|-------------|
| global | google | 8.8.8.8 / 8.8.4.4 | Google Public DNS |
| global | cloudflare | 1.1.1.1 / 1.0.0.1 | Cloudflare DNS |
| global | quad9 | 9.9.9.9 | Quad9 (privacy+security) |
| global | opendns | 208.67.222.222 | OpenDNS (Cisco) |
| global | verisign | 64.6.64.6 | Verisign |
| cn | alidns | 223.5.5.5 / 223.6.6.6 | Alibaba AliDNS |
| cn | tencent | 119.29.29.29 / 119.28.28.28 | Tencent DNSPod |
| cn | baidu | 180.76.76.76 | BaiduDNS |
| cn | 114dns | 114.114.114.114 | 114DNS |

`dns --list-servers` to view the full list.

## `--server` Protocol Formats

`--server` and `@server` support multiple servers via protocol prefix:

| Format | Protocol | Description |
|--------|----------|-------------|
| `8.8.8.8` | UDP | Standard DNS, default port 53 |
| `8.8.8.8:5353` | UDP | Custom port |
| `a.gtld-servers.net` | UDP | Domain name (auto-resolved to IP) |
| `tls://1.1.1.1` | DoT | DNS-over-TLS, default 853 |
| `tls://1.1.1.1?sn=cloudflare-dns.com` | DoT | With SNI |
| `quic://1.1.1.1?sn=cloudflare-dns.com` | DoQ | DNS-over-QUIC, default 853 |
| `https://doh.pub/dns-query` | DoH | Standard DoH (RFC 8444) |
| `http://119.29.29.29/d` | HTTPDNS | Tencent private format (auto-detected) |
| `doh://8.8.8.8` | DoH | Alias for https:// |
| `dot://1.1.1.1` | DoT | Alias for tls:// |

```sh
dns example.com --server tls://1.1.1.1?sn=cloudflare-dns.com   # Cloudflare DoT
dns example.com @quic://1.1.1.1                                 # Cloudflare DoQ
dns example.com --server https://doh.pub/dns-query              # Tencent DoH
dns example.com --server http://119.29.29.29/d                  # Tencent HTTPDNS
```

Omit `--server` to use built-in servers. Use `--tcp` to force TCP (default UDP).

### HTTPDNS Smart Compatibility

`http://` or `https://` URLs auto-try multiple formats:

1. **Standard DoH JSON** (RFC 8444) — Cloudflare, Google, Tencent doh.pub, Alibaba dns.alidns.com
2. **Tencent private HTTPDNS** — 119.29.29.29 etc. (only A/AAAA)

Auto-fallback on failure — just write the vendor's access URL.

## Output Formats

Auto-switches based on stdout TTY:

- **Interactive terminal (TTY)**: Colored groups, IP green, domains yellow, diffs in magenta
- **Pipe / redirect (non-TTY)**: Plain text, `TYPE: value` one per line, grep-friendly
- **`--json`**: Full multi-server details with status codes and flags
- **`--yaml`**: YAML format
- **`--csv`**: CSV format (`domain,type,value,ttl`)
- **`--short`**: Minimal output, only values

### Verbose Mode (-v)

Shows: query timing, consensus count `(N/M)`, response status codes (RFC standard: NOERROR/NXDOMAIN/SERVFAIL etc.),
DNS flags (RD/RA/AA/AD/TC), response message size, DNSSEC validation result (AD flag),
Authority/Additional sections.

```sh
$ dns example.com -v --server 8.8.8.8
# example.com @ udp:8.8.8.8:53 (single) (273ms)
A: 172.66.147.243 (ttl=300)
A: 104.20.23.154 (ttl=300)
A: flags=RD,RA size=72B
...
```

### Multi-domain Merge (--merge)

```sh
$ dns example.com github.com --merge
A 172.66.147.243
A 104.20.23.154
A 20.205.243.166
AAAA 2606:4700:10::6814:179a
```

### Read Domains from File or Pipe

```sh
dns --file domains.txt               # From file (one per line, # comments skipped)
dns --file -                         # From stdin
cat domains.txt | dns A --merge      # Auto-detect stdin pipe
echo example.com | dns MX            # Single domain via pipe
```

## Configuration File

Path (system standard):
- Linux: `~/.config/ejfkdev/dns/config.toml`
- macOS: `~/Library/Application Support/ejfkdev/dns/config.toml`
- Windows: `%APPDATA%\ejfkdev\dns\config.toml`

```toml
# Default DNS server (single)
server = "8.8.8.8"

# Or multiple servers (array)
servers = [
  "8.8.8.8",
  "1.1.1.1",
  "tls://1.1.1.1?sn=cloudflare-dns.com",  # DoT/DoH/DoQ supported
  "https://doh.pub/dns-query",
]

# server and servers can both be used; they are merged

region = "cn"       # Filter built-in servers: global / cn (default: all)
timeout = 3          # Query timeout in seconds (default: 2)
color = "auto"        # Color: auto / always / never (default: auto)
ttl = true            # Show TTL (default: false)
verbose = false       # Verbose output (default: false)
```

CLI args override config file. Unset options fall back to config, then to defaults.

## Help Language

Auto-detects system language (Chinese/English). `dns -h` or running `dns` with no args shows the matching language.

## Download Pre-built Binaries

From [Releases](https://github.com/ejfkdev/dns/releases):

| Platform | File |
|----------|------|
| Linux x86_64 | `dns-v*-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `dns-v*-aarch64-unknown-linux-gnu.tar.gz` |
| macOS x86_64 | `dns-v*-x86_64-apple-darwin.tar.gz` |
| macOS arm64 (Apple Silicon) | `dns-v*-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `dns-v*-x86_64-pc-windows-msvc.zip` |

Binaries are stripped + UPX compressed for minimal size.

Tagging (e.g. `v0.2.0`) triggers CI build and Release publish.

## Build from Source

```sh
git clone https://github.com/ejfkdev/dns.git
cd dns
cargo build --release
cp target/release/dns ~/.local/bin/
```

Release profile configured with LTO + strip + opt-level=z for minimal size.

## Dependencies

- [hickory-resolver](https://crates.io/crates/hickory-resolver) — DNS (UDP/TCP/DoT/DoH/DoQ)
- [clap](https://crates.io/crates/clap) — CLI args
- [colored](https://crates.io/crates/colored) — Terminal colors
- [tokio](https://crates.io/crates/tokio) — Async runtime
- [reqwest](https://crates.io/crates/reqwest) — HTTPDNS client
- [idna](https://crates.io/crates/idna) — Punycode conversion
- [dirs](https://crates.io/crates/dirs) — System standard paths
- [toml](https://crates.io/crates/toml) — Config file parsing

## License

MIT
