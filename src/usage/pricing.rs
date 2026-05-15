use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Deserialize)]
pub struct ModelPricing {
    #[serde(rename = "input_cost_per_token", default)]
    pub input: f64,
    #[serde(rename = "output_cost_per_token", default)]
    pub output: f64,
    #[serde(rename = "cache_creation_input_token_cost", default)]
    pub cache_creation: f64,
    #[serde(rename = "cache_read_input_token_cost", default)]
    pub cache_read: f64,
}

#[derive(Debug, Default, Clone)]
pub struct Pricing {
    pub models: HashMap<String, ModelPricing>,
}

const BUNDLED: &str = include_str!("pricing.json");

const LITELLM_URL: &str = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

fn cache_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oronzo")
        .join("pricing.json")
}

fn mtime_is_today(path: &std::path::Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    let Ok(mtime_secs) = mtime.duration_since(UNIX_EPOCH) else {
        return false;
    };
    let Ok(now_secs) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return false;
    };
    const ONE_DAY: u64 = 86_400;
    (now_secs.as_secs() / ONE_DAY) == (mtime_secs.as_secs() / ONE_DAY)
}

fn parse_str(s: &str) -> Pricing {
    let models = serde_json::from_str(s).unwrap_or_default();
    Pricing { models }
}

fn try_fetch_and_cache() -> Option<Pricing> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(2)))
        .build()
        .into();
    let mut resp = agent.get(LITELLM_URL).call().ok()?;
    let body = resp.body_mut().read_to_string().ok()?;
    let parsed = parse_str(&body);
    if parsed.models.is_empty() {
        return None;
    }
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, &body).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
    Some(parsed)
}

impl Pricing {
    pub fn bundled() -> Self {
        let models: HashMap<String, ModelPricing> =
            serde_json::from_str(BUNDLED).unwrap_or_default();
        Pricing { models }
    }

    pub fn lookup(&self, model: &str) -> Option<&ModelPricing> {
        self.models.get(model)
    }

    pub fn load(offline: bool) -> Self {
        if offline {
            return Pricing::bundled();
        }
        let path = cache_path();
        if mtime_is_today(&path) {
            if let Ok(body) = fs::read_to_string(&path) {
                let parsed = parse_str(&body);
                if !parsed.models.is_empty() {
                    return parsed;
                }
            }
        }
        if let Some(fresh) = try_fetch_and_cache() {
            return fresh;
        }
        Pricing::bundled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_loads_sonnet_46() {
        let p = Pricing::bundled();
        let m = p.lookup("claude-sonnet-4-6").expect("model present");
        assert!((m.input - 0.000003).abs() < 1e-12);
        assert!((m.output - 0.000015).abs() < 1e-12);
        assert!((m.cache_creation - 0.00000375).abs() < 1e-12);
        assert!((m.cache_read - 0.0000003).abs() < 1e-12);
    }

    #[test]
    fn bundled_returns_none_for_unknown_model() {
        let p = Pricing::bundled();
        assert!(p.lookup("nonexistent-model").is_none());
    }

    #[test]
    fn load_offline_returns_bundled() {
        let p = Pricing::load(true);
        assert!(p.lookup("claude-sonnet-4-6").is_some());
    }
}
