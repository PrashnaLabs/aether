use adk_rust::model::openrouter::OpenRouterBigNumber;
use adk_rust::prelude::OpenRouterClient;
use aether_driver::catalog::strip_vendor;

pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_length: Option<u64>,
    /// Dollars per token, when OpenRouter reports them.
    pub prompt_price: Option<f64>,
    pub completion_price: Option<f64>,
}

/// OpenRouter models that can call tools, since every message may offer some.
pub(crate) async fn list(client: OpenRouterClient) -> adk_rust::Result<Vec<ModelInfo>> {
    Ok(client
        .list_models()
        .await?
        .into_iter()
        .filter(|m| m.supported_parameters.iter().any(|p| p == "tools"))
        .map(|m| ModelInfo {
            context_length: m.context_length.and_then(|n| u64::try_from(n).ok()),
            prompt_price: m.pricing.as_ref().and_then(|p| price(&p.prompt)),
            completion_price: m.pricing.as_ref().and_then(|p| price(&p.completion)),
            name: m
                .name
                .as_deref()
                .map(strip_vendor)
                .filter(|n| !n.is_empty())
                .unwrap_or(&m.id)
                .into(),
            id: m.id,
        })
        .collect())
}

fn price(number: &Option<OpenRouterBigNumber>) -> Option<f64> {
    match number {
        Some(OpenRouterBigNumber::Float(f)) => Some(*f),
        Some(OpenRouterBigNumber::Integer(n)) => Some(*n as f64),
        Some(OpenRouterBigNumber::String(s)) => s.parse().ok(),
        None => None,
    }
}
