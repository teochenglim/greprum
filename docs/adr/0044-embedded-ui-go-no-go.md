# ADR 0044 — Embedded UI go/no-go

Status: Accepted

## Context

PRD §9/§10 phase 6: build an embedded UI only if Grafana shows a gap. The v0.6.0 gap analysis
(`docs/grafana-gap-analysis.md`) found funnel steps and session drill-down covered, and a
journey *diagram* as the only gap (the transitions table covers the question).

## Decision

No-go for v1.0.0: ship Grafana dashboards only. Revisit if product owners ask for a visual
journey diagram or funnels with more than four steps.

## Consequences

No `ui/` application to build, secure or serve. The binary stays one process with no frontend
toolchain. A future embedded UI would get its own release file (v1.1.0).
