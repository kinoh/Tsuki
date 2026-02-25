# Soft Trigger Threshold Tuning for Submodule Scenario

## Overview
Adjusted router `recommendation_threshold` from `0.6` to `0.2` in `core-rust/config.toml` and re-ran the `Submodule` integration scenario with `runner.max10.toml`.

## Problem Statement
Observed router `hard_effective_scores` clustered around `0.125`, `0.25`, and `0.5`, so a soft threshold of `0.6` often produced no soft recommendations. This prevented useful observability of submodule recommendation behavior in the scenario.

## Decision
Set `recommendation_threshold = 0.2` to make soft recommendations visible under current relation weights and activation levels.

## Evidence
- Before change (same scenario family): soft recommendations were frequently empty.
- After change (`20260225-125045__Submodule.json`):
  - `submodule_trigger_recall`: `0.0 -> 0.2`
  - router soft recommendation counts by turn: `0, 1, 2, 3, 3`

## Why
The goal is not to force hard triggers. The goal is to make soft recommendation behavior observable and evaluable under the existing activation model.

## Compatibility Impact
Breaking-by-default policy unaffected. This is a runtime behavior tuning in current config and intentionally changes recommendation frequency.
