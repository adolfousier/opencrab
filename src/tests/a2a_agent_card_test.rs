use crate::a2a::agent_card::*;
use crate::brain::tools::ToolRegistry;

#[test]
fn test_build_agent_card_default() {
    let card = build_agent_card("127.0.0.1", 18790, None);
    assert!(card.name.contains("OpenCrabs"));
    assert_eq!(card.skills.len(), 3);
    assert_eq!(
        card.supported_interfaces[0].url,
        "http://127.0.0.1:18790/a2a/v1"
    );
}

#[test]
fn test_build_agent_card_with_registry() {
    let registry = ToolRegistry::new();
    let card = build_agent_card("127.0.0.1", 18790, Some(&registry));
    // No search tools registered, so no research skill
    assert_eq!(card.skills.len(), 2);
    assert_eq!(card.skills[0].id, "code-analysis");
    assert_eq!(card.skills[1].id, "debate");
}
