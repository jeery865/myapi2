use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::str::FromStr;
use std::time::Duration;

/// 全局配置，JSON 文件 + 环境变量双来源（环境变量优先）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 监听地址，默认 127.0.0.1:47821（本地软件默认仅本机）
    pub listen_addr: String,
    /// 上游 API 地址
    pub upstream_base_url: String,
    /// Freebuff 认证 token（多账号轮询）
    pub auth_tokens: Vec<String>,
    /// 本网关对外鉴权 key（空则不校验）
    pub api_keys: Vec<String>,
    /// run 轮换间隔
    pub rotation_interval_sec: u64,
    /// 上游请求超时
    pub request_timeout_sec: u64,
    /// HTTP 代理（支持 http/socks5）
    pub http_proxy: String,
    /// 会话保活间隔（广告刷新 / 心跳）
    pub session_keepalive_sec: u64,
    /// 广告保活 provider（逗号分隔：gravity,zeroclick,carbon）
    pub ad_providers: Vec<String>,
    /// 模型路由降级链配置
    pub fallback_models: Vec<String>,
    /// 是否启用 token 节省（压缩超长 tool_result）
    pub token_saver: bool,
    /// 用量统计 SQLite 路径（空则禁用统计）
    pub sqlite_path: String,
    /// 导入凭证存储路径（curl/HAR/Cookie 解析后落盘位置）
    pub tokens_path: String,
    /// 遥测 SQLite 路径（请求详情/事件链，独立库避免写锁竞争）
    pub telemetry_path: String,
    /// 记忆库 SQLite 路径（用户偏好/纠正；独立库）
    pub memory_path: String,
    /// 上游会话记录（web 协议 threadId；供自动清理）
    pub threads_path: String,
    /// 凭证账号信息缓存（昵称/邮箱/套餐/今日剩余，面板凭证列表用）
    pub cred_meta_path: String,
    /// 账号使用记录（JSONL，按凭证可查历史）
    pub account_history_path: String,
    /// 上游会话自动清理间隔（秒）；0 = 关闭自动清理
    pub thread_cleanup_interval_sec: u64,
    /// 上游会话保留时长（小时），超过即清理
    pub thread_max_age_hours: u64,
    /// web 协议桥接的会话绑定（OpenAI/Anthropic 客户端 → 上游 thread 复用）
    pub web_threads_path: String,
    /// 记忆层开关（false 时既不自动记录也不注入；隐私敏感用户可关）
    pub memory_enabled: bool,
    /// 技能目录（技能文件真相源）
    pub skills_dir: String,
    /// 技能注入模式：roster（只注入名称+描述）| full（全量拼接）
    pub skills_inject_mode: String,
    /// roster 注入的 token 预算上限
    pub max_roster_tokens: usize,
    /// 内置面板目录（空则用嵌入资源）
    pub web_dir: String,
    /// 启动时跳过上游连通性检查
    pub skip_upstream_check: bool,
    /// 日志/遥测脱敏（默认开）：写入日志总线与遥测前把 Cookie/Bearer/authorization 值替换为 ***
    pub redact_logs: bool,
    /// 双桶并发信号量：免费层 {付费槽, 普通}
    pub concurrency_free_slots: usize,
    pub concurrency_free_multi: usize,
    /// 双桶并发信号量：订阅层 {付费槽, 普通}
    pub concurrency_sub_slots: usize,
    pub concurrency_sub_multi: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:47821".into(),
            upstream_base_url: "https://www.codebuff.com".into(),
            auth_tokens: vec![],
            api_keys: vec![],
            rotation_interval_sec: 6 * 3600,
            request_timeout_sec: 900,
            http_proxy: String::new(),
            session_keepalive_sec: 45,
            ad_providers: vec!["gravity".into()],
            fallback_models: vec![],
            token_saver: false,
            sqlite_path: "data/freebuff2api.sqlite".into(),
            tokens_path: "data/tokens.json".into(),
            telemetry_path: "data/telemetry.sqlite".into(),
            memory_path: "data/memory.sqlite".into(),
            threads_path: "data/threads.json".into(),
            cred_meta_path: "data/cred_meta.json".into(),
            account_history_path: "data/account_history.jsonl".into(),
            // 用户批注（网页对话.txt:605）：反代要自己清理上游会话，别把压力留给上游被查出来
            thread_cleanup_interval_sec: 3600,
            thread_max_age_hours: 24,
            web_threads_path: "data/web_threads.json".into(),
            // 记忆默认关闭（用户批注 2026-09-11：记忆不是每个人都需要的，要有单独开关且默认关）
            memory_enabled: false,
            skills_dir: "data/skills".into(),
            skills_inject_mode: "roster".into(),
            max_roster_tokens: 2000,
            web_dir: String::new(),
            skip_upstream_check: false,
            redact_logs: true,
            concurrency_free_slots: 1,
            concurrency_free_multi: 3,
            concurrency_sub_slots: 3,
            concurrency_sub_multi: 8,
        }
    }
}

impl Config {
    /// 从默认值 + JSON 文件 + 环境变量合并加载
    pub fn load(path: Option<&str>) -> Result<Self> {
        let mut cfg = Self::default();

        if let Some(p) = path {
            if std::path::Path::new(p).exists() {
                let data = std::fs::read_to_string(p)
                    .map_err(|e| anyhow!("读取配置文件 {p} 失败: {e}"))?;
                let file_cfg: Config = serde_json::from_str(&data)
                    .map_err(|e| anyhow!("解析配置文件 {p} 失败: {e}"))?;
                cfg = file_cfg;
            } else {
                return Err(anyhow!("配置文件不存在: {p}"));
            }
        }
        // 自动探测默认 config.json
        else if std::path::Path::new("config.json").exists() {
            let data = std::fs::read_to_string("config.json")?;
            cfg = serde_json::from_str(&data)?;
        }

        cfg.apply_env();
        cfg.validate()?;
        Ok(cfg)
    }

    fn apply_env(&mut self) {
        if let Ok(v) = env::var("LISTEN_ADDR") {
            self.listen_addr = v;
        }
        // 容器/PaaS 注入的端口（Railway / Render / Fly / Heroku / Cloud Run 统一叫 PORT）：
        // 覆盖 listen_addr 的端口部分；原 host 是回环（默认 127.0.0.1）时改成 0.0.0.0 ——
        // 平台的反向代理从容器外部进来，绑回环等于服务完全不可达。
        if let Ok(v) = env::var("PORT") {
            match v.trim().parse::<u16>() {
                Ok(port) if port > 0 => self.listen_addr = rewrite_port(&self.listen_addr, port),
                Ok(_) => eprintln!("[config] 忽略非法的 PORT 环境变量（端口必须大于 0）: {v:?}"),
                Err(_) => eprintln!("[config] 忽略非法的 PORT 环境变量（不是数字）: {v:?}"),
            }
        }
        // 数据目录（平台把持久卷挂到这个路径）：所有相对路径统一挪到它下面。
        // 不做这一步，默认的 data/* 会落在容器可写层，每次重新部署都会丢数据。
        if let Ok(v) = env::var("DATA_DIR") {
            let dir = v.trim().trim_end_matches(['/', '\\']).to_string();
            if dir.is_empty() {
                eprintln!("[config] 忽略空的 DATA_DIR 环境变量");
            } else {
                self.sqlite_path = under_data_dir(&self.sqlite_path, &dir);
                self.tokens_path = under_data_dir(&self.tokens_path, &dir);
                self.telemetry_path = under_data_dir(&self.telemetry_path, &dir);
                self.memory_path = under_data_dir(&self.memory_path, &dir);
                self.threads_path = under_data_dir(&self.threads_path, &dir);
                self.cred_meta_path = under_data_dir(&self.cred_meta_path, &dir);
                self.account_history_path = under_data_dir(&self.account_history_path, &dir);
                self.web_threads_path = under_data_dir(&self.web_threads_path, &dir);
                self.skills_dir = under_data_dir(&self.skills_dir, &dir);
            }
        }
        if let Ok(v) = env::var("UPSTREAM_BASE_URL") {
            self.upstream_base_url = v;
        }
        if let Ok(v) = env::var("AUTH_TOKENS") {
            self.auth_tokens = v
                .split([',', '\n'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = env::var("API_KEYS") {
            self.api_keys = v
                .split([',', '\n'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        // 启动期跳过「至少需要一个 AUTH_TOKENS」的门禁。
        // PaaS 场景（Railway 等）首次部署时还没导凭证，必须先让服务起来、面板可访问，
        // 否则容器会 crash-loop，用户连导入 token 的入口都没有。
        if let Ok(v) = env::var("SKIP_UPSTREAM_CHECK") {
            self.skip_upstream_check = v == "1" || v.eq_ignore_ascii_case("true");
        }
        if let Ok(v) = env::var("ROTATION_INTERVAL") {
            self.rotation_interval_sec =
                parse_duration_sec(&v).unwrap_or(self.rotation_interval_sec);
        }
        if let Ok(v) = env::var("REQUEST_TIMEOUT") {
            self.request_timeout_sec = parse_duration_sec(&v).unwrap_or(self.request_timeout_sec);
        }
        if let Ok(v) = env::var("HTTP_PROXY") {
            self.http_proxy = v;
        }
        if let Ok(v) = env::var("AD_PROVIDERS") {
            self.ad_providers = v
                .split([',', '\n'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = env::var("SQLITE_PATH") {
            self.sqlite_path = v;
        }
        if let Ok(v) = env::var("TOKENS_PATH") {
            self.tokens_path = v;
        }
        if let Ok(v) = env::var("TELEMETRY_PATH") {
            self.telemetry_path = v;
        }
        if let Ok(v) = env::var("MEMORY_PATH") {
            self.memory_path = v;
        }
        if let Ok(v) = env::var("CRED_META_PATH") {
            self.cred_meta_path = v;
        }
        if let Ok(v) = env::var("ACCOUNT_HISTORY_PATH") {
            self.account_history_path = v;
        }
        if let Ok(v) = env::var("THREAD_CLEANUP_INTERVAL") {
            self.thread_cleanup_interval_sec =
                parse_duration_sec(&v).unwrap_or(self.thread_cleanup_interval_sec);
        }
        if let Ok(v) = env::var("THREAD_MAX_AGE_HOURS") {
            if let Ok(n) = v.parse() {
                self.thread_max_age_hours = n;
            }
        }
        if let Ok(v) = env::var("WEB_THREADS_PATH") {
            self.web_threads_path = v;
        }
        if let Ok(v) = env::var("MEMORY_ENABLED") {
            self.memory_enabled = v == "1" || v.eq_ignore_ascii_case("true");
        }
        if let Ok(v) = env::var("SKILLS_DIR") {
            self.skills_dir = v;
        }
        if let Ok(v) = env::var("SKILLS_INJECT_MODE") {
            self.skills_inject_mode = v;
        }
        if let Ok(v) = env::var("MAX_ROSTER_TOKENS") {
            if let Ok(n) = v.parse() {
                self.max_roster_tokens = n;
            }
        }
        if let Ok(v) = env::var("WEB_DIR") {
            self.web_dir = v;
        }
        if let Ok(v) = env::var("REDACT_LOGS") {
            self.redact_logs = v == "1" || v.eq_ignore_ascii_case("true");
        }
        if let Ok(v) = env::var("CONCURRENCY_FREE_SLOTS") {
            if let Ok(n) = v.parse() {
                self.concurrency_free_slots = n;
            }
        }
        if let Ok(v) = env::var("CONCURRENCY_FREE_MULTI") {
            if let Ok(n) = v.parse() {
                self.concurrency_free_multi = n;
            }
        }
        if let Ok(v) = env::var("CONCURRENCY_SUB_SLOTS") {
            if let Ok(n) = v.parse() {
                self.concurrency_sub_slots = n;
            }
        }
        if let Ok(v) = env::var("CONCURRENCY_SUB_MULTI") {
            if let Ok(n) = v.parse() {
                self.concurrency_sub_multi = n;
            }
        }
        if let Ok(v) = env::var("TOKEN_SAVER") {
            self.token_saver = v == "1" || v.eq_ignore_ascii_case("true");
        }
        if let Ok(v) = env::var("FALLBACK_MODELS") {
            self.fallback_models = v
                .split([',', '\n'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }

    fn validate(&self) -> Result<()> {
        if self.listen_addr.trim().is_empty() {
            return Err(anyhow!("LISTEN_ADDR 不能为空"));
        }
        // 安全守卫：监听非本机地址时必须配置 api_keys（否则管理端点/记忆/凭证对网络裸奔）
        if !is_loopback_listen(&self.listen_addr) && self.api_keys.is_empty() {
            return Err(anyhow!(
                "安全拒绝：listen_addr={} 不是本机地址，但未配置 api_keys。\
                 请配置 api_keys（推荐）或改回 127.0.0.1",
                self.listen_addr
            ));
        }
        if self.upstream_base_url.trim().is_empty() {
            return Err(anyhow!("UPSTREAM_BASE_URL 不能为空"));
        }
        if self.auth_tokens.is_empty() && !self.skip_upstream_check {
            return Err(anyhow!("至少需要一个 AUTH_TOKENS"));
        }
        let unique: HashSet<&String> = self.auth_tokens.iter().collect();
        if unique.len() != self.auth_tokens.len() {
            return Err(anyhow!("AUTH_TOKENS 存在重复 token"));
        }
        Ok(())
    }
}

/// 从 `host[:port]` 取 host。`[::1]:8080` → `::1`；裸 `::1` → `::1`；`0.0.0.0:8080` → `0.0.0.0`。
pub fn host_of(listen_addr: &str) -> String {
    let s = listen_addr.trim();
    if let Some(rest) = s.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("").to_string();
    }
    // 裸 IPv6（没有方括号也没有端口）直接就是 host，别让 rsplit_once(':') 切坏
    if s.parse::<std::net::IpAddr>().is_ok() {
        return s.to_string();
    }
    match s.rsplit_once(':') {
        Some((h, _)) => h.to_string(),
        None => s.to_string(),
    }
}

/// host 是否为回环（精确匹配，绝不做前缀判断 —— 防 `localhost.evil.com` 这类绕过）
pub fn is_loopback_host(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    matches!(host, "127.0.0.1" | "localhost" | "::1")
        || std::net::IpAddr::from_str(host)
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

/// 判断监听地址是否为本机（host 部分精确匹配，防 `localhost.evil.com` 这类前缀绕过）。
pub fn is_loopback_listen(listen_addr: &str) -> bool {
    is_loopback_host(&host_of(listen_addr))
}

/// 用平台注入的端口重写监听地址：保留原 host（回环则换成 `0.0.0.0`），只换端口。
///
/// 容器里绑 `127.0.0.1` 等于对外不可达，所以回环必须改掉 —— 这是 `PORT` 语义的一部分，
/// 不是可选优化（Railway/Render 的健康检查从容器外部发起，绑回环会直接判定部署失败）。
pub fn rewrite_port(listen_addr: &str, port: u16) -> String {
    let host = host_of(listen_addr);
    let host = if host.is_empty() || is_loopback_host(&host) {
        "0.0.0.0".to_string()
    } else {
        host
    };
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// 绝对路径判定：POSIX `/x`、Windows `C:\x` / `C:/x`（容器里以 POSIX 为主，两种都认）
pub fn is_absolute_path(p: &str) -> bool {
    if p.starts_with('/') || p.starts_with('\\') {
        return true;
    }
    let b = p.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// 把相对路径并入 `DATA_DIR`；绝对路径与空串原样返回。
///
/// 约定：`data/xxx` 视作 `DATA_DIR/xxx`（去掉重复的 `data/` 前缀），
/// 其余相对路径（如 `tokens.json`）视作 `DATA_DIR/tokens.json`。
pub fn under_data_dir(path: &str, data_dir: &str) -> String {
    let p = path.trim();
    if p.is_empty() || is_absolute_path(p) {
        return p.to_string();
    }
    let rel = p
        .strip_prefix("data/")
        .or_else(|| p.strip_prefix("data\\"))
        .unwrap_or(p);
    let dir = data_dir.trim().trim_end_matches(['/', '\\']);
    format!("{dir}/{rel}")
}

/// 解析配置文件路径：`--config x.json` > 第一个位置参数 > 当前目录 config.json > None
///
/// 与启动时 `Config::load` 的取舍保持一致，供"运行时写回配置"（如面板一键生成 API Key）复用。
pub fn resolve_config_path() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--config") {
        if let Some(p) = args.get(i + 1) {
            return Some(p.clone());
        }
    }
    if let Some(a) = args.get(1) {
        if !a.starts_with("--") {
            return Some(a.clone());
        }
    }
    if std::path::Path::new("config.json").exists() {
        return Some("config.json".into());
    }
    None
}

/// 解析 "6h" / "900s" / "15m" 或纯秒数
pub fn parse_duration_sec(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if let Ok(secs) = raw.parse::<u64>() {
        return Some(secs);
    }
    let (num, unit) = raw.split_at(raw.len().saturating_sub(1));
    let n: u64 = num.trim().parse().ok()?;
    Some(match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        _ => return None,
    })
}

pub fn default_request_timeout() -> Duration {
    Duration::from_secs(900)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_parses_all_forms() {
        assert_eq!(host_of("127.0.0.1:47821"), "127.0.0.1");
        assert_eq!(host_of("0.0.0.0:8080"), "0.0.0.0");
        assert_eq!(host_of("localhost:1"), "localhost");
        assert_eq!(host_of("[::1]:8080"), "::1");
        assert_eq!(host_of("::1"), "::1");
        assert_eq!(host_of("47821"), "47821");
    }

    #[test]
    fn loopback_matching_is_exact_not_prefix() {
        assert!(is_loopback_listen("127.0.0.1:47821"));
        assert!(is_loopback_listen("localhost:47821"));
        assert!(is_loopback_listen("[::1]:47821"));
        // 前缀绕过必须被挡住
        assert!(!is_loopback_listen("localhost.evil.com:47821"));
        assert!(!is_loopback_listen("127.0.0.1.evil.com:47821"));
        assert!(!is_loopback_listen("0.0.0.0:47821"));
    }

    #[test]
    fn rewrite_port_breaks_loopback_so_container_is_reachable() {
        // 容器里最关键的一条：默认 127.0.0.1 必须被换成 0.0.0.0，否则平台健康检查必失败
        assert_eq!(rewrite_port("127.0.0.1:47821", 8080), "0.0.0.0:8080");
        assert_eq!(rewrite_port("localhost:47821", 8080), "0.0.0.0:8080");
        assert_eq!(rewrite_port("[::1]:47821", 8080), "0.0.0.0:8080");
    }

    #[test]
    fn rewrite_port_keeps_explicit_non_loopback_host() {
        assert_eq!(rewrite_port("0.0.0.0:47821", 8080), "0.0.0.0:8080");
        assert_eq!(
            rewrite_port("192.168.1.10:47821", 8080),
            "192.168.1.10:8080"
        );
        // 显式 IPv6 要保持方括号，否则拼出来不是一个合法监听地址
        assert_eq!(
            rewrite_port("[2001:db8::1]:47821", 8080),
            "[2001:db8::1]:8080"
        );
    }

    #[test]
    fn under_data_dir_moves_relative_paths_only() {
        assert_eq!(
            under_data_dir("data/freebuff2api.sqlite", "/data"),
            "/data/freebuff2api.sqlite"
        );
        assert_eq!(under_data_dir("data/skills", "/data"), "/data/skills");
        assert_eq!(under_data_dir("tokens.json", "/data"), "/data/tokens.json");
        // 绝对路径不动（用户已经写死到卷上的情形）
        assert_eq!(under_data_dir("/data/x.sqlite", "/data"), "/data/x.sqlite");
        assert_eq!(under_data_dir("/srv/y.sqlite", "/data"), "/srv/y.sqlite");
        assert_eq!(under_data_dir("C:\\x.sqlite", "/data"), "C:\\x.sqlite");
        // 空串保持空串（空 = 关闭该功能，别给它拼出一个路径）
        assert_eq!(under_data_dir("", "/data"), "");
        // 尾部斜杠不会拼出双斜杠
        assert_eq!(under_data_dir("data/x.sqlite", "/data/"), "/data/x.sqlite");
    }

    #[test]
    fn is_absolute_path_accepts_posix_and_windows() {
        assert!(is_absolute_path("/data/a"));
        assert!(is_absolute_path("C:\\a"));
        assert!(is_absolute_path("c:/a"));
        assert!(!is_absolute_path("data/a"));
        assert!(!is_absolute_path("a.sqlite"));
        assert!(!is_absolute_path(""));
    }
}
