---
date: 2026-05-17
---

# Integration Harness Inclusive Metric Thresholds

## Context

The integration harness aggregated scenario metrics with strict `mean > 0.7` and `min > 0.5`
thresholds. This made a metric score of exactly `0.7` fail at the aggregate level even when the
judge marked the run as passing, producing confusing single-run results where `run.pass=true` and
`overall_pass=false`.

## Decision

Use inclusive aggregate thresholds: `mean >= 0.7` and `min >= 0.5` for every non-excluded metric.

## Rationale

Scenario metrics are coarse LLM-judge scores, and the threshold value is intended to represent the
minimum acceptable quality band. Treating the boundary as failing makes borderline-but-acceptable
scores look like harness failures rather than quality warnings.

## Compatibility Impact

breaking-by-default (no compatibility layer). Existing result interpretation should use the new
inclusive threshold rule.
