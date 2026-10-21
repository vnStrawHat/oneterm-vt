# Agent Workflow

The main session is the orchestrator. It only orchestrates (assigns work, relays results to the user) and never implements.

## Delivery loop

1. Orchestrator assigns a work item.
2. `architect` writes a spec folder `docs/specs/NNNN-short-title/` (`README.md` index plus one short file per topic group).
3. (Optional, for risky or large specs) `advisor` reviews the spec.
4. `coder` implements the spec, with unit tests and a passing quality gate.
5. `coder-lite` re-runs the quality gate and reports evidence.
6. `advisor` reviews the diff against the spec and `docs/agents/code-style/`.
7. Orchestrator reports to the user.

Deviations from the spec go back to the architect (or the orchestrator), never silently into code.

Small mechanical tasks (dependency bump, fmt fix, doc edit) skip the spec and go straight to `coder-lite`.

## Agents

| Agent | Model | Effort | May edit code? | Typical tasks |
|---|---|---|---|---|
| architect | opus | high | No (docs/ only) | Crate boundaries, public APIs, async architecture, specs, spec compliance review |
| coder | sonnet | high | Yes | Implement specs, unit tests, full quality gate |
| coder-lite | sonnet | low | Yes (small/mechanical) | Config, dependency bumps, scaffolding, fmt/clippy fixes, doc updates, env setup, gate runs |
| advisor | fable | medium | No (read-only) | Spec and diff review, risk analysis, crate/API research with sources |

## Parallel work with worktrees

Independent work items may run in parallel, one agent per worktree (`EnterWorktree` or `claude --worktree <name>`). Worktrees are created under `.claude/worktrees/<name>/` and share the root `target/` and `.cargo-home/`, so dependencies compile once. Cargo locks the target dir while it builds, so builds across worktrees run one at a time; this is expected.

## Documentation size

Docs stay short: split by topic group into files of at most ~120 lines; every folder has a README.md index.
