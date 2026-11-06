use std::collections::HashMap;

use aether_core::Message;
use aether_driver::CliModel;

use super::*;

#[test]
fn foot_facts_prefer_cli_cost_and_served_model() {
    let models = [CliModel { id: "m".into(), name: "Model".into(), ..Default::default() }];
    let prices = HashMap::from([("m".to_string(), (1e-6, 1e-6))]);
    let mut reply = Message::assistant();
    reply.model = Some("m".into());
    reply.prompt_tokens = 80_000;
    let guess = foot_facts(&reply, &models, &prices, false);
    assert_eq!(guess.cost.as_deref(), Some("≈$0.08"));
    assert_eq!(guess.model.as_deref(), Some("Model"));
    assert_eq!(guess.duration_ms, None);
    reply.cost_micros = 120_000;
    reply.served_model = Some("served/x".into());
    reply.elapsed_ms = Some(5_000);
    let real = foot_facts(&reply, &models, &prices, false);
    assert_eq!(real.cost.as_deref(), Some("$0.12"));
    assert_eq!(real.model.as_deref(), Some("served/x"));
    assert_eq!(real.duration_ms, Some(5_000));
}
