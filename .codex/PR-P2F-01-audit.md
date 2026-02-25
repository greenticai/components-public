# PR-P2F-01: Audit component runtime return/outcome plumbing for Jump (Option A)
Date: 2026-02-24
Repo: component-pack2flow

## Objective
Audit and align the component scaffolding so pack2flow can return a structured **Jump** outcome without hacks.

This PR:
- adds no jump behavior yet
- confirms how components return outcomes to the runner in your current component model

## Audit tasks
1) Confirm component entrypoint signature
   - Identify whether components return:
     - a raw CBOR payload only, or
     - a structured result type, or
     - an enum-like outcome already (continue/wait/respond).
2) Confirm how the runner-host interprets component returns
   - Ensure there is a place where pack2flow can return Jump cleanly.
3) Decide the minimal WIT world shape for pack2flow
   - If you already have a shared “component world” interface, reuse it.
   - Avoid introducing new shared interfaces in greentic-interfaces.

## Deliverables
- `docs/pack2flow_audit.md` describing:
  - current entrypoint signature
  - how to return a Jump outcome
  - any required tiny changes to pack2flow WIT (local to this repo)

## Acceptance criteria
- CI passes, no behavior added.
- Clear plan to implement Jump return in PR-P2F-02.
