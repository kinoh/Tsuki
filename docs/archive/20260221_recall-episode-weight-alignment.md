# Recall Episode Weight Alignment

## Context
- Recall scoring used relation-edge weight for concept-to-concept propositions.
- Episode propositions (`concept evokes episode`) were scored without EVOKES edge weight.
- This created an unintended bias where episode propositions could dominate ranking even when edge confidence was weak.

## Decision
- Added EVOKES relation weight to episode recall scoring.
- Updated score formula for episode propositions from:
  - `arousal * hop_decay`
  to:
  - `arousal * hop_decay * edge.weight`
- Extended internal `EpisodeEntry` to carry `weight` from the EVOKES relationship.
- Included episode `weight` in concept debug detail output.

## Why
- Keep scoring semantics consistent across relation-based and episode-based propositions.
- Reduce accidental episode-heavy recall results not supported by relation strength.
- Improve explainability of recall ranking in debug output.
