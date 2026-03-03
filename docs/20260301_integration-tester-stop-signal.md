# Integration Tester Stop-Signal Contract Update

## Overview
This document records a reliability update for integration conversation testing in `core-rust`.

Date:
- March 1, 2026

## Problem
`router_concept_discovery.yaml` failed scenario requirement fit due to repeated topic mentions.
A key contributor was tester-side continuation after mission coverage was already sufficient.

User feedback clarified the expected behavior:
- conversation should end when requirements are satisfied,
- not continue until fixed turn budget is consumed.

## Decision
Update tester prompt contract to make early-stop behavior explicit and mandatory:
- treat max turns as upper bound only,
- output `__TEST_DONE__` as soon as mission coverage is satisfied,
- avoid over-covering completed missions,
- enforce one-line strict output (`__TEST_DONE__` or one utterance text).

## Why
This keeps scenario evaluation aligned with intent and reduces false failures caused by unnecessary additional turns.
It also preserves existing harness compatibility because `__TEST_DONE__` was already supported.

## Compatibility Impact
- No runtime API changes.
- No harness code change required.
- Backward compatible with existing `__TEST_DONE__` handling.
