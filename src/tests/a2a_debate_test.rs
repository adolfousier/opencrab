use crate::a2a::debate::*;
use crate::a2a::types::Role;
fn test_config() -> DebateConfig {
    DebateConfig {
        topic: "Should AI agents have persistent memory across sessions?".to_string(),
        num_bees: 3,
        max_rounds: 3,
        consensus_threshold: 0.8,
        knowledge_context: vec![
            "Memory architectures: L0-L6 layered system with SQLite + FTS5".to_string(),
            "Security concern: memory injection attacks via prompt manipulation".to_string(),
        ],
        bee_endpoints: vec![
            "http://bee-1:18789/a2a/v1".to_string(),
            "http://bee-2:18789/a2a/v1".to_string(),
            "http://bee-3:18789/a2a/v1".to_string(),
        ],
    }
}

#[test]
fn test_debate_session_creation() {
    let config = test_config();
    let session = DebateSession::new(config);
    assert_eq!(session.state, DebateState::Pending);
    assert_eq!(session.current_round, 0);
    assert!(session.rounds.is_empty());
}

#[test]
fn test_round1_prompt_includes_knowledge() {
    let config = test_config();
    let session = DebateSession::new(config);
    let prompt = session.round1_prompt();

    assert!(prompt.contains("Should AI agents"));
    assert!(prompt.contains("Knowledge Base Context"));
    assert!(prompt.contains("L0-L6 layered system"));
    assert!(prompt.contains("memory injection attacks"));
    assert!(prompt.contains("Confidence score"));
}

#[test]
fn test_build_round_messages() {
    let config = test_config();
    let session = DebateSession::new(config);
    let messages = session.build_round_messages(1);

    assert_eq!(messages.len(), 3);
    for (endpoint, msg) in &messages {
        assert!(endpoint.starts_with("http://bee-"));
        assert_eq!(msg.role, Role::User);
        assert!(!msg.parts.is_empty());
        assert!(msg.metadata.is_some());
    }
}

#[test]
fn test_consensus_analysis_agreement() {
    let responses = vec![
        BeeResponse {
            bee_id: "bee-1".to_string(),
            endpoint: "http://bee-1:18789".to_string(),
            content: "Yes, persistent memory is essential.".to_string(),
            confidence: 0.9,
            position: Some("pro".to_string()),
            key_points: vec![],
        },
        BeeResponse {
            bee_id: "bee-2".to_string(),
            endpoint: "http://bee-2:18789".to_string(),
            content: "Strongly agree with persistent memory.".to_string(),
            confidence: 0.85,
            position: Some("pro".to_string()),
            key_points: vec![],
        },
        BeeResponse {
            bee_id: "bee-3".to_string(),
            endpoint: "http://bee-3:18789".to_string(),
            content: "Yes, but with security constraints.".to_string(),
            confidence: 0.8,
            position: Some("pro".to_string()),
            key_points: vec![],
        },
    ];

    let consensus = DebateSession::analyze_consensus(&responses, 0.8);
    assert!(consensus.consensus_reached);
    assert!(!consensus.agreement_points.is_empty());
    assert!(consensus.avg_confidence > 0.8);
}

#[test]
fn test_consensus_analysis_contention() {
    let responses = vec![
        BeeResponse {
            bee_id: "bee-1".to_string(),
            endpoint: "http://bee-1:18789".to_string(),
            content: "Yes to memory.".to_string(),
            confidence: 0.9,
            position: Some("pro".to_string()),
            key_points: vec![],
        },
        BeeResponse {
            bee_id: "bee-2".to_string(),
            endpoint: "http://bee-2:18789".to_string(),
            content: "No, too risky.".to_string(),
            confidence: 0.7,
            position: Some("con".to_string()),
            key_points: vec![],
        },
    ];

    let consensus = DebateSession::analyze_consensus(&responses, 0.8);
    assert!(!consensus.consensus_reached);
    assert!(!consensus.contention_points.is_empty());
}

#[test]
fn test_record_round_and_state_transition() {
    let config = test_config();
    let mut session = DebateSession::new(config);

    let responses = vec![BeeResponse {
        bee_id: "bee-1".to_string(),
        endpoint: "http://bee-1:18789".to_string(),
        content: "My analysis...".to_string(),
        confidence: 0.9,
        position: Some("pro".to_string()),
        key_points: vec![],
    }];

    session.record_round(1, "Round 1 prompt".to_string(), responses);
    assert_eq!(session.current_round, 1);
    // With only 1 bee saying "pro", consensus should be reached
    assert_eq!(session.state, DebateState::Concluded);
}

#[test]
fn test_summary_report() {
    let config = test_config();
    let mut session = DebateSession::new(config);

    let responses = vec![BeeResponse {
        bee_id: "bee-1".to_string(),
        endpoint: "http://bee-1:18789".to_string(),
        content: "Persistent memory is crucial for continuity.".to_string(),
        confidence: 0.85,
        position: Some("pro".to_string()),
        key_points: vec!["continuity".to_string()],
    }];

    session.record_round(1, "Topic prompt".to_string(), responses);
    let report = session.summary_report();

    assert!(report.contains("Bee Colony Debate Report"));
    assert!(report.contains("Should AI agents"));
    assert!(report.contains("Persistent memory is crucial"));
    assert!(report.contains("Consensus Analysis"));
}

#[test]
fn test_critique_prompt_includes_previous_responses() {
    let config = test_config();
    let mut session = DebateSession::new(config);

    // Simulate Round 1
    let r1_responses = vec![
        BeeResponse {
            bee_id: "bee-1".to_string(),
            endpoint: "http://bee-1:18789".to_string(),
            content: "Memory helps with learning.".to_string(),
            confidence: 0.8,
            position: Some("pro".to_string()),
            key_points: vec![],
        },
        BeeResponse {
            bee_id: "bee-2".to_string(),
            endpoint: "http://bee-2:18789".to_string(),
            content: "Privacy risks are high.".to_string(),
            confidence: 0.6,
            position: Some("con".to_string()),
            key_points: vec![],
        },
    ];
    session.record_round(1, "Round 1".to_string(), r1_responses);
    session.state = DebateState::InRound; // Force to allow R2

    let critique = session.critique_prompt(2);
    assert!(critique.contains("Critique & Synthesis"));
    assert!(critique.contains("Memory helps with learning"));
    assert!(critique.contains("Privacy risks are high"));
    assert!(critique.contains("Bee bee-1"));
    assert!(critique.contains("Bee bee-2"));
}
