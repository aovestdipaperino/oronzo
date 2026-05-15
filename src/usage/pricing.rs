use serde::Deserialize;
use std::collections::HashMap;

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

impl Pricing {
    pub fn bundled() -> Self {
        let models: HashMap<String, ModelPricing> =
            serde_json::from_str(BUNDLED).unwrap_or_default();
        Pricing { models }
    }

    pub fn lookup(&self, model: &str) -> Option<&ModelPricing> {
        self.models.get(model)
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
}
