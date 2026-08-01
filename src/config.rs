//! 配置文件读写。
//!
//! 路径（系统标准）：
//! - Linux: ~/.config/ejfkdev/dns/config.toml
//! - macOS: ~/Library/Application Support/ejfkdev/dns/config.toml
//! - Windows: %APPDATA%\ejfkdev\dns/config.toml

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 配置文件内容。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// 默认 DNS 服务器（单个，与 servers 合并）
    pub server: Option<String>,
    /// 默认 DNS 服务器列表（多个）
    pub servers: Vec<String>,
    /// 默认 region（global / cn）
    pub region: Option<String>,
    /// 默认超时秒数
    pub timeout: Option<u64>,
    /// 默认颜色模式（auto / always / never）
    pub color: Option<String>,
    /// 默认是否显示 TTL
    pub ttl: Option<bool>,
    /// 默认是否详细输出
    pub verbose: Option<bool>,
}

impl Config {
    /// 合并 server 和 servers，返回所有服务器列表
    pub fn all_servers(&self) -> Vec<String> {
        let mut list = self.servers.clone();
        if let Some(s) = &self.server {
            list.insert(0, s.clone());
        }
        list
    }
}

/// 配置文件路径。
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|base| base.join("ejfkdev").join("dns").join("config.toml"))
}

/// 加载配置文件。文件不存在时返回默认值（不报错）。
pub fn load() -> Config {
    let path = match config_path() {
        Some(p) => p,
        None => return Config::default(),
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// 保存配置到文件（创建目录）。
#[allow(dead_code)]
pub fn save(config: &Config) -> std::io::Result<()> {
    let path = config_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无法确定配置目录"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    std::fs::write(&path, content)?;
    Ok(())
}
