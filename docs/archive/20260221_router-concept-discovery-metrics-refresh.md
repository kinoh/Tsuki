# Router Concept Discovery Metrics Refresh

## Context
- The previous `concept_discovery_stability` metric could dominate scenario outcomes even when conversation quality and relevance were already strong.
- The team wanted evaluation to emphasize practical usefulness while still observing concept-graph signal quality.

## Decision
- Removed `concept_discovery_stability` from `router_concept_discovery.yaml`.
- Added three replacement metrics:
  - `concept_score_separation`
  - `concept_episode_coverage`
  - `concept_information_richness`

## Why
- `concept_score_separation` checks whether ranking carries meaningful prioritization signal.
- `concept_episode_coverage` checks structural balance (concept relations + episode recall), avoiding one-sided retrieval.
- `concept_information_richness` checks diversity and non-redundancy of top retrieved lines, closer to practical usefulness than strict temporal stability.
