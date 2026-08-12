use std::{net::IpAddr, process::Command, str::FromStr};

use url::Url;

use crate::{
    activity,
    model::{AppSettings, ModelInfo, Provider, ReasoningLevel},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ReasoningRecommendation {
    pub level: ReasoningLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy)]
struct GpuMemory {
    total_mb: u64,
    used_mb: u64,
}

fn parameter_count_from_name(model: &str) -> Option<f64> {
    let lower = model.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for index in 0..bytes.len() {
        if !bytes[index].is_ascii_digit() { continue; }
        let mut end = index + 1;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') { end += 1; }
        if end < bytes.len() && bytes[end] == b'b' {
            if let Ok(value) = lower[index..end].parse::<f64>() { return Some(value); }
        }
    }
    None
}

fn is_local_provider(provider: &Provider) -> bool {
    let Ok(url) = Url::parse(&provider.base_url) else { return false; };
    let Some(host) = url.host_str() else { return false; };
    if host.eq_ignore_ascii_case("localhost") { return true; }
    IpAddr::from_str(host).is_ok_and(|ip| match ip {
        IpAddr::V4(value) => value.is_loopback() || value.is_private(),
        IpAddr::V6(value) => value.is_loopback() || value.is_unique_local(),
    })
}

fn gpu_memory() -> Option<GpuMemory> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total,memory.used", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    String::from_utf8_lossy(&output.stdout).lines().find_map(|line| {
        let mut values = line.split(',').map(str::trim);
        Some(GpuMemory {
            total_mb: values.next()?.parse().ok()?,
            used_mb: values.next()?.parse().ok()?,
        })
    })
}

fn selected_model(provider: &Provider) -> Option<&ModelInfo> {
    provider.default_model.as_deref()
        .and_then(|id| provider.models.iter().find(|model| model.id == id))
        .or_else(|| provider.models.first())
}

pub fn recommend(provider: &Provider) -> ReasoningRecommendation {
    let model = selected_model(provider);
    let model_name = model.map(|item| format!("{} {}", item.id, item.display_name))
        .or_else(|| provider.default_model.clone())
        .unwrap_or_else(|| "未标注模型".into());
    let parameters = model.and_then(|item| item.parameter_count_billions)
        .or_else(|| parameter_count_from_name(&model_name));
    let context_window = model.and_then(|item| item.context_window);
    let local = is_local_provider(provider);
    let gpu = local.then(gpu_memory).flatten();
    let lower = model_name.to_ascii_lowercase();

    let mut score = 0_i32;
    let mut factors = Vec::new();
    if let Some(value) = parameters {
        if value >= 65.0 { score += 3; factors.push(format!("大参数量模型（{value:.1}B）")); }
        else if value >= 30.0 { score += 2; factors.push(format!("中大型模型（{value:.1}B）")); }
        else if value >= 13.0 { score += 1; factors.push(format!("中等参数量模型（{value:.1}B）")); }
        else { factors.push(format!("轻量模型（{value:.1}B）")); }
    }
    if let Some(window) = context_window {
        if window >= 500_000 { score += 2; factors.push(format!("超长上下文（{}K）", window / 1000)); }
        else if window >= 128_000 { score += 1; factors.push(format!("长上下文（{}K）", window / 1000)); }
    }
    if ["reason", "deepseek-r1", "o1", "o3", "o4"].iter().any(|marker| lower.contains(marker)) {
        score += 2;
        factors.push("模型名称包含推理能力标识".into());
    }
    if local {
        factors.push("本地 API".into());
        if let Some(memory) = gpu {
            let free_mb = memory.total_mb.saturating_sub(memory.used_mb);
            if free_mb < 8 * 1024 { score -= 2; factors.push(format!("可用显存约 {}GB", free_mb / 1024)); }
            else if free_mb < 16 * 1024 { score -= 1; factors.push(format!("可用显存约 {}GB", free_mb / 1024)); }
            else if free_mb >= 24 * 1024 { score += 1; factors.push(format!("可用显存约 {}GB", free_mb / 1024)); }
        } else {
            factors.push("未检测到可读取的 NVIDIA 显存数据".into());
        }
    } else {
        score += 1;
        factors.push("云端 API，不占用本机显存".into());
    }

    let level = if score >= 3 { ReasoningLevel::High } else if score >= 1 { ReasoningLevel::Medium } else { ReasoningLevel::Low };
    let label = match level { ReasoningLevel::Low => "轻度", ReasoningLevel::Medium => "中度", ReasoningLevel::High => "高" };
    let basis = if factors.is_empty() { "模型元数据有限，采用保守规则".into() } else { factors.join("、") };
    ReasoningRecommendation {
        level,
        message: format!("{basis}，自动选用{label}推理模式"),
    }
}

pub fn refresh_settings(settings: &mut AppSettings, provider: Option<&Provider>, log_action: bool) {
    if settings.auto_reasoning_mode {
        if let Some(provider) = provider {
            let recommendation = recommend(provider);
            settings.effective_reasoning_level = recommendation.level;
            settings.reasoning_match_message = Some(recommendation.message.clone());
            if log_action {
                activity::record("auto_reasoning", &format!("{}：{}", provider.name, recommendation.message), true);
            }
        } else {
            settings.effective_reasoning_level = settings.manual_reasoning_level;
            settings.reasoning_match_message = Some("尚未选择 Provider，暂时沿用上一次手动推理档位".into());
        }
    } else {
        settings.effective_reasoning_level = settings.manual_reasoning_level;
        settings.reasoning_match_message = None;
        if log_action {
            activity::record("manual_reasoning", &format!("切换为{}档", settings.manual_reasoning_level.as_str()), true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClaudeModelMappings, CodexCompatibility, ProtocolKind};

    fn provider(model: ModelInfo, base_url: &str) -> Provider {
        Provider {
            id: "p".into(), name: "测试".into(), base_url: base_url.into(),
            protocol: ProtocolKind::Openai, enabled: true, is_current: true,
            default_model: Some(model.id.clone()), claude_model_profile: None,
            claude_extended_context: false, claude_model_mappings: ClaudeModelMappings::default(),
            codex_compatibility: CodexCompatibility::Full, codex_probe_model: None,
            codex_probe_detail: None, reasoning_selections: vec![],
            models: vec![model], connection_state: "connected".into(),
            confidence: None, last_checked_at: None, applied_clients: vec![], error_summary: None,
        }
    }

    #[test]
    fn large_cloud_model_uses_high() {
        let model = ModelInfo {
            id: "coder-70b".into(), display_name: "Coder 70B".into(), provider: None,
            protocol: ProtocolKind::Openai, source: "server".into(), capabilities: vec![],
            context_window: Some(128_000), parameter_count_billions: None, reasoning: None,
        };
        assert_eq!(recommend(&provider(model, "https://api.example.com/v1")).level, ReasoningLevel::High);
    }

    #[test]
    fn small_local_model_uses_low_without_gpu_bonus() {
        let model = ModelInfo {
            id: "mini-3b".into(), display_name: "Mini 3B".into(), provider: None,
            protocol: ProtocolKind::Openai, source: "server".into(), capabilities: vec![],
            context_window: Some(32_000), parameter_count_billions: Some(3.0), reasoning: None,
        };
        assert_eq!(recommend(&provider(model, "http://127.0.0.1:11434/v1")).level, ReasoningLevel::Low);
    }
}
