# Submodule Scenario Metrics Consolidation

## Overview
Reduced Submodule scenario-specific metrics from four to two while keeping baseline metrics unchanged.

## Problem Statement
The previous metric set mixed overlapping concerns and did not directly isolate submodule independence.
- Trigger quality was split across recall/precision, which increased scoring variance.
- Independence intent was only indirectly captured.
- Flow-related metric overlapped with baseline dialog naturalness.

## Decision
For `core-rust/tests/integration/scenarios/submodule.yaml`, keep only two scenario-specific metrics:
- `activation_alignment`
- `module_independence`

Baseline metrics remain unchanged:
- `scenario_requirement_fit`
- `dialog_naturalness`

## Why
This scenario's primary objective is:
1. activate the right submodule at the right phase,
2. verify that submodules are not tightly co-activated as a single block.

The new pair maps directly to those objectives and removes redundant dimensions.

## Metric Intent Mapping
- `activation_alignment` replaces the split between prior trigger recall and trigger precision by evaluating per-phase intent alignment and transition timing in one place.
- `module_independence` explicitly evaluates target vs non-target separation and penalizes broad co-activation.

## Compatibility Impact
Breaking-by-default policy is unchanged. Historical score comparability with prior Submodule runs is intentionally not preserved because metric keys and definitions changed.
