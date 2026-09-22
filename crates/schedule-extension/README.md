# schedule-extension

A Greentic Designer **design** extension that ships the schedule (cron) trigger nodeType.

- id: `greentic.schedule`
- version: `1.0.0`
- contract: `greentic:extension-design@0.1.0`
- trigger contract: **v1** (`greentic.triggers.v1`), kind `cron`

## What it does

A schedule trigger starts a flow on a repeating schedule. greentic-start's
scheduler fires it when the cron expression comes due and enters the flow at
the node AFTER the trigger; nothing in the flow needs to know it was a timer
rather than a message.

This extension ships the node type and its JSON Schema, plus the design-time
tools the designer LLM calls while an operator configures one. The runtime half
lives in greentic-start (`src/triggers/scheduler.rs`); the contract both sides
build against is `docs/trigger-contract-v1.md` in greentic-designer.

### The schedule is parsed the way the runtime parses it

`src/tools/cron_expr.rs` is the single reader, and it reproduces the runtime's
rule exactly: the `cron` crate wants six fields, an operator writes the five of
an ordinary crontab line, and both greentic-start and the designer's emitter
bridge that by prepending a seconds field.

That is not a convenience. A design-time validator that is only *nearly* the
runtime's parser moves a class of typos off the canvas — where the operator can
still see the node — into a deploy that refuses minutes later, for exactly the
expressions the two disagree about. If the runtime's rule ever changes, this
follows it rather than approximating it.

The same reasoning decides the zone: `timezone` must be an IANA name, because
that is what `chrono_tz` resolves. `WIB` and `+07:00` are what an operator
reaches for first and are precisely the values that would deploy and then never
resolve, so both are refused here.

### Why the trigger kind is declared inside `config_schema`

The designer decides a node emits a trigger declaration by reading
`x-trigger-kind` at the root of this node type's `config_schema`. It is NOT a
`contributions.trigger_kinds` key: `greentic_extension_sdk_contract`'s
`Contributions` and `NodeType` are both `deny_unknown_fields`, so a new key
there makes every designer refuse the whole describe and unload the extension.
`config_schema` is a free-form schema carried as a string, so an `x-` keyword
inside it is invisible to the typed reader and ignored by every schema
validator. `src/describe_tests.rs` pins both halves, plus that the schema names
the fields at the level the emitter reads them (`expr` and `timezone` at the
top of the node config, never nested under a `cron` object — that nesting is
what the emitted `assets/triggers.json` looks like, not what the node config
looks like).

## Design-time tools

- `validate_cron` — diagnostics on a schedule trigger config block. A missing
  or unparseable `expr` and a non-IANA zone are errors; a five-field
  expression, an absent zone and a dropped legacy field are warnings that say
  what happens instead
- `describe_schedule` — the schedule in words plus its next five fire times in
  the declared zone. The prose is best effort and **says so** when it cannot
  describe an expression; the fire times are always exact, because they come
  from the same crate the runtime schedules with
- `suggest_cron` — a cron expression and zone from a phrase such as "every
  weekday at 9am Jakarta time". It **abstains** with a reason when the phrasing
  is not one it recognises: an unattended worker runs when its schedule says it
  does, and a guessed expression that is close but wrong fires quietly for
  weeks before anyone notices

## Build

```bash
bash crates/schedule-extension/build.sh
ls -lh crates/schedule-extension/dist/
```

## Publish

Store publish via CI (mirrors `webhook-extension`):
1. Bump `version` in both `describe.json` and `Cargo.toml` (a test fails if they disagree)
2. Commit + push to main
3. Tag: `git tag schedule-ext-v<version> && git push origin schedule-ext-v<version>`
4. A `publish-schedule-extension` workflow posts the `.gtxpack` to the Store

## Layout

- `describe.json`         — extension manifest with the `trigger.schedule` nodeType + inline JSON Schema
- `src/lib.rs`            — WASM guest exports (tool dispatch; prompting / validation / knowledge are no-op stubs)
- `src/tools/cron_expr.rs` — the single cron/zone reader, matching the runtime
- `src/tools/`            — the three design-time tools
- `src/describe_tests.rs` — what `describe.json` must keep saying
- `wit/`                  — WIT contract
- `i18n/`                 — locale catalogs
