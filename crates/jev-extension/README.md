# component-jev-ext — `greentic.jev`

Typed decisions with calibrated confidence from [Jev](https://typesafe.ai/) (TypeSafe AI's
System One model) for a Greentic agentic worker, via one tool, `decide`.
Install the `.gtxpack`, set one secret, done.

Jev does not generate text. It answers narrow typed questions about some data and returns
probabilities and a confidence, so a worker can act when it is sure and escalate when it is not.

## Tool

`decide(state, questions, model?)` → `{ model, answers, usage? }`

- `state`: the data to evaluate, text or a JSON object.
- `questions`: keyed by an id you choose; each answer comes back under the same id.
  - `choice`: `criteria` is an object of option key to description (two or more) →
    `choice`, `probabilities`, `confidence`.
  - `score`: `criteria` is an ordered array of level descriptions (two or more) →
    `score`, `probabilities`, `confidence`.
  - `noul`: a yes/no question, no `criteria` → `noul` (0 to 1).
- `model` defaults to `jev-latest`.

Answers are passed through exactly as the API sent them. A reply with no answers is an error,
never an empty decision. API reference: https://docs.typesafe.ai/introduction/quickstart

The extension proxies `POST https://api.typesafe.ai/v1/systemone` through the host `http`
capability, authenticating with `secret://jev/api_key` (never returned). Outbound calls are
restricted to `api.typesafe.ai`.

## Secret

Set `jev/api_key` in the tenant secret store. Create the key in https://console.typesafe.ai/.

## Build & install

```bash
bash ci/local_check.sh          # fmt + clippy + test + wasm build
gtdx publish --registry local --sign --key <pem>   # a valid, signed .gtxpack
gtdx install <that .gtxpack>
```

`build.sh` produces an unsigned pack with no `manifest.json`, which `gtdx install` refuses.
Use `gtdx publish` for anything you install.

## Wiring into a worker

```yaml
tools:
  - extension_id: greentic.jev
    tool_name: decide
```

## Status

Builds, passes its tests and loads in the designer (`gtdx doctor`). **Not yet called against the
real API**: the docs publish no error codes or rate limits, so those mappings are best effort.
There is no flow node yet; that needs a separate component published to the registry.
