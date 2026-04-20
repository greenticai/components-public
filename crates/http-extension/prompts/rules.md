# Greentic HTTP Extension — LLM Rules

When generating HTTP call nodes for Greentic flows, follow these rules.

## Security

- **NEVER** emit a raw bearer token, API key, or password in `config.auth_token`. Always use a `secret:NAME` reference. If the user pastes a raw token (e.g. via `curl_to_node`), rewrite it as `secret:HTTP_TOKEN` and warn the user to create that secret.
- **NEVER** include user PII in the node `rationale` field — the rationale is visible in flow diffs.
- Prefer HTTPS base URLs. Warn when a user requests `http://` to a non-localhost host.

## Defaults

- `timeout_ms` defaults to **15000** (15 s). Never exceed **60000** (60 s) unless the user explicitly asks for a long-poll scenario.
- For POST/PUT/PATCH with a body, **always** set `Content-Type: application/json` unless the user specifies a different content type.
- `node_id` must be `snake_case` and start with the HTTP verb (e.g. `post_to_crm`, `get_user_profile`).

## Auth type selection

- Bearer tokens: most modern REST APIs (GitHub, Slack, Airtable, OpenAI) — use `auth_type: bearer`.
- API keys in custom headers: OpenAI (legacy), some SaaS — use `auth_type: api_key` with `api_key_header`.
- Basic auth: legacy intranet, some enterprise APIs — use `auth_type: basic`, token format `user:password`.
- Unknown API: call `suggest_auth` first. If it returns `confidence: low`, ask the user.

## Tool selection

- User describes intent in natural language → `generate_http_node`
- User pastes a `curl` command → `curl_to_node`
- User is wiring a step after an Adaptive Card submit → `generate_from_card_submit`
- After ANY generation: call `validate_http_config` and act on diagnostics before returning to the user.

## Diagnostic codes (from validate_http_config)

- `url:invalid-scheme` — reject non-http(s) URL
- `url:missing` — base_url required
- `auth:unsupported-type` — auth_type not in `{none, bearer, api_key, basic}`
- `auth:missing-token` — auth_type is not `none` but `auth_token` is empty
- `auth:bare-token` — `auth_token` does not start with `secret:` — tell user to use secret reference
- `timeout:zero` / `timeout:too-long` — timeout sanity checks
- `curl:unsupported-flag` — curl command used a flag (e.g. `-F`, `--data-urlencode`) that is not mapped 1:1 to HTTP node config
