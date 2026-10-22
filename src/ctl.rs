use crate::adc_auth::{configure_background_command, user_home_dir, ADCAuthManager};
use crate::translator::{
    format_model_display_name, format_model_short_badge, format_model_short_label,
};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

const MAX_OUTPUT_BYTES: u64 = 1_048_576; // 1 MiB
pub const VALID_TARGETS: &[&str] = &["claude", "codex", "both"];
pub const DEFAULT_PRESET_MODELS: &[&str] = &["gemini-3.8-flash", "gemini-4-argon"];
pub const VALID_MODELS: &[&str] = DEFAULT_PRESET_MODELS;
pub const VALID_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh"];

const CODEX_BLOCK_BEGIN: &str = "# BEGIN OMARCHY_GEMINI_ADC_ROUTER";
const CODEX_BLOCK_END: &str = "# END OMARCHY_GEMINI_ADC_ROUTER";
const MENU_BLOCK_BEGIN: &str = "  // gemini-router-menu-start";
const MENU_BLOCK_END: &str = "  // gemini-router-menu-end";

const CLAUDE_MANAGED_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "DISABLE_PROMPT_CACHING",
    "DISABLE_PROMPT_CACHING_HAIKU",
    "DISABLE_PROMPT_CACHING_SONNET",
    "DISABLE_PROMPT_CACHING_OPUS",
];

pub struct Paths {
    pub config_dir: PathBuf,
    pub state_file: PathBuf,
    pub env_file: PathBuf,
    pub pid_file: PathBuf,
    pub claude_settings: PathBuf,
    pub codex_config: PathBuf,
    pub menu_config: PathBuf,
    pub project_dir: PathBuf,
}

pub fn home_dir() -> PathBuf {
    user_home_dir()
}

pub fn get_paths() -> Paths {
    let home = home_dir();
    let config_dir = env::var("GEMINI_ROUTER_CONFIG_DIR")
        .or_else(|_| env::var("OMARCHY_GEMINI_ROUTER_CONFIG_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let generic = home.join(".config").join("gemini-router");
            let omarchy = home.join(".config").join("omarchy").join("gemini-router");
            if !generic.exists() && omarchy.exists() {
                omarchy
            } else {
                generic
            }
        });
    let claude_settings = env::var("CLAUDE_SETTINGS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".claude").join("settings.json"));
    let codex_config = env::var("CODEX_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".codex").join("config.toml"));
    let menu_config = env::var("OMARCHY_MENU_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            home.join(".config")
                .join("omarchy")
                .join("extensions")
                .join("omarchy-menu.jsonc")
        });

    let mut default_project_dir = home.join("Projects").join("claude-codex-gemini-router");
    if !default_project_dir.join("Cargo.toml").is_file() {
        let omarchy_dir = home.join("Projects").join("omarchy-claude-codex-router");
        if omarchy_dir.join("Cargo.toml").is_file() {
            default_project_dir = omarchy_dir;
        }
    }
    if let Ok(exe_path) = env::current_exe() {
        if let Some(p3) = exe_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            if p3.join("Cargo.toml").is_file() {
                default_project_dir = p3.to_path_buf();
            }
        }
    }
    let project_dir = env::var("ROUTER_PROJECT_DIR")
        .map(PathBuf::from)
        .unwrap_or(default_project_dir);

    Paths {
        state_file: config_dir.join("state.json"),
        env_file: config_dir.join("router.env"),
        pid_file: config_dir.join("router.pid"),
        config_dir,
        claude_settings,
        codex_config,
        menu_config,
        project_dir,
    }
}

pub fn router_port() -> u16 {
    env::var("ROUTER_PORT")
        .ok()
        .and_then(|raw| raw.parse::<u16>().ok())
        .filter(|&p| p >= 1)
        .unwrap_or(4000)
}

pub fn router_url() -> String {
    env::var("ROUTER_URL")
        .unwrap_or_else(|_| format!("http://127.0.0.1:{}", router_port()))
        .trim_end_matches('/')
        .to_string()
}

fn is_mock_daemon() -> bool {
    env::var("GEMINI_ROUTER_MOCK_DAEMON").as_deref() == Ok("1")
        || env::var("OMARCHY_GEMINI_ROUTER_MOCK_DAEMON").as_deref() == Ok("1")
}

#[cfg(unix)]
fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

pub fn read_file_safely(path: &Path) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(format!("Refusing to read symlink: {}", path.display()));
            }
            #[cfg(unix)]
            if meta.uid() != current_uid() {
                return Err(format!(
                    "Refusing to read file not owned by current user: {}",
                    path.display()
                ));
            }
            if meta.len() > MAX_OUTPUT_BYTES {
                return Err(format!(
                    "File exceeds size limit ({} bytes): {}",
                    meta.len(),
                    path.display()
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Failed to stat {}: {}", path.display(), e)),
    }

    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    opts.custom_flags(libc::O_NOFOLLOW);

    let mut file = match opts.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            #[cfg(unix)]
            if e.raw_os_error() == Some(libc::ELOOP) {
                return Err(format!("Refusing to read symlink: {}", path.display()));
            }
            return Err(format!("Failed to open {}: {}", path.display(), e));
        }
    };

    let meta = file
        .metadata()
        .map_err(|e| format!("fstat failed on {}: {}", path.display(), e))?;
    #[cfg(unix)]
    if meta.uid() != current_uid() {
        return Err(format!(
            "Refusing to read file not owned by current user: {}",
            path.display()
        ));
    }
    if meta.len() > MAX_OUTPUT_BYTES {
        return Err(format!(
            "File exceeds size limit ({} bytes): {}",
            meta.len(),
            path.display()
        ));
    }

    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

pub fn write_file_safely(path: &Path, content: &str, mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Invalid path without parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;

    let parent_meta = fs::symlink_metadata(parent)
        .map_err(|e| format!("Failed to lstat parent {}: {}", parent.display(), e))?;
    if parent_meta.file_type().is_symlink() {
        return Err(format!(
            "Parent directory {} must not be a symlink",
            parent.display()
        ));
    }
    #[cfg(unix)]
    if parent_meta.uid() != current_uid() {
        return Err(format!(
            "Parent directory {} is not owned by current user",
            parent.display()
        ));
    }

    if let Ok(target_meta) = fs::symlink_metadata(path) {
        if target_meta.file_type().is_symlink() {
            return Err(format!("Refusing to write to symlink: {}", path.display()));
        }
        #[cfg(unix)]
        if target_meta.uid() != current_uid() {
            return Err(format!(
                "Target file {} is not owned by current user",
                path.display()
            ));
        }
    }

    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp_name = format!(
        ".{}.tmp.{}.{}",
        file_name,
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    );
    let tmp_path = parent.join(tmp_name);

    let res = (|| -> Result<(), String> {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            opts.mode(mode).custom_flags(libc::O_NOFOLLOW);
        }
        let mut f = opts
            .open(&tmp_path)
            .map_err(|e| format!("Failed to create temp file {}: {}", tmp_path.display(), e))?;
        f.write_all(content.as_bytes())
            .map_err(|e| format!("Failed to write temp file {}: {}", tmp_path.display(), e))?;
        f.sync_all()
            .map_err(|e| format!("Failed to fsync temp file {}: {}", tmp_path.display(), e))?;
        drop(f);
        #[cfg(unix)]
        {
            fs::set_permissions(&tmp_path, fs::Permissions::from_mode(mode))
                .map_err(|e| format!("chmod failed on {}: {}", tmp_path.display(), e))?;
        }
        #[cfg(windows)]
        {
            let _ = mode;
            if path.exists() {
                let _ = fs::remove_file(path);
            }
        }
        fs::rename(&tmp_path, path)
            .map_err(|e| format!("Atomic rename to {} failed: {}", path.display(), e))?;
        Ok(())
    })();

    if res.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    res
}

fn enriched_path_env() -> String {
    let home = home_dir();
    let current_path = env::var("PATH").unwrap_or_default();
    if cfg!(windows) {
        let mut parts = vec![
            home.join(".cargo")
                .join("bin")
                .to_string_lossy()
                .into_owned(),
            home.join(".local")
                .join("bin")
                .to_string_lossy()
                .into_owned(),
            home.join("google-cloud-sdk")
                .join("bin")
                .to_string_lossy()
                .into_owned(),
        ];
        if let Ok(local_appdata) = env::var("LOCALAPPDATA") {
            parts.push(
                PathBuf::from(local_appdata)
                    .join(r"Google\Cloud SDK\google-cloud-sdk\bin")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        parts.push(current_path);
        parts.join(";")
    } else {
        format!(
            "{}/.cargo/bin:{}/.local/share/mise/shims:{}/.local/bin:{}/google-cloud-sdk/bin:/opt/homebrew/bin:/opt/homebrew/share/google-cloud-sdk/bin:/usr/local/bin:/usr/bin:/bin:{}",
            home.display(),
            home.display(),
            home.display(),
            home.display(),
            current_path
        )
    }
}

pub fn run_cmd(args: &[&str], timeout: Duration) -> (i32, String, String) {
    if args.is_empty() {
        return (1, String::new(), "Empty command".to_string());
    }
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..])
        .env("PATH", enriched_path_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_command(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                127,
                String::new(),
                format!("Executable not found: {}", args[0]),
            );
        }
        Err(e) => return (1, String::new(), e.to_string()),
    };

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return match child.wait_with_output() {
                    Ok(out) => {
                        let code = out.status.code().unwrap_or(1);
                        let stdout_s = String::from_utf8_lossy(
                            &out.stdout[..out.stdout.len().min(MAX_OUTPUT_BYTES as usize)],
                        )
                        .into_owned();
                        let stderr_s = String::from_utf8_lossy(
                            &out.stderr[..out.stderr.len().min(MAX_OUTPUT_BYTES as usize)],
                        )
                        .into_owned();
                        (code, stdout_s, stderr_s)
                    }
                    Err(e) => (1, String::new(), e.to_string()),
                };
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (
                        124,
                        String::new(),
                        format!("Command timed out after {:.1}s", timeout.as_secs_f64()),
                    );
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return (1, String::new(), e.to_string()),
        }
    }
}

pub fn default_available_models() -> Vec<String> {
    if let Ok(raw) = env::var("GEMINI_AVAILABLE_MODELS") {
        let parsed: Vec<String> = raw
            .split(',')
            .filter_map(|s| normalize_model(Some(s.trim())).ok())
            .collect();
        if !parsed.is_empty() {
            return parsed;
        }
    }
    DEFAULT_PRESET_MODELS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

pub fn get_state_available_models(state: &Map<String, Value>) -> Vec<String> {
    let mut models: Vec<String> = state
        .get("available_models")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter_map(|s| normalize_model(Some(s)).ok())
                .collect()
        })
        .unwrap_or_default();
    if models.is_empty() {
        models = default_available_models();
    }
    if let Some(pm) = state.get("primary_model").and_then(|v| v.as_str()) {
        if let Ok(norm) = normalize_model(Some(pm)) {
            if !models.contains(&norm) {
                models.push(norm);
            }
        }
    }
    models
}

pub fn ensure_model_in_state_available(state: &mut Map<String, Value>, model: &str) {
    let mut models = get_state_available_models(state);
    if !models.iter().any(|m| m == model) {
        models.push(model.to_string());
    }
    state.insert(
        "available_models".to_string(),
        Value::Array(models.into_iter().map(Value::String).collect()),
    );
}

pub fn load_state() -> Result<Map<String, Value>, String> {
    let paths = get_paths();
    let raw = read_file_safely(&paths.state_file)?;
    let mut default_state = Map::new();
    default_state.insert(
        "primary_model".to_string(),
        Value::String("gemini-3.8-flash".to_string()),
    );
    default_state.insert(
        "available_models".to_string(),
        Value::Array(
            default_available_models()
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
    );
    default_state.insert(
        "fast_model".to_string(),
        Value::String("gemini-3.8-flash".to_string()),
    );
    default_state.insert("fallback_model".to_string(), Value::Null);
    default_state.insert(
        "reasoning_effort".to_string(),
        Value::String("medium".to_string()),
    );
    default_state.insert("claude_backup_env".to_string(), Value::Null);
    default_state.insert("codex_backup".to_string(), Value::Null);

    if let Some(text) = raw {
        if let Ok(Value::Object(parsed)) = serde_json::from_str::<Value>(&text) {
            for (k, v) in parsed {
                default_state.insert(k, v);
            }
        }
    }
    if !is_mock_daemon() {
        default_state.remove("mock_router_running");
    }
    if let Some(Value::Object(b)) = default_state.get_mut("claude_backup_env") {
        b.remove("DISABLE_PROMPT_CACHING");
        b.remove("DISABLE_PROMPT_CACHING_HAIKU");
        b.remove("DISABLE_PROMPT_CACHING_SONNET");
        b.remove("DISABLE_PROMPT_CACHING_OPUS");
    }
    let avail = get_state_available_models(&default_state);
    default_state.insert(
        "available_models".to_string(),
        Value::Array(avail.into_iter().map(Value::String).collect()),
    );
    Ok(default_state)
}

pub fn save_state(state: &Map<String, Value>) -> Result<(), String> {
    let paths = get_paths();
    let json_text = serde_json::to_string_pretty(&Value::Object(state.clone()))
        .map_err(|e| e.to_string())?
        + "\n";
    write_file_safely(&paths.state_file, &json_text, 0o600)?;

    let primary_model = state
        .get("primary_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash");
    let effort = state
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let fast_model = state
        .get("fast_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash");
    let avail = get_state_available_models(state).join(",");
    let mut env_content = format!(
        "GEMINI_PRIMARY_MODEL={}\nGEMINI_REASONING_EFFORT={}\nGEMINI_FAST_MODEL={}\nGEMINI_AVAILABLE_MODELS={}\nGEMINI_HOT_SWAP_ACTIVE=1\nROUTER_PORT={}\n",
        primary_model,
        effort,
        fast_model,
        avail,
        router_port()
    );
    if let Some(fb) = state
        .get("fallback_model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        env_content.push_str(&format!("GEMINI_FALLBACK_MODEL={}\n", fb));
    }
    write_file_safely(&paths.env_file, &env_content, 0o600)
}

pub fn is_safe_model_identifier(s: &str) -> bool {
    if s.len() < 2 || s.len() > 80 {
        return false;
    }
    if s.contains("..") || s.starts_with('-') || s.starts_with('.') {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn normalize_model(model: Option<&str>) -> Result<String, String> {
    let Some(m) = model else {
        return Ok("gemini-3.8-flash".to_string());
    };
    let clean = m.split('[').next().unwrap_or(m).trim();
    if clean.is_empty() {
        return Ok("gemini-3.8-flash".to_string());
    }
    let lowered = clean.to_lowercase();
    if !is_safe_model_identifier(&lowered) {
        return Err(format!(
            "Invalid model identifier '{}'. Must match [a-z0-9._-] (e.g. gemini-3.8-flash, gemini-4-argon, gemini-2.5-pro)",
            m
        ));
    }

    // 1. Check user-defined dynamic aliases in GEMINI_MODEL_ALIASES="alias=target,..."
    if let Ok(custom_aliases) = env::var("GEMINI_MODEL_ALIASES") {
        for pair in custom_aliases.split(',') {
            if let Some((alias, target)) = pair.split_once('=') {
                let alias_clean = alias.trim().to_lowercase();
                let target_clean = target.trim().to_lowercase();
                if !alias_clean.is_empty()
                    && is_safe_model_identifier(&target_clean)
                    && lowered == alias_clean
                {
                    return Ok(target_clean);
                }
            }
        }
    }

    // 2. Built-in shorthand aliases
    match lowered.as_str() {
        "gemini-4.0" | "gemini-4.0-pro" | "gemini-4-argon" | "4.0" | "argon" | "pro" => {
            return Ok("gemini-4-argon".to_string());
        }
        "gemini-3.8" | "gemini-3.8-flash" | "3.8" | "3.8-flash" | "flash" => {
            return Ok("gemini-3.8-flash".to_string());
        }
        "gemini-3.1" | "gemini-3.1-pro" | "3.1" | "3.1-pro" | "gemini-3.1-pro-preview" => {
            return Ok("gemini-3.1-pro-preview".to_string());
        }
        "gemini-3-flash" | "3-flash" | "gemini-3-flash-preview" => {
            return Ok("gemini-3-flash-preview".to_string());
        }
        "2.5-pro" | "gemini-2.5-pro" => {
            return Ok("gemini-2.5-pro".to_string());
        }
        "2.5-flash" | "gemini-2.5-flash" => {
            return Ok("gemini-2.5-flash".to_string());
        }
        "2.5-flash-lite" | "flash-lite" | "gemini-2.5-flash-lite" => {
            return Ok("gemini-2.5-flash-lite".to_string());
        }
        _ => {}
    }

    // 3. Any explicit Gemini model identifier (e.g. gemini-2.0-flash, gemini-exp-1206, future gemini-*)
    if lowered.starts_with("gemini-") {
        return Ok(lowered);
    }

    Err(format!(
        "Invalid model '{}'. Provide a 'gemini-*' model ID (e.g. gemini-3.8-flash, gemini-4-argon, gemini-2.5-pro) or shorthand alias",
        m
    ))
}

pub fn normalize_effort(effort: Option<&str>) -> Result<String, String> {
    let Some(e) = effort else {
        return Ok("medium".to_string());
    };
    let lowered = e.trim().to_lowercase();
    if lowered.is_empty() {
        return Ok("medium".to_string());
    }
    if VALID_EFFORTS.contains(&lowered.as_str()) {
        return Ok(lowered);
    }
    Err(format!(
        "Invalid reasoning effort '{}'. Allowed: {}",
        e,
        VALID_EFFORTS.join(", ")
    ))
}

pub fn is_claude_routed() -> Result<bool, String> {
    let paths = get_paths();
    let Some(raw) = read_file_safely(&paths.claude_settings)? else {
        return Ok(false);
    };
    let Ok(Value::Object(data)) = serde_json::from_str::<Value>(&raw) else {
        return Ok(false);
    };
    let Some(Value::Object(env_obj)) = data.get("env") else {
        return Ok(false);
    };
    let base_url = env_obj
        .get("ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_end_matches('/');
    let auth_token = env_obj
        .get("ANTHROPIC_AUTH_TOKEN")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    Ok(base_url == router_url() && auth_token == "adc-keyless")
}

pub fn configure_claude_routing(
    enable: bool,
    primary_model: &str,
    state: &mut Map<String, Value>,
) -> Result<(), String> {
    let paths = get_paths();
    let raw = read_file_safely(&paths.claude_settings)?;
    let mut settings: Map<String, Value> = Map::new();
    if let Some(text) = raw {
        if !text.trim().is_empty() {
            match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(m)) => settings = m,
                Ok(_) => {}
                Err(e) => {
                    return Err(format!(
                        "Malformed JSON in {}: {}",
                        paths.claude_settings.display(),
                        e
                    ))
                }
            }
        }
    }

    let current_env = settings
        .get("env")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let already_routed = current_env
        .get("ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_end_matches('/')
        == router_url()
        && current_env
            .get("ANTHROPIC_AUTH_TOKEN")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            == "adc-keyless";

    if enable {
        if !already_routed
            && state
                .get("claude_backup_env")
                .map(|v| v.is_null())
                .unwrap_or(true)
        {
            state.insert(
                "claude_backup_env".to_string(),
                Value::Object(current_env.clone()),
            );
            state.insert(
                "claude_backup_model".to_string(),
                settings.get("model").cloned().unwrap_or(Value::Null),
            );
            state.insert(
                "claude_backup_effort".to_string(),
                settings.get("effortLevel").cloned().unwrap_or(Value::Null),
            );
        }

        let mut base_env = state
            .get("claude_backup_env")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or(current_env);

        for conflict_key in [
            "CLAUDE_CODE_USE_VERTEX",
            "ANTHROPIC_API_KEY",
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
        ] {
            base_env.remove(conflict_key);
        }

        base_env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            Value::String(router_url()),
        );
        base_env.insert(
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            Value::String("adc-keyless".to_string()),
        );
        base_env.insert(
            "ANTHROPIC_MODEL".to_string(),
            Value::String(primary_model.to_string()),
        );
        base_env.insert(
            "ANTHROPIC_SMALL_FAST_MODEL".to_string(),
            Value::String("claude-haiku-4-5".to_string()),
        );
        base_env.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
            Value::String("claude-haiku-4-5".to_string()),
        );
        base_env.insert(
            "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
            Value::String(primary_model.to_string()),
        );
        base_env.insert(
            "ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(),
            Value::String(primary_model.to_string()),
        );
        // Keep prompt caching enabled so Claude Code sends normal cache_control
        // markers and the router reports Vertex AI cachedContentTokenCount back
        // as cache_read_input_tokens.
        base_env.remove("DISABLE_PROMPT_CACHING");
        base_env.remove("DISABLE_PROMPT_CACHING_HAIKU");
        base_env.remove("DISABLE_PROMPT_CACHING_SONNET");
        base_env.remove("DISABLE_PROMPT_CACHING_OPUS");

        let effort_level = state
            .get("reasoning_effort")
            .and_then(|v| v.as_str())
            .unwrap_or("medium")
            .to_string();
        settings.insert(
            "model".to_string(),
            Value::String(primary_model.to_string()),
        );
        settings.insert("effortLevel".to_string(), Value::String(effort_level));
        settings.insert("env".to_string(), Value::Object(base_env));
    } else if let Some(Value::Object(backup_env)) = state.get("claude_backup_env").cloned() {
        if !backup_env.is_empty() {
            settings.insert("env".to_string(), Value::Object(backup_env));
        } else {
            settings.remove("env");
        }
        match state.get("claude_backup_model").cloned() {
            Some(Value::String(m)) if !m.is_empty() => {
                settings.insert("model".to_string(), Value::String(m));
            }
            _ => {
                settings.remove("model");
            }
        }
        match state.get("claude_backup_effort").cloned() {
            Some(Value::String(e)) if !e.is_empty() => {
                settings.insert("effortLevel".to_string(), Value::String(e));
            }
            _ => {
                settings.remove("effortLevel");
            }
        }
        state.insert("claude_backup_env".to_string(), Value::Null);
        state.insert("claude_backup_model".to_string(), Value::Null);
        state.insert("claude_backup_effort".to_string(), Value::Null);
    } else {
        let mut updated_env = current_env;
        for k in CLAUDE_MANAGED_ENV_KEYS {
            updated_env.remove(*k);
        }
        if !updated_env.is_empty() {
            settings.insert("env".to_string(), Value::Object(updated_env));
        } else {
            settings.remove("env");
        }
        if settings
            .get("model")
            .and_then(|v| v.as_str())
            .map(|m| m.starts_with("gemini-"))
            .unwrap_or(false)
        {
            settings.remove("model");
        }
    }

    let out =
        serde_json::to_string_pretty(&Value::Object(settings)).map_err(|e| e.to_string())? + "\n";
    write_file_safely(&paths.claude_settings, &out, 0o600)
}

fn extract_top_level_toml_key(lines: &[String], key: &str) -> Option<String> {
    let pattern = Regex::new(&format!(r"^\s*{}\s*=\s*(.+?)\s*$", regex::escape(key))).ok()?;
    for line in lines {
        let stripped = line.trim();
        if stripped.starts_with('[') && !stripped.starts_with("[[") {
            break;
        }
        if pattern.is_match(line) {
            return Some(line.trim_end_matches('\n').to_string());
        }
    }
    None
}

fn remove_top_level_toml_keys(lines: &[String], keys: &[&str]) -> Vec<String> {
    let mut result = Vec::new();
    let mut in_top_level = true;
    let patterns: Vec<Regex> = keys
        .iter()
        .filter_map(|k| Regex::new(&format!(r"^\s*{}\s*=", regex::escape(k))).ok())
        .collect();
    for line in lines {
        let stripped = line.trim();
        if stripped.starts_with('[') {
            in_top_level = false;
        }
        if in_top_level && patterns.iter().any(|p| p.is_match(line)) {
            continue;
        }
        result.push(line.clone());
    }
    result
}

fn strip_codex_managed_block(text: &str) -> String {
    let pattern = Regex::new(&format!(
        r"(?s)\n?{}.*?{}\n?",
        regex::escape(CODEX_BLOCK_BEGIN),
        regex::escape(CODEX_BLOCK_END)
    ))
    .unwrap();
    pattern.replace_all(text, "\n").into_owned()
}

pub fn is_codex_routed() -> Result<bool, String> {
    let paths = get_paths();
    let Some(raw) = read_file_safely(&paths.codex_config)? else {
        return Ok(false);
    };
    let re = Regex::new(r#"(?m)^\s*model_provider\s*=\s*"gemini_adc""#).unwrap();
    let has_provider = re.is_match(&raw);
    let has_section = raw.contains("[model_providers.gemini_adc]");
    Ok(has_provider && has_section)
}

pub fn configure_codex_routing(
    enable: bool,
    primary_model: &str,
    reasoning_effort: &str,
    state: &mut Map<String, Value>,
) -> Result<(), String> {
    let paths = get_paths();
    let raw = read_file_safely(&paths.codex_config)?.unwrap_or_default();
    let already_routed = is_codex_routed()?;

    let cleaned = strip_codex_managed_block(&raw);
    let mut lines: Vec<String> = cleaned.lines().map(|s| s.to_string()).collect();

    if enable {
        if !already_routed
            && state
                .get("codex_backup")
                .map(|v| v.is_null())
                .unwrap_or(true)
        {
            let mut backup = Map::new();
            backup.insert(
                "model_provider".to_string(),
                extract_top_level_toml_key(&lines, "model_provider")
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            backup.insert(
                "model".to_string(),
                extract_top_level_toml_key(&lines, "model")
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            backup.insert(
                "model_reasoning_effort".to_string(),
                extract_top_level_toml_key(&lines, "model_reasoning_effort")
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            state.insert("codex_backup".to_string(), Value::Object(backup));
        }

        lines = remove_top_level_toml_keys(
            &lines,
            &["model_provider", "model", "model_reasoning_effort"],
        );
        while !lines.is_empty() && lines[0].trim().is_empty() {
            lines.remove(0);
        }

        let mut body_lines = vec![
            r#"model_provider = "gemini_adc""#.to_string(),
            format!(r#"model = "{}""#, primary_model),
            format!(r#"model_reasoning_effort = "{}""#, reasoning_effort),
        ];
        if !lines.is_empty() {
            body_lines.push(String::new());
            body_lines.extend(lines);
        }
        let body = body_lines.join("\n").trim_end().to_string();
        let managed_block = [
            CODEX_BLOCK_BEGIN.to_string(),
            "[model_providers.gemini_adc]".to_string(),
            r#"name = "Gemini ADC Router (Vertex AI)""#.to_string(),
            format!(r#"base_url = "{}/v1""#, router_url()),
            r#"wire_api = "responses""#.to_string(),
            CODEX_BLOCK_END.to_string(),
        ]
        .join("\n");

        let full_text = format!("{}\n\n{}\n", body, managed_block);
        write_file_safely(&paths.codex_config, &full_text, 0o600)
    } else {
        lines = remove_top_level_toml_keys(
            &lines,
            &["model_provider", "model", "model_reasoning_effort"],
        );
        let mut restored_top = Vec::new();
        if let Some(Value::Object(backup)) = state.get("codex_backup").cloned() {
            for k in ["model_provider", "model", "model_reasoning_effort"] {
                if let Some(Value::String(val)) = backup.get(k) {
                    if !val.trim().is_empty() {
                        restored_top.push(val.clone());
                    }
                }
            }
            state.insert("codex_backup".to_string(), Value::Null);
        }

        while !lines.is_empty() && lines[0].trim().is_empty() {
            lines.remove(0);
        }

        let mut combined = Vec::new();
        combined.extend(restored_top.clone());
        if !restored_top.is_empty() && !lines.is_empty() {
            combined.push(String::new());
        }
        combined.extend(lines);
        let mut full_text = combined.join("\n").trim().to_string();
        if !full_text.is_empty() {
            full_text.push('\n');
        }
        write_file_safely(&paths.codex_config, &full_text, 0o600)
    }
}

pub fn local_http_request(
    method: &str,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> Option<(u16, String)> {
    let url = router_url();
    let stripped = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(&url);
    let host_port = stripped.split('/').next().unwrap_or("127.0.0.1:4000");
    let addr: std::net::SocketAddr = host_port.parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout).ok()?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let payload = body.unwrap_or("");
    let req = if method == "POST" {
        format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            path,
            host_port,
            payload.len(),
            payload
        )
    } else {
        format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            path, host_port
        )
    };
    stream.write_all(req.as_bytes()).ok()?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp).ok()?;
    let (head, body_part) = resp.split_once("\r\n\r\n")?;
    let status_line = head.lines().next()?;
    let status_code: u16 = status_line.split_whitespace().nth(1)?.parse().ok()?;
    Some((status_code, body_part.to_string()))
}

pub fn fetch_router_health(timeout: Duration) -> Option<Map<String, Value>> {
    let (status, body) = local_http_request("GET", "/health", None, timeout)?;
    if status == 200 {
        if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&body) {
            return Some(m);
        }
    }
    None
}

pub fn push_router_config(state: &Map<String, Value>, hot_swap: bool) -> bool {
    let primary_model = state
        .get("primary_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash");
    let reasoning_effort = state
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let fast_model = state
        .get("fast_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash");
    let fallback_model = state.get("fallback_model").cloned().unwrap_or(Value::Null);
    let available_models = get_state_available_models(state);
    let payload = json!({
        "primary_model": primary_model,
        "reasoning_effort": reasoning_effort,
        "fast_model": fast_model,
        "fallback_model": fallback_model,
        "available_models": available_models,
        "hot_swap": hot_swap,
    })
    .to_string();
    matches!(
        local_http_request("POST", "/config", Some(&payload), Duration::from_secs(2)),
        Some((200, _))
    )
}

pub fn is_os_service_enabled() -> bool {
    if is_mock_daemon() {
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let plist = home_dir()
            .join("Library")
            .join("LaunchAgents")
            .join("com.gemini.adc.router.plist");
        plist.is_file()
    }

    #[cfg(target_os = "windows")]
    {
        let (rc, out, _) = run_cmd(
            &[
                "reg",
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "GeminiADCRouter",
            ],
            Duration::from_secs(3),
        );
        rc == 0 && out.contains("GeminiADCRouter")
    }

    #[cfg(target_os = "linux")]
    {
        let (rc, out, _) = run_cmd(
            &[
                "systemctl",
                "--user",
                "is-enabled",
                "claude-gemini-router.service",
            ],
            Duration::from_secs(3),
        );
        rc == 0 && out.trim().contains("enabled")
    }
}

pub fn is_os_service_active() -> bool {
    if is_mock_daemon() {
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        let (rc, out, _) = run_cmd(
            &["launchctl", "list", "com.gemini.adc.router"],
            Duration::from_secs(3),
        );
        rc == 0 && out.contains("\"PID\"")
    }

    #[cfg(target_os = "windows")]
    {
        let (rc, out, _) = run_cmd(
            &[
                "tasklist",
                "/FI",
                "IMAGENAME eq claude-gemini-router.exe",
                "/NH",
            ],
            Duration::from_secs(3),
        );
        rc == 0 && out.to_lowercase().contains("claude-gemini-router.exe")
    }

    #[cfg(target_os = "linux")]
    {
        let (rc, out, _) = run_cmd(
            &[
                "systemctl",
                "--user",
                "is-active",
                "claude-gemini-router.service",
            ],
            Duration::from_secs(3),
        );
        rc == 0 && out.trim() == "active"
    }
}

fn find_router_binary(paths: &Paths) -> Option<PathBuf> {
    let home = home_dir();
    let bin_name = if cfg!(windows) {
        "claude-gemini-router.exe"
    } else {
        "claude-gemini-router"
    };
    let mut candidates = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(bin_name));
        }
    }
    candidates.push(home.join(".local").join("bin").join(bin_name));
    candidates.push(
        paths
            .project_dir
            .join("target")
            .join("release")
            .join(bin_name),
    );
    candidates.push(
        paths
            .project_dir
            .join("target")
            .join("debug")
            .join(bin_name),
    );

    candidates.into_iter().find(|p| p.is_file())
}

pub fn start_router_daemon(state: &Map<String, Value>) -> (bool, String) {
    if is_mock_daemon() {
        return (true, "mock-daemon-started".to_string());
    }

    let primary_model = state
        .get("primary_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash");
    let reasoning_effort = state
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let fast_model = state
        .get("fast_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash");
    let avail_csv = get_state_available_models(state).join(",");

    if let Some(health) = fetch_router_health(Duration::from_secs(1)) {
        if health.get("status").and_then(|v| v.as_str()) == Some("ok") {
            push_router_config(state, true);
            return (true, "already-running".to_string());
        }
    }

    #[cfg(target_os = "macos")]
    {
        let plist = home_dir()
            .join("Library")
            .join("LaunchAgents")
            .join("com.gemini.adc.router.plist");
        if plist.is_file() {
            let _ = run_cmd(
                &["launchctl", "load", "-w", &plist.to_string_lossy()],
                Duration::from_secs(3),
            );
            let (rc, _, _) = run_cmd(
                &["launchctl", "start", "com.gemini.adc.router"],
                Duration::from_secs(4),
            );
            if rc == 0 {
                for _ in 0..20 {
                    if fetch_router_health(Duration::from_millis(500)).is_some() {
                        push_router_config(state, true);
                        return (true, "launchd".to_string());
                    }
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let (rc, _, _) = run_cmd(
            &[
                "systemctl",
                "--user",
                "start",
                "claude-gemini-router.service",
            ],
            Duration::from_secs(5),
        );
        if rc == 0 {
            for _ in 0..20 {
                if fetch_router_health(Duration::from_millis(500)).is_some() {
                    push_router_config(state, true);
                    return (true, "systemd".to_string());
                }
                thread::sleep(Duration::from_millis(200));
            }
        }
    }

    let paths = get_paths();
    let Some(router_bin) = find_router_binary(&paths) else {
        return (false, "claude-gemini-router binary not found".to_string());
    };

    let _ = fs::create_dir_all(&paths.config_dir);
    let log_path = paths.config_dir.join("router.log");
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let Ok(log_file) = opts.open(&log_path) else {
        return (false, "Failed to open router.log".to_string());
    };
    let Ok(log_err) = log_file.try_clone() else {
        return (false, "Failed to clone log fd".to_string());
    };

    let port_s = router_port().to_string();
    let mut cmd = Command::new(&router_bin);
    cmd.args(["--host", "127.0.0.1", "--port", &port_s])
        .env("GEMINI_PRIMARY_MODEL", primary_model)
        .env("GEMINI_REASONING_EFFORT", reasoning_effort)
        .env("GEMINI_FAST_MODEL", fast_model)
        .env("GEMINI_AVAILABLE_MODELS", &avail_csv)
        .env("GEMINI_HOT_SWAP_ACTIVE", "1")
        .env("ROUTER_PORT", &port_s)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_err));
    if let Some(fb) = state
        .get("fallback_model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        cmd.env("GEMINI_FALLBACK_MODEL", fb);
    }

    if paths.project_dir.is_dir() {
        cmd.current_dir(&paths.project_dir);
    }
    configure_background_command(&mut cmd);

    match cmd.spawn() {
        Ok(child) => {
            let _ = write_file_safely(&paths.pid_file, &format!("{}\n", child.id()), 0o600);
        }
        Err(e) => return (false, format!("Failed to spawn router: {}", e)),
    }

    for _ in 0..25 {
        if fetch_router_health(Duration::from_millis(500)).is_some() {
            push_router_config(state, true);
            return (true, "background-process".to_string());
        }
        thread::sleep(Duration::from_millis(200));
    }

    (
        false,
        "Router failed to respond on /health within timeout".to_string(),
    )
}

pub fn stop_router_daemon() -> (bool, String) {
    if is_mock_daemon() {
        return (true, "mock-daemon-stopped".to_string());
    }

    let paths = get_paths();

    #[cfg(target_os = "macos")]
    {
        let _ = run_cmd(
            &["launchctl", "stop", "com.gemini.adc.router"],
            Duration::from_secs(4),
        );
    }

    #[cfg(target_os = "linux")]
    {
        let _ = run_cmd(
            &[
                "systemctl",
                "--user",
                "stop",
                "claude-gemini-router.service",
            ],
            Duration::from_secs(5),
        );
    }

    if let Ok(Some(pid_str)) = read_file_safely(&paths.pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            #[cfg(windows)]
            {
                let pid_s = pid.to_string();
                let _ = run_cmd(&["taskkill", "/F", "/PID", &pid_s], Duration::from_secs(3));
            }
        }
        let _ = fs::remove_file(&paths.pid_file);
    }

    #[cfg(unix)]
    {
        let uid_s = current_uid().to_string();
        let (rc, out, _) = run_cmd(
            &["pgrep", "-u", &uid_s, "-f", "claude-gemini-router"],
            Duration::from_secs(3),
        );
        if rc == 0 && !out.trim().is_empty() {
            let my_pid = std::process::id() as i32;
            for line in out.lines() {
                if let Ok(pid) = line.trim().parse::<i32>() {
                    if pid != my_pid {
                        unsafe {
                            libc::kill(pid, libc::SIGTERM);
                        }
                    }
                }
            }
        }
    }

    #[cfg(windows)]
    {
        let _ = run_cmd(
            &["taskkill", "/F", "/IM", "claude-gemini-router.exe"],
            Duration::from_secs(3),
        );
    }

    for _ in 0..10 {
        if fetch_router_health(Duration::from_millis(400)).is_none() {
            return (true, "stopped".to_string());
        }
        thread::sleep(Duration::from_millis(200));
    }
    (
        false,
        "Router is still responding after stop attempt".to_string(),
    )
}

pub fn probe_adc_status_offline() -> Value {
    let adc_file = ADCAuthManager::adc_file_path();
    let has_adc_file = adc_file.is_file();

    let project_id =
        ADCAuthManager::read_project_id_from_disk().unwrap_or_else(|| "unknown".to_string());

    json!({
        "adc_mode": true,
        "api_key_used": false,
        "credentials_type": if has_adc_file { Value::String("UserCredentials".to_string()) } else { Value::Null },
        "project_id": if project_id.is_empty() { "unknown".to_string() } else { project_id },
        "location": env::var("GOOGLE_CLOUD_LOCATION").unwrap_or_else(|_| "global".to_string()),
        "token_valid": has_adc_file,
        "error": if has_adc_file { Value::Null } else { Value::String("ADC credentials file not found".to_string()) },
    })
}

pub fn resolve_binary_in_path(binary: &str) -> Option<PathBuf> {
    let search_path = enriched_path_env();
    let sep = if cfg!(windows) { ';' } else { ':' };
    let extensions: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat", ".ps1"]
    } else {
        &[""]
    };

    for dir in search_path.split(sep) {
        if dir.is_empty() {
            continue;
        }
        for ext in extensions {
            let candidate = Path::new(dir).join(format!("{}{}", binary, ext));
            if let Ok(meta) = fs::metadata(&candidate) {
                if meta.is_file() {
                    #[cfg(unix)]
                    {
                        if meta.permissions().mode() & 0o111 != 0 {
                            return Some(candidate);
                        }
                    }
                    #[cfg(windows)]
                    {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

pub fn which_in_path(binary: &str) -> bool {
    resolve_binary_in_path(binary).is_some()
}

pub fn send_desktop_notification(summary: &str, body: &str, urgency: &str) {
    if env::var("OMARCHY_GEMINI_ROUTER_NO_NOTIFY").as_deref() == Ok("1") {
        return;
    }
    if let Some(bin) = resolve_binary_in_path("omarchy-notification-send") {
        let _ = Command::new(bin)
            .args([
                "--app-name",
                "Gemini ADC Router",
                "--urgency",
                urgency,
                "--",
                summary,
                body,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    } else if let Some(bin) = resolve_binary_in_path("notify-send") {
        let _ = Command::new(bin)
            .args([
                "-a",
                "Gemini ADC Router",
                "-u",
                urgency,
                "--",
                summary,
                body,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

pub fn strip_menu_managed_block(raw: &str) -> String {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in raw.lines() {
        if line.trim() == MENU_BLOCK_BEGIN.trim() {
            in_block = true;
            continue;
        }
        if in_block {
            if line.trim() == MENU_BLOCK_END.trim() {
                in_block = false;
            }
            continue;
        }
        out.push(line);
    }
    let mut joined = out.join("\n");
    if raw.ends_with('\n') && !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

pub fn is_menu_enabled() -> Result<bool, String> {
    let paths = get_paths();
    let Some(raw) = read_file_safely(&paths.menu_config)? else {
        return Ok(false);
    };
    Ok(raw.contains(MENU_BLOCK_BEGIN) && raw.contains(r#""trigger.gemini-router""#))
}

fn has_jsonc_entries_after_brace(raw: &str, open_brace_idx: usize) -> bool {
    let rest = &raw[open_brace_idx + 1..];
    for line in rest.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with('}') {
            return false;
        }
        return true;
    }
    false
}

pub fn configure_menu_entry(enable: bool) -> Result<bool, String> {
    let paths = get_paths();
    let raw = read_file_safely(&paths.menu_config)?.unwrap_or_else(|| "{\n}\n".to_string());
    let cleaned = strip_menu_managed_block(&raw);
    if !enable {
        if raw != cleaned || paths.menu_config.is_file() {
            write_file_safely(&paths.menu_config, &cleaned, 0o600)?;
        }
        return Ok(false);
    }

    let open_brace_idx = cleaned
        .find('{')
        .ok_or_else(|| "Invalid omarchy-menu.jsonc: missing opening '{'".to_string())?;
    let has_following = has_jsonc_entries_after_brace(&cleaned, open_brace_idx);
    let trailing_comma = if has_following { "," } else { "" };

    let entries = [
        MENU_BLOCK_BEGIN.to_string(),
        r#"  "trigger.gemini-router": {"icon": "󰚩", "label": "Gemini ADC Router", "description": "Route Claude Code & OpenAI Codex to Vertex AI Gemini", "aliases": ["gemini", "router", "adc", "vertex"]},"#.to_string(),
        r#"  "trigger.gemini-router.toggle-both": {"icon": "󰚩", "label": "Toggle Both (Claude + Codex)", "aliases": ["gemini-toggle"], "action": "omarchy-gemini-router-ctl toggle --target both --notify"},"#.to_string(),
        r#"  "trigger.gemini-router.toggle-claude": {"icon": "󰚩", "label": "Toggle Claude Code Routing", "action": "omarchy-gemini-router-ctl toggle --target claude --notify"},"#.to_string(),
        r#"  "trigger.gemini-router.toggle-codex": {"icon": "󰚩", "label": "Toggle OpenAI Codex Routing", "action": "omarchy-gemini-router-ctl toggle --target codex --notify"},"#.to_string(),
        r#"  "trigger.gemini-router.flip-model": {"icon": "󰁯", "label": "Switch Active Gemini Model", "aliases": ["gemini-model"], "action": "omarchy-gemini-router-ctl set-model --flip --notify"},"#.to_string(),
        format!(
            r#"  "trigger.gemini-router.adc-login": {{"icon": "󰌋", "label": "Reset Vertex AI ADC Credentials", "action": "omarchy-gemini-router-ctl adc-login --notify"}}{}"#,
            trailing_comma
        ),
        MENU_BLOCK_END.to_string(),
    ]
    .join("\n");

    let mut updated = String::with_capacity(cleaned.len() + entries.len() + 4);
    updated.push_str(&cleaned[..=open_brace_idx]);
    updated.push('\n');
    updated.push_str(&entries);
    if !cleaned[open_brace_idx + 1..].starts_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&cleaned[open_brace_idx + 1..]);
    write_file_safely(&paths.menu_config, &updated, 0o600)?;
    Ok(true)
}

pub fn cmd_menu(action: &str) -> Result<Map<String, Value>, String> {
    let enabled = match action {
        "enable" => configure_menu_entry(true)?,
        "disable" => configure_menu_entry(false)?,
        "toggle" => {
            let cur = is_menu_enabled()?;
            configure_menu_entry(!cur)?
        }
        "status" => is_menu_enabled()?,
        other => {
            return Err(format!(
                "Invalid menu action '{}'. Use --enable, --disable, --toggle, or --status",
                other
            ))
        }
    };
    let mut res = build_status_dict()?;
    res.insert("menu_enabled".to_string(), Value::Bool(enabled));
    Ok(res)
}

pub fn build_status_dict() -> Result<Map<String, Value>, String> {
    let mut state = load_state()?;
    let health = if is_mock_daemon() {
        None
    } else {
        fetch_router_health(Duration::from_millis(1200))
    };
    let mut router_running = health
        .as_ref()
        .and_then(|h| h.get("status"))
        .and_then(|v| v.as_str())
        == Some("ok");

    if is_mock_daemon() {
        router_running = state
            .get("mock_router_running")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
    } else if !router_running {
        router_running = is_os_service_active();
    }

    let mut default_stats = serde_json::from_value::<Map<String, Value>>(json!({
        "total_requests": 0,
        "claude_requests": 0,
        "codex_requests": 0,
        "chat_requests": 0,
        "input_tokens": 0,
        "output_tokens": 0,
        "reasoning_tokens": 0,
        "cached_tokens": 0,
        "total_tokens": 0,
        "fallback_count": 0,
        "error_count": 0,
        "last_client": Value::Null,
        "last_requested_model": Value::Null,
        "last_vertex_model": Value::Null,
        "last_latency_ms": Value::Null,
        "last_request_at": Value::Null,
    }))
    .unwrap();

    let state_primary = state
        .get("primary_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash")
        .to_string();
    let state_effort = state
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or("medium")
        .to_string();

    let (adc_info, primary_model, reasoning_effort, hot_swap_active, stats_info) =
        if let Some(ref h) = health {
            if let Some(adc_obj) = h.get("adc").filter(|v| v.is_object()) {
                let mut pm = h
                    .get("primary_model")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&state_primary)
                    .to_string();
                let mut re = h
                    .get("reasoning_effort")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&state_effort)
                    .to_string();
                let mut hs = h
                    .get("hot_swap_active")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !hs || pm != state_primary || re != state_effort {
                    push_router_config(&state, true);
                    pm = state_primary;
                    re = state_effort;
                    hs = true;
                }
                if let Some(Value::Object(raw_stats)) = h.get("stats") {
                    for (k, v) in raw_stats {
                        default_stats.insert(k.clone(), v.clone());
                    }
                }
                (adc_obj.clone(), pm, re, hs, Value::Object(default_stats))
            } else {
                (
                    probe_adc_status_offline(),
                    state_primary,
                    state_effort,
                    false,
                    Value::Object(default_stats),
                )
            }
        } else {
            (
                probe_adc_status_offline(),
                state_primary,
                state_effort,
                false,
                Value::Object(default_stats),
            )
        };

    let claude_on = is_claude_routed()?;
    let codex_on = is_codex_routed()?;

    if claude_on {
        let settings = read_file_safely(&get_paths().claude_settings)
            .ok()
            .flatten()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .unwrap_or_else(|| json!({}));
        let needs_claude_heal = settings.get("model").and_then(|v| v.as_str())
            != Some(primary_model.as_str())
            || settings
                .get("env")
                .and_then(|v| v.as_object())
                .is_some_and(|env| {
                    env.contains_key("DISABLE_PROMPT_CACHING")
                        || env.contains_key("DISABLE_PROMPT_CACHING_HAIKU")
                        || env.contains_key("DISABLE_PROMPT_CACHING_SONNET")
                        || env.contains_key("DISABLE_PROMPT_CACHING_OPUS")
                        || env.get("ANTHROPIC_MODEL").and_then(|v| v.as_str())
                            != Some(primary_model.as_str())
                });
        if needs_claude_heal {
            let _ = configure_claude_routing(true, &primary_model, &mut state);
            let _ = save_state(&state);
        }
    }

    let mut available_models = get_state_available_models(&state);
    if !available_models.contains(&primary_model) {
        available_models.push(primary_model.clone());
    }
    let model_presets: Vec<Value> = available_models
        .iter()
        .map(|m| {
            json!({
                "id": m,
                "label": format_model_display_name(m),
                "short": format_model_short_label(m),
                "badge": format_model_short_badge(m),
            })
        })
        .collect();

    let short_model = format_model_short_label(&primary_model);
    let short_badge = format_model_short_badge(&primary_model);
    let (status_label, badge_text) = if claude_on && codex_on {
        (
            format!("Claude + Codex -> {}", short_model),
            format!("C+X -> {}", short_badge),
        )
    } else if claude_on {
        (
            format!("Claude -> {}", short_model),
            format!("Claude -> {}", short_badge),
        )
    } else if codex_on {
        (
            format!("Codex -> {}", short_model),
            format!("Codex -> {}", short_badge),
        )
    } else {
        ("Direct (Routing Off)".to_string(), "ADC Off".to_string())
    };

    let fast_model = state
        .get("fast_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash")
        .to_string();
    let fallback_model = state.get("fallback_model").cloned().unwrap_or(Value::Null);
    let menu_enabled = is_menu_enabled().unwrap_or(false);

    let val = json!({
        "ok": true,
        "platform": std::env::consts::OS,
        "router_running": router_running,
        "service_enabled": is_os_service_enabled(),
        "router_url": router_url(),
        "router_port": router_port(),
        "claude_enabled": claude_on,
        "codex_enabled": codex_on,
        "any_enabled": claude_on || codex_on,
        "both_enabled": claude_on && codex_on,
        "claude_installed": which_in_path("claude"),
        "codex_installed": which_in_path("codex"),
        "menu_enabled": menu_enabled,
        "primary_model": primary_model,
        "primary_model_display": format_model_display_name(&primary_model),
        "primary_model_short": short_model,
        "primary_model_badge": short_badge,
        "available_models": available_models,
        "model_presets": model_presets,
        "fast_model": fast_model,
        "fallback_model": fallback_model,
        "reasoning_effort": reasoning_effort,
        "hot_swap_active": hot_swap_active,
        "status_label": status_label,
        "badge_text": badge_text,
        "adc": adc_info,
        "stats": stats_info,
    });
    Ok(val.as_object().unwrap().clone())
}

pub fn cmd_enable(
    target: &str,
    model_arg: Option<&str>,
    effort_arg: Option<&str>,
) -> Result<Map<String, Value>, String> {
    if !VALID_TARGETS.contains(&target) {
        return Err(format!(
            "Invalid target '{}'. Must be one of {:?}",
            target, VALID_TARGETS
        ));
    }

    let mut state = load_state()?;
    if let Some(m) = model_arg {
        let norm = normalize_model(Some(m))?;
        state.insert("primary_model".to_string(), Value::String(norm.clone()));
        ensure_model_in_state_available(&mut state, &norm);
    }
    if let Some(e) = effort_arg {
        state.insert(
            "reasoning_effort".to_string(),
            Value::String(normalize_effort(Some(e))?),
        );
    }

    let primary_model = state
        .get("primary_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash")
        .to_string();
    let reasoning_effort = state
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or("medium")
        .to_string();

    if target == "claude" || target == "both" {
        configure_claude_routing(true, &primary_model, &mut state)?;
    }
    if target == "codex" || target == "both" {
        configure_codex_routing(true, &primary_model, &reasoning_effort, &mut state)?;
    }

    state.insert("mock_router_running".to_string(), Value::Bool(true));
    save_state(&state)?;

    let (started, mode) = start_router_daemon(&state);
    push_router_config(&state, true);
    let mut res = build_status_dict()?;
    res.insert("daemon_mode".to_string(), Value::String(mode.clone()));
    if !started {
        res.insert(
            "warning".to_string(),
            Value::String(format!(
                "Routing config enabled, but router daemon reported: {}",
                mode
            )),
        );
    }
    Ok(res)
}

pub fn cmd_disable(target: &str) -> Result<Map<String, Value>, String> {
    if !VALID_TARGETS.contains(&target) {
        return Err(format!(
            "Invalid target '{}'. Must be one of {:?}",
            target, VALID_TARGETS
        ));
    }

    let mut state = load_state()?;
    let primary_model = state
        .get("primary_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash")
        .to_string();
    let reasoning_effort = state
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or("medium")
        .to_string();

    if target == "claude" || target == "both" {
        configure_claude_routing(false, &primary_model, &mut state)?;
    }
    if target == "codex" || target == "both" {
        configure_codex_routing(false, &primary_model, &reasoning_effort, &mut state)?;
    }
    save_state(&state)?;
    build_status_dict()
}

pub fn cmd_toggle(target: &str) -> Result<Map<String, Value>, String> {
    if !VALID_TARGETS.contains(&target) {
        return Err(format!(
            "Invalid target '{}'. Must be one of {:?}",
            target, VALID_TARGETS
        ));
    }

    let claude_on = is_claude_routed()?;
    let codex_on = is_codex_routed()?;
    let enable = match target {
        "claude" => !claude_on,
        "codex" => !codex_on,
        _ => !(claude_on && codex_on),
    };
    if enable {
        cmd_enable(target, None, None)
    } else {
        cmd_disable(target)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_set_model(
    model_arg: Option<&str>,
    effort_arg: Option<&str>,
    flip: bool,
    add_model_arg: Option<&str>,
    remove_model_arg: Option<&str>,
    models_csv_arg: Option<&str>,
    fast_model_arg: Option<&str>,
    fallback_model_arg: Option<&str>,
) -> Result<Map<String, Value>, String> {
    let mut state = load_state()?;

    if let Some(csv) = models_csv_arg {
        let mut parsed = Vec::new();
        for item in csv.split(',') {
            let trimmed = item.trim();
            if !trimmed.is_empty() {
                let norm = normalize_model(Some(trimmed))?;
                if !parsed.contains(&norm) {
                    parsed.push(norm);
                }
            }
        }
        if parsed.is_empty() {
            return Err("At least one valid model is required in --models".to_string());
        }
        state.insert(
            "available_models".to_string(),
            Value::Array(parsed.into_iter().map(Value::String).collect()),
        );
    }

    if let Some(add_m) = add_model_arg {
        let norm = normalize_model(Some(add_m))?;
        ensure_model_in_state_available(&mut state, &norm);
    }

    if let Some(rem_m) = remove_model_arg {
        let norm = normalize_model(Some(rem_m))?;
        let mut avail = get_state_available_models(&state);
        avail.retain(|x| x != &norm);
        if avail.is_empty() {
            avail = default_available_models();
        }
        state.insert(
            "available_models".to_string(),
            Value::Array(avail.into_iter().map(Value::String).collect()),
        );
    }

    if let Some(fm) = fast_model_arg {
        let norm = normalize_model(Some(fm))?;
        state.insert("fast_model".to_string(), Value::String(norm));
    }

    if let Some(fb) = fallback_model_arg {
        let trimmed = fb.trim().to_lowercase();
        if trimmed.is_empty() || trimmed == "none" || trimmed == "off" {
            state.insert("fallback_model".to_string(), Value::Null);
        } else {
            let norm = normalize_model(Some(fb))?;
            state.insert("fallback_model".to_string(), Value::String(norm));
        }
    }

    if flip {
        let current = state
            .get("primary_model")
            .and_then(|v| v.as_str())
            .unwrap_or("gemini-3.8-flash")
            .to_string();
        let avail = get_state_available_models(&state);
        let next = if avail.len() >= 2 {
            let idx = avail.iter().position(|m| m == &current).unwrap_or(0);
            avail[(idx + 1) % avail.len()].clone()
        } else if current.contains('4') {
            "gemini-3.8-flash".to_string()
        } else {
            "gemini-4-argon".to_string()
        };
        state.insert("primary_model".to_string(), Value::String(next.clone()));
        ensure_model_in_state_available(&mut state, &next);
    } else if let Some(m) = model_arg {
        let norm = normalize_model(Some(m))?;
        state.insert("primary_model".to_string(), Value::String(norm.clone()));
        ensure_model_in_state_available(&mut state, &norm);
    }
    if let Some(e) = effort_arg {
        state.insert(
            "reasoning_effort".to_string(),
            Value::String(normalize_effort(Some(e))?),
        );
    }

    let primary_model = state
        .get("primary_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash")
        .to_string();
    let reasoning_effort = state
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or("medium")
        .to_string();

    if is_claude_routed()? {
        configure_claude_routing(true, &primary_model, &mut state)?;
    }
    if is_codex_routed()? {
        configure_codex_routing(true, &primary_model, &reasoning_effort, &mut state)?;
    }

    save_state(&state)?;
    push_router_config(&state, true);
    build_status_dict()
}

pub fn cmd_start_router() -> Result<Map<String, Value>, String> {
    let mut state = load_state()?;
    state.insert("mock_router_running".to_string(), Value::Bool(true));
    save_state(&state)?;
    let (ok, detail) = start_router_daemon(&state);
    let mut res = build_status_dict()?;
    res.insert("ok".to_string(), Value::Bool(ok));
    res.insert("detail".to_string(), Value::String(detail.clone()));
    if !ok {
        res.insert("error".to_string(), Value::String(detail));
    }
    Ok(res)
}

pub fn cmd_stop_router() -> Result<Map<String, Value>, String> {
    let mut state = load_state()?;
    state.insert("mock_router_running".to_string(), Value::Bool(false));
    save_state(&state)?;
    let (ok, detail) = stop_router_daemon();
    let mut res = build_status_dict()?;
    res.insert("ok".to_string(), Value::Bool(ok));
    res.insert("detail".to_string(), Value::String(detail));
    Ok(res)
}

pub fn cmd_restart_router() -> Result<Map<String, Value>, String> {
    let _ = stop_router_daemon();
    let mut state = load_state()?;
    state.insert("mock_router_running".to_string(), Value::Bool(true));
    save_state(&state)?;
    let (ok, detail) = start_router_daemon(&state);
    let mut res = build_status_dict()?;
    res.insert("ok".to_string(), Value::Bool(ok));
    res.insert("detail".to_string(), Value::String(detail));
    Ok(res)
}

pub fn cmd_logs(lines: usize) -> Result<Map<String, Value>, String> {
    let clamped = lines.clamp(1, 500);
    let paths = get_paths();
    let log_file = paths.config_dir.join("router.log");

    let mut entries: Vec<String> = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let lines_s = clamped.to_string();
        let (rc, out, _) = run_cmd(
            &[
                "journalctl",
                "--user",
                "-u",
                "claude-gemini-router.service",
                "-n",
                &lines_s,
                "--no-pager",
            ],
            Duration::from_secs(4),
        );
        if rc == 0 && !out.trim().is_empty() && !out.contains("-- No entries --") {
            let all: Vec<String> = out.trim().lines().map(|s| s.to_string()).collect();
            let start = all.len().saturating_sub(clamped);
            entries = all[start..].to_vec();
        }
    }

    if entries.is_empty() && log_file.is_file() {
        let raw = read_file_safely(&log_file)?.unwrap_or_default();
        let all: Vec<String> = raw.trim().lines().map(|s| s.to_string()).collect();
        let start = all.len().saturating_sub(clamped);
        entries = all[start..].to_vec();
    }

    let text = if entries.is_empty() {
        "No router log entries yet.".to_string()
    } else {
        entries.join("\n")
    };
    let val = json!({
        "ok": true,
        "lines": entries,
        "text": text,
    });
    Ok(val.as_object().unwrap().clone())
}

pub fn cmd_refresh_auth() -> Result<Map<String, Value>, String> {
    let _ = local_http_request(
        "POST",
        "/config",
        Some(r#"{"refresh_auth":true}"#),
        Duration::from_secs(2),
    );
    build_status_dict()
}

pub fn cmd_adc_login() -> Result<Map<String, Value>, String> {
    let gcloud_path = ADCAuthManager::find_gcloud_binary();
    if !gcloud_path.is_file() && !which_in_path("gcloud") {
        let val = json!({
            "ok": false,
            "launched": false,
            "error": "gcloud CLI not found. Install google-cloud-sdk first.",
        });
        return Ok(val.as_object().unwrap().clone());
    }
    let gcloud_bin = gcloud_path.to_string_lossy().to_string();
    let refresh_url = format!("{}/config", router_url());

    #[cfg(target_os = "macos")]
    {
        let login_script = format!(
            "'{}' auth application-default login; curl -s -X POST '{}' -H 'Content-Type: application/json' -d '{{\"refresh_auth\":true}}' >/dev/null 2>&1 || true",
            gcloud_bin.replace('\'', "'\\''"),
            refresh_url
        );
        let applescript = format!(
            "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
            login_script.replace('\\', "\\\\").replace('"', "\\\"")
        );
        if Command::new("osascript")
            .args(["-e", &applescript])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            let _ = local_http_request(
                "POST",
                "/config",
                Some(r#"{"refresh_auth":true}"#),
                Duration::from_secs(1),
            );
            let val = json!({
                "ok": true,
                "launched": true,
                "command": "osascript Terminal.app gcloud auth application-default login",
            });
            return Ok(val.as_object().unwrap().clone());
        }
    }

    #[cfg(target_os = "windows")]
    {
        let ps_cmd = format!(
            "& '{}' auth application-default login; try {{ Invoke-RestMethod -Method Post -Uri '{}' -ContentType 'application/json' -Body '{{\"refresh_auth\":true}}' | Out-Null }} catch {{}}",
            gcloud_bin.replace('\'', "''"),
            refresh_url
        );
        // Prefer Windows Terminal (wt.exe) if installed, fallback to cmd/powershell start
        let launched = if which_in_path("wt") {
            Command::new("wt.exe")
                .args(["powershell.exe", "-NoExit", "-Command", &ps_cmd])
                .spawn()
                .is_ok()
        } else {
            Command::new("cmd.exe")
                .args([
                    "/C",
                    "start",
                    "powershell.exe",
                    "-NoExit",
                    "-Command",
                    &ps_cmd,
                ])
                .spawn()
                .is_ok()
        };
        if launched {
            let _ = local_http_request(
                "POST",
                "/config",
                Some(r#"{"refresh_auth":true}"#),
                Duration::from_secs(1),
            );
            let val = json!({
                "ok": true,
                "launched": true,
                "command": "powershell gcloud auth application-default login",
            });
            return Ok(val.as_object().unwrap().clone());
        }
    }

    let login_script = format!(
        "'{}' auth application-default login; curl -s -X POST '{}' -H 'Content-Type: application/json' -d '{{\"refresh_auth\":true}}' >/dev/null 2>&1 || true",
        gcloud_bin.replace('\'', "'\\''"),
        refresh_url
    );

    let candidates: Vec<Vec<String>> = vec![
        vec![
            "uwsm-app".to_string(),
            "--".to_string(),
            "xdg-terminal-exec".to_string(),
            "sh".to_string(),
            "-c".to_string(),
            login_script.clone(),
        ],
        vec![
            "xdg-terminal-exec".to_string(),
            "sh".to_string(),
            "-c".to_string(),
            login_script.clone(),
        ],
        vec![
            "ghostty".to_string(),
            "-e".to_string(),
            "sh".to_string(),
            "-c".to_string(),
            login_script.clone(),
        ],
        vec![
            "kitty".to_string(),
            "-e".to_string(),
            "sh".to_string(),
            "-c".to_string(),
            login_script.clone(),
        ],
        vec![
            "alacritty".to_string(),
            "-e".to_string(),
            "sh".to_string(),
            "-c".to_string(),
            login_script,
        ],
    ];

    let enriched_path = enriched_path_env();
    for term_cmd in candidates {
        if let Some(launcher_bin) = resolve_binary_in_path(&term_cmd[0]) {
            if Command::new(launcher_bin)
                .args(&term_cmd[1..])
                .env("PATH", &enriched_path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_ok()
            {
                let _ = local_http_request(
                    "POST",
                    "/config",
                    Some(r#"{"refresh_auth":true}"#),
                    Duration::from_secs(1),
                );
                let val = json!({
                    "ok": true,
                    "launched": true,
                    "command": term_cmd.join(" "),
                });
                return Ok(val.as_object().unwrap().clone());
            }
        }
    }

    let val = json!({
        "ok": false,
        "launched": false,
        "error": "No terminal emulator found. Run: gcloud auth application-default login",
    });
    Ok(val.as_object().unwrap().clone())
}

pub fn dispatch_args(args: &[String]) -> Result<Map<String, Value>, String> {
    if args.is_empty() {
        return build_status_dict();
    }

    let sub = args[0].as_str();
    let mut target = "both".to_string();
    let mut model: Option<String> = None;
    let mut effort: Option<String> = None;
    let mut flip = false;
    let mut add_model: Option<String> = None;
    let mut remove_model: Option<String> = None;
    let mut models_csv: Option<String> = None;
    let mut fast_model: Option<String> = None;
    let mut fallback_model: Option<String> = None;
    let mut lines: usize = 40;
    let mut notify = false;
    let mut menu_action = "status".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--notify" => {
                notify = true;
            }
            "--enable" => {
                menu_action = "enable".to_string();
            }
            "--disable" => {
                menu_action = "disable".to_string();
            }
            "--toggle" => {
                menu_action = "toggle".to_string();
            }
            "--status" => {
                menu_action = "status".to_string();
            }
            "--target" => {
                i += 1;
                if i < args.len() {
                    target = args[i].clone();
                }
            }
            arg if arg.starts_with("--target=") => {
                target = arg["--target=".len()..].to_string();
            }
            "--model" => {
                i += 1;
                if i < args.len() {
                    model = Some(args[i].clone());
                }
            }
            arg if arg.starts_with("--model=") => {
                model = Some(arg["--model=".len()..].to_string());
            }
            "--add-model" => {
                i += 1;
                if i < args.len() {
                    add_model = Some(args[i].clone());
                }
            }
            arg if arg.starts_with("--add-model=") => {
                add_model = Some(arg["--add-model=".len()..].to_string());
            }
            "--remove-model" => {
                i += 1;
                if i < args.len() {
                    remove_model = Some(args[i].clone());
                }
            }
            arg if arg.starts_with("--remove-model=") => {
                remove_model = Some(arg["--remove-model=".len()..].to_string());
            }
            "--models" => {
                i += 1;
                if i < args.len() {
                    models_csv = Some(args[i].clone());
                }
            }
            arg if arg.starts_with("--models=") => {
                models_csv = Some(arg["--models=".len()..].to_string());
            }
            "--fast-model" => {
                i += 1;
                if i < args.len() {
                    fast_model = Some(args[i].clone());
                }
            }
            arg if arg.starts_with("--fast-model=") => {
                fast_model = Some(arg["--fast-model=".len()..].to_string());
            }
            "--fallback-model" => {
                i += 1;
                if i < args.len() {
                    fallback_model = Some(args[i].clone());
                }
            }
            arg if arg.starts_with("--fallback-model=") => {
                fallback_model = Some(arg["--fallback-model=".len()..].to_string());
            }
            "--effort" => {
                i += 1;
                if i < args.len() {
                    effort = Some(args[i].clone());
                }
            }
            arg if arg.starts_with("--effort=") => {
                effort = Some(arg["--effort=".len()..].to_string());
            }
            "--flip" => {
                flip = true;
            }
            "--lines" => {
                i += 1;
                if i < args.len() {
                    lines = args[i].parse().unwrap_or(40);
                }
            }
            arg if arg.starts_with("--lines=") => {
                lines = arg["--lines=".len()..].parse().unwrap_or(40);
            }
            _ => {}
        }
        i += 1;
    }

    let res = match sub {
        "status" => build_status_dict(),
        "enable" => cmd_enable(&target, model.as_deref(), effort.as_deref()),
        "disable" => cmd_disable(&target),
        "toggle" => cmd_toggle(&target),
        "set-model" => cmd_set_model(
            model.as_deref(),
            effort.as_deref(),
            flip,
            add_model.as_deref(),
            remove_model.as_deref(),
            models_csv.as_deref(),
            fast_model.as_deref(),
            fallback_model.as_deref(),
        ),
        "start-router" => cmd_start_router(),
        "stop-router" => cmd_stop_router(),
        "restart-router" => cmd_restart_router(),
        "logs" => cmd_logs(lines),
        "adc-login" => cmd_adc_login(),
        "refresh-auth" => cmd_refresh_auth(),
        "menu" => cmd_menu(&menu_action),
        other => Err(format!("Unknown subcommand: '{}'", other)),
    }?;

    if notify {
        let status_label = res
            .get("status_label")
            .and_then(|v| v.as_str())
            .unwrap_or("Updated");
        let model_display = res
            .get("primary_model_display")
            .and_then(|v| v.as_str())
            .unwrap_or("Gemini");
        let effort_s = res
            .get("reasoning_effort")
            .and_then(|v| v.as_str())
            .unwrap_or("medium");
        match sub {
            "enable" | "disable" | "toggle" => {
                send_desktop_notification(
                    status_label,
                    &format!("Model: {} ({})", model_display, effort_s),
                    "normal",
                );
            }
            "set-model" => {
                send_desktop_notification(
                    &format!("Active Model: {}", model_display),
                    &format!("{} · Effort: {}", status_label, effort_s),
                    "normal",
                );
            }
            "adc-login" => {
                send_desktop_notification(
                    "Vertex AI ADC Login",
                    "Launched gcloud application-default login in terminal",
                    "normal",
                );
            }
            "menu" => {
                let on = res
                    .get("menu_enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                send_desktop_notification(
                    if on {
                        "Omarchy Menu Enabled"
                    } else {
                        "Omarchy Menu Disabled"
                    },
                    "SUPER+SPACE -> Trigger -> Gemini ADC Router",
                    "normal",
                );
            }
            _ => {}
        }
    }

    Ok(res)
}

pub fn dispatch_json_action(body: &Value) -> Result<Map<String, Value>, String> {
    if let Some(arr) = body.get("args").and_then(|v| v.as_array()) {
        let args: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        return dispatch_args(&args);
    }
    let cmd = body
        .get("command")
        .or_else(|| body.get("action"))
        .and_then(|v| v.as_str())
        .unwrap_or("status");
    let mut args = vec![cmd.to_string()];
    if let Some(t) = body.get("target").and_then(|v| v.as_str()) {
        args.push("--target".to_string());
        args.push(t.to_string());
    }
    if let Some(m) = body.get("model").and_then(|v| v.as_str()) {
        args.push("--model".to_string());
        args.push(m.to_string());
    }
    if let Some(e) = body.get("effort").and_then(|v| v.as_str()) {
        args.push("--effort".to_string());
        args.push(e.to_string());
    }
    if body.get("flip").and_then(|v| v.as_bool()) == Some(true) {
        args.push("--flip".to_string());
    }
    if let Some(l) = body.get("lines").and_then(|v| v.as_u64()) {
        args.push("--lines".to_string());
        args.push(l.to_string());
    }
    dispatch_args(&args)
}

pub fn get_status_map() -> Map<String, Value> {
    build_status_dict().unwrap_or_else(|e| {
        let mut m = Map::new();
        m.insert("ok".to_string(), Value::Bool(false));
        m.insert("error".to_string(), Value::String(e));
        m
    })
}

pub fn is_router_daemon_running() -> bool {
    fetch_router_health(Duration::from_millis(800)).is_some() || is_os_service_active()
}

pub fn run_cli() {
    let args: Vec<String> = env::args().skip(1).collect();
    match dispatch_args(&args) {
        Ok(res) => {
            println!(
                "{}",
                serde_json::to_string(&Value::Object(res)).unwrap_or_default()
            );
        }
        Err(err) => {
            println!(
                "{}",
                serde_json::to_string(&json!({"ok": false, "error": err})).unwrap_or_default()
            );
        }
    }
}
