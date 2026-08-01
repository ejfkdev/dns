//! DNS 记录类型定义与分组。
//!
//! 与 `dig`/`dog` 默认只查单一类型不同，本工具默认查询所有记录类型，
//! 但仅显示有结果的项目；`dns any` 则显示全部（含无记录与错误项）。

use std::str::FromStr;

use hickory_proto::rr::RecordType;

/// "常用全量"记录类型分组。
///
/// 覆盖日常运维、调试最常见的类型。当前默认查询实际使用 [`ALL_TYPES`]，
/// 此分组保留以备 `--common` 等细分选项使用。
#[allow(dead_code)]
pub const COMMON_TYPES: &[RecordType] = &[
    RecordType::A,
    RecordType::AAAA,
    RecordType::CNAME,
    RecordType::MX,
    RecordType::NS,
    RecordType::TXT,
    RecordType::SOA,
    RecordType::SRV,
    RecordType::CAA,
    RecordType::PTR,
    RecordType::HINFO,
];

/// `dns any` 触发的 RFC 全量记录类型，也是默认查询实际使用的类型集。
///
/// 在常用全量基础上增加 DNSSEC、TLS 验证、SSH 指纹等较全的类型。
/// 不包含 AXFR/IXFR（区传送需授权，普通查询会失败）和 OPT（伪记录）。
pub const ALL_TYPES: &[RecordType] = &[
    RecordType::A,
    RecordType::AAAA,
    RecordType::CNAME,
    RecordType::MX,
    RecordType::NS,
    RecordType::TXT,
    RecordType::SOA,
    RecordType::SRV,
    RecordType::CAA,
    RecordType::PTR,
    RecordType::HINFO,
    RecordType::ANAME,
    RecordType::DNSKEY,
    RecordType::DS,
    RecordType::RRSIG,
    RecordType::NSEC,
    RecordType::NSEC3,
    RecordType::NSEC3PARAM,
    RecordType::TLSA,
    RecordType::SMIMEA,
    RecordType::SSHFP,
    RecordType::NAPTR,
    RecordType::OPENPGPKEY,
    RecordType::KEY,
    RecordType::CERT,
    RecordType::CSYNC,
    RecordType::SVCB,
    RecordType::HTTPS,
];

/// 判断字符串是否为一个已知的记录类型名或 `any`/`*`。
pub fn is_type_keyword(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    upper == "ANY" || upper == "*" || RecordType::from_str(&upper).is_ok()
}
