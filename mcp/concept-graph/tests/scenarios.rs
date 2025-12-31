use concept_graph::service::{
    ConceptGraphService, ConceptUpdateAffectRequest, ConceptUpsertRequest, EpisodeAddRequest,
    RecallQueryRequest, RelationAddRequest,
};
use neo4rs::{query, Graph};
use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;
use std::env;
use uuid::Uuid;

const TAU_MS: f64 = 86_400_000.0;

#[derive(Debug, Deserialize)]
struct PropositionOut {
    text: String,
    score: f64,
    valence: Option<f64>,
}

async fn connect_service() -> ConceptGraphService {
    let uri = env::var("MEMGRAPH_URI").unwrap_or_else(|_| "bolt://localhost:7687".to_string());
    let user = env::var("MEMGRAPH_USER").unwrap_or_default();
    let password = env::var("MEMGRAPH_PASSWORD").unwrap_or_default();
    ConceptGraphService::connect(uri, user, password, TAU_MS)
        .await
        .expect("connect concept graph service")
}

async fn connect_graph() -> Graph {
    let uri = env::var("MEMGRAPH_URI").unwrap_or_else(|_| "bolt://localhost:7687".to_string());
    let user = env::var("MEMGRAPH_USER").unwrap_or_default();
    let password = env::var("MEMGRAPH_PASSWORD").unwrap_or_default();
    Graph::new(uri, user, password).expect("connect graph")
}

async fn cleanup(graph: &Graph, tag: &str) {
    let query = query(
        "MATCH (n)\n\
         WHERE (n:Concept AND n.name CONTAINS $tag)\n\
            OR (n:Episode AND n.summary CONTAINS $tag)\n\
         DETACH DELETE n",
    )
    .param("tag", tag);

    let mut result = graph.execute(query).await.expect("cleanup query");
    let _ = result.next().await;
}

async fn fetch_concept_state(graph: &Graph, concept: &str) -> Option<(f64, f64, i64)> {
    let query = query(
        "MATCH (c:Concept {name: $name})\n\
         RETURN c.valence AS valence, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at",
    )
    .param("name", concept);

    let mut result = graph.execute(query).await.ok()?;
    let row = result.next().await.ok().flatten()?;
    let valence: f64 = row.get("valence").unwrap_or(0.0);
    let arousal_level: f64 = row.get("arousal_level").unwrap_or(0.0);
    let accessed_at: i64 = row.get("accessed_at").unwrap_or(0);
    Some((valence, arousal_level, accessed_at))
}

fn extract_propositions(result: rmcp::model::CallToolResult) -> Vec<PropositionOut> {
    let value = result
        .structured_content
        .expect("structured_content present");
    let props = value
        .get("propositions")
        .expect("propositions present")
        .clone();
    serde_json::from_value(props).expect("parse propositions")
}

#[tokio::test]
async fn scenario_directional_hop_decay() {
    let tag = format!("test__{}", Uuid::new_v4());
    let apple = format!("apple__{}", tag);
    let fruit = format!("fruit__{}", tag);

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    service
        .concept_upsert(Parameters(ConceptUpsertRequest {
            concept: apple.clone(),
        }))
        .await
        .expect("concept_upsert apple");
    service
        .concept_upsert(Parameters(ConceptUpsertRequest {
            concept: fruit.clone(),
        }))
        .await
        .expect("concept_upsert fruit");

    service
        .concept_update_affect(Parameters(ConceptUpdateAffectRequest {
            concept: apple.clone(),
            valence_delta: 1.0,
        }))
        .await
        .expect("update_affect apple");
    service
        .concept_update_affect(Parameters(ConceptUpdateAffectRequest {
            concept: fruit.clone(),
            valence_delta: 1.0,
        }))
        .await
        .expect("update_affect fruit");

    service
        .relation_add(Parameters(RelationAddRequest {
            from: apple.clone(),
            to: fruit.clone(),
            relation_type: "is-a".to_string(),
        }))
        .await
        .expect("relation_add");

    let forward = service
        .recall_query(Parameters(RecallQueryRequest {
            seeds: vec![apple.clone()],
            max_hop: 1,
        }))
        .await
        .expect("recall forward");
    let forward_props = extract_propositions(forward);
    let forward_text = format!("{} is-a {}", apple, fruit);
    let forward_score = forward_props
        .iter()
        .find(|p| p.text == forward_text)
        .map(|p| p.score)
        .expect("forward proposition");

    let reverse = service
        .recall_query(Parameters(RecallQueryRequest {
            seeds: vec![fruit.clone()],
            max_hop: 1,
        }))
        .await
        .expect("recall reverse");
    let reverse_props = extract_propositions(reverse);
    let reverse_score = reverse_props
        .iter()
        .find(|p| p.text == forward_text)
        .map(|p| p.score)
        .expect("reverse proposition");

    assert!(forward_score > reverse_score, "forward should be larger");
    assert!(forward_score > 0.9, "forward score should be near 1");
    assert!(reverse_score > 0.4, "reverse score should be near 0.5");

    cleanup(&graph, &tag).await;
}

#[tokio::test]
async fn scenario_update_affect_does_not_lower_arousal() {
    let tag = format!("test__{}", Uuid::new_v4());
    let concept = format!("arousal__{}", tag);

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    service
        .concept_update_affect(Parameters(ConceptUpdateAffectRequest {
            concept: concept.clone(),
            valence_delta: 0.8,
        }))
        .await
        .expect("update_affect high");

    let (valence_1, arousal_level_1, accessed_1) =
        fetch_concept_state(&graph, &concept).await.expect("state 1");

    service
        .concept_update_affect(Parameters(ConceptUpdateAffectRequest {
            concept: concept.clone(),
            valence_delta: 0.1,
        }))
        .await
        .expect("update_affect low");

    let (valence_2, arousal_level_2, accessed_2) =
        fetch_concept_state(&graph, &concept).await.expect("state 2");

    assert!(valence_2 > valence_1, "valence should increase");
    assert!(
        (arousal_level_1 - arousal_level_2).abs() < 1e-6,
        "arousal_level should not drop"
    );
    assert_eq!(accessed_1, accessed_2, "accessed_at should not update");

    cleanup(&graph, &tag).await;
}

#[tokio::test]
async fn scenario_episode_valence_is_returned() {
    let tag = format!("test__{}", Uuid::new_v4());
    let concept = format!("episode__{}", tag);
    let summary = format!("episode summary {}", tag);

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    service
        .concept_update_affect(Parameters(ConceptUpdateAffectRequest {
            concept: concept.clone(),
            valence_delta: 0.6,
        }))
        .await
        .expect("update_affect");

    service
        .episode_add(Parameters(EpisodeAddRequest {
            summary: summary.clone(),
            concepts: vec![concept.clone()],
            valence: -0.4,
        }))
        .await
        .expect("episode_add");

    let recall = service
        .recall_query(Parameters(RecallQueryRequest {
            seeds: vec![concept.clone()],
            max_hop: 1,
        }))
        .await
        .expect("recall");
    let props = extract_propositions(recall);
    let text = format!("{} evokes {}", concept, summary);
    let entry = props.iter().find(|p| p.text == text).expect("episode proposition");

    assert_eq!(entry.valence, Some(-0.4));

    cleanup(&graph, &tag).await;
}

#[tokio::test]
async fn scenario_relation_tautology_is_rejected() {
    let tag = format!("test__{}", Uuid::new_v4());
    let concept = format!("loop__{}", tag);

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    let result = service
        .relation_add(Parameters(RelationAddRequest {
            from: concept.clone(),
            to: concept.clone(),
            relation_type: "is-a".to_string(),
        }))
        .await;

    assert!(result.is_err(), "tautology should error");
    let err = result.err().expect("error present");
    assert!(err.message.contains("tautology"));

    cleanup(&graph, &tag).await;
}
