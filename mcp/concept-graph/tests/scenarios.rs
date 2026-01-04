use concept_graph::service::{
    ConceptGraphService, ConceptSearchRequest, ConceptUpsertRequest, EpisodeAddRequest,
    RecallQueryRequest, RelationAddRequest, RelationType, UpdateAffectRequest,
};
use futures_util::FutureExt;
use neo4rs::{query, Graph};
use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;
use std::env;
use std::future::Future;
use std::panic::{AssertUnwindSafe, resume_unwind};
use serial_test::serial;
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
    ConceptGraphService::connect(uri, user, password, TAU_MS, false)
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

async fn with_cleanup<F, Fut>(graph: &Graph, tag: &str, f: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    let result = AssertUnwindSafe(f()).catch_unwind().await;
    cleanup(graph, tag).await;
    if let Err(payload) = result {
        resume_unwind(payload);
    }
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

fn extract_concepts(result: rmcp::model::CallToolResult) -> Vec<String> {
    let value = result
        .structured_content
        .expect("structured_content present");
    let concepts = value
        .get("concepts")
        .expect("concepts present")
        .clone();
    serde_json::from_value(concepts).expect("parse concepts")
}

#[tokio::test]
#[serial]
async fn scenario_directional_hop_decay() {
    let tag = format!("test__{}", Uuid::new_v4());
    let apple = format!("apple__{}", tag);
    let fruit = format!("fruit__{}", tag);

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    with_cleanup(&graph, &tag, || async {
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
            .update_affect(Parameters(UpdateAffectRequest {
                target: apple.clone(),
                valence_delta: 1.0,
            }))
            .await
            .expect("update_affect apple");
        service
            .update_affect(Parameters(UpdateAffectRequest {
                target: fruit.clone(),
                valence_delta: 1.0,
            }))
            .await
            .expect("update_affect fruit");

        service
            .relation_add(Parameters(RelationAddRequest {
                from: apple.clone(),
                to: fruit.clone(),
                relation_type: RelationType::IsA,
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
        assert!(forward_score > 0.2, "forward score should be near 0.25");
        assert!(reverse_score > 0.1, "reverse score should be near 0.125");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn scenario_update_affect_does_not_lower_arousal() {
    let tag = format!("test__{}", Uuid::new_v4());
    let concept = format!("arousal__{}", tag);

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    with_cleanup(&graph, &tag, || async {
        service
            .update_affect(Parameters(UpdateAffectRequest {
                target: concept.clone(),
                valence_delta: 0.8,
            }))
            .await
            .expect("update_affect high");

        let (valence_1, arousal_level_1, accessed_1) =
            fetch_concept_state(&graph, &concept).await.expect("state 1");

        service
            .update_affect(Parameters(UpdateAffectRequest {
                target: concept.clone(),
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
    })
    .await;
}

#[tokio::test]
#[serial]
async fn scenario_episode_valence_is_returned() {
    let tag = format!("test__{}", Uuid::new_v4());
    let concept = format!("episode__{}", tag);
    let summary = format!("episode summary {}", tag);

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    with_cleanup(&graph, &tag, || async {
        service
            .update_affect(Parameters(UpdateAffectRequest {
                target: concept.clone(),
                valence_delta: 0.6,
            }))
            .await
            .expect("update_affect");

        let episode_result = service
            .episode_add(Parameters(EpisodeAddRequest {
                summary: summary.clone(),
                concepts: vec![concept.clone()],
            }))
            .await
            .expect("episode_add");

        let episode_id = episode_result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("episode_id"))
            .and_then(|value| value.as_str())
            .expect("episode_id")
            .to_string();

        service
            .update_affect(Parameters(UpdateAffectRequest {
                target: episode_id.clone(),
                valence_delta: -0.4,
            }))
            .await
            .expect("update_affect episode");

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
    })
    .await;
}

#[tokio::test]
#[serial]
async fn scenario_relation_tautology_is_rejected() {
    let tag = format!("test__{}", Uuid::new_v4());
    let concept = format!("loop__{}", tag);

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    with_cleanup(&graph, &tag, || async {
        let result = service
            .relation_add(Parameters(RelationAddRequest {
                from: concept.clone(),
                to: concept.clone(),
                relation_type: RelationType::IsA,
            }))
            .await;

        assert!(result.is_err(), "tautology should error");
        let err = result.err().expect("error present");
        assert!(err.message.contains("tautology"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn scenario_relation_add_supports_episode_evokes() {
    let tag = format!("test__{}", Uuid::new_v4());
    let concept = format!("concept__{}", tag);
    let summary_one = format!("episode one {}", tag);
    let summary_two = format!("episode two {}", tag);

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    with_cleanup(&graph, &tag, || async {
        service
            .concept_upsert(Parameters(ConceptUpsertRequest {
                concept: concept.clone(),
            }))
            .await
            .expect("concept_upsert");

        let episode_one = service
            .episode_add(Parameters(EpisodeAddRequest {
                summary: summary_one.clone(),
                concepts: vec![concept.clone()],
            }))
            .await
            .expect("episode_add one");
        let episode_one_id = episode_one
            .structured_content
            .as_ref()
            .and_then(|value| value.get("episode_id"))
            .and_then(|value| value.as_str())
            .expect("episode_id one")
            .to_string();

        let episode_two = service
            .episode_add(Parameters(EpisodeAddRequest {
                summary: summary_two.clone(),
                concepts: vec![concept.clone()],
            }))
            .await
            .expect("episode_add two");
        let episode_two_id = episode_two
            .structured_content
            .as_ref()
            .and_then(|value| value.get("episode_id"))
            .and_then(|value| value.as_str())
            .expect("episode_id two")
            .to_string();

        service
            .relation_add(Parameters(RelationAddRequest {
                from: concept.clone(),
                to: episode_one_id.clone(),
                relation_type: RelationType::Evokes,
            }))
            .await
            .expect("relation_add concept->episode");

        service
            .relation_add(Parameters(RelationAddRequest {
                from: concept.clone(),
                to: episode_one_id.clone(),
                relation_type: RelationType::Evokes,
            }))
            .await
            .expect("relation_add concept->episode again");

        let weight_query = query(
            "MATCH (c:Concept {name: $from})-[r:EVOKES]->(e:Episode {name: $to})\n\
             RETURN r.weight AS weight",
        )
        .param("from", concept.as_str())
        .param("to", episode_one_id.as_str());

        let mut result = graph.execute(weight_query).await.expect("weight query");
        let row = result.next().await.expect("weight row");
        let weight: f64 = row
            .expect("weight row present")
            .get("weight")
            .unwrap_or(0.0);
        assert!(weight > 0.25, "weight should be strengthened");

        service
            .relation_add(Parameters(RelationAddRequest {
                from: episode_one_id.clone(),
                to: episode_two_id.clone(),
                relation_type: RelationType::Evokes,
            }))
            .await
            .expect("relation_add episode->episode");

        let episode_weight_query = query(
            "MATCH (a:Episode {name: $from})-[r:EVOKES]->(b:Episode {name: $to})\n\
             RETURN r.weight AS weight",
        )
        .param("from", episode_one_id.as_str())
        .param("to", episode_two_id.as_str());

        let mut result = graph.execute(episode_weight_query).await.expect("episode weight query");
        let row = result.next().await.expect("episode weight row");
        let weight: f64 = row
            .expect("episode weight row present")
            .get("weight")
            .unwrap_or(0.0);
        assert!(weight >= 0.25, "episode weight should be set");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn scenario_relation_add_rejects_episode_is_a() {
    let tag = format!("test__{}", Uuid::new_v4());
    let concept = format!("concept__{}", tag);
    let summary = format!("episode reject {}", tag);

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    with_cleanup(&graph, &tag, || async {
        service
            .concept_upsert(Parameters(ConceptUpsertRequest {
                concept: concept.clone(),
            }))
            .await
            .expect("concept_upsert");

        let episode_result = service
            .episode_add(Parameters(EpisodeAddRequest {
                summary: summary.clone(),
                concepts: vec![concept.clone()],
            }))
            .await
            .expect("episode_add");
        let episode_id = episode_result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("episode_id"))
            .and_then(|value| value.as_str())
            .expect("episode_id")
            .to_string();

        let result = service
            .relation_add(Parameters(RelationAddRequest {
                from: episode_id.clone(),
                to: concept.clone(),
                relation_type: RelationType::IsA,
            }))
            .await;

        assert!(result.is_err(), "episode is-a should error");
        let err = result.err().expect("error present");
        assert!(err.message.contains("episode endpoint"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn scenario_concept_search_matches_keywords_and_arousal() {
    let tag = format!("test__{}", Uuid::new_v4());
    let match_concept = format!("match__{}", tag);
    let high_concept = format!("!high__{}", tag);
    let keyword = match_concept.clone();

    let graph = connect_graph().await;
    cleanup(&graph, &tag).await;

    let service = connect_service().await;

    with_cleanup(&graph, &tag, || async {
        service
            .concept_upsert(Parameters(ConceptUpsertRequest {
                concept: match_concept.clone(),
            }))
            .await
            .expect("concept_upsert match");

        service
            .concept_upsert(Parameters(ConceptUpsertRequest {
                concept: high_concept.clone(),
            }))
            .await
            .expect("concept_upsert high");

        service
            .update_affect(Parameters(UpdateAffectRequest {
                target: high_concept.clone(),
                valence_delta: 1.0,
            }))
            .await
            .expect("update_affect high");

        let search = service
            .concept_search(Parameters(ConceptSearchRequest {
                keywords: vec![keyword],
                limit: Some(2),
            }))
            .await
            .expect("concept_search");

        let results = extract_concepts(search);
        assert!(
            results.contains(&match_concept),
            "partial match should be included"
        );
        assert!(
            results.contains(&high_concept),
            "arousal-ranked concept should be included"
        );
    })
    .await;
}
