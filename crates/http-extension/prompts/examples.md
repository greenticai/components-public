# Examples — intent → HTTP node

### Example 1 — Simple POST with bearer auth

**User intent**: *"Post new leads to my CRM at crm.example.com, bearer auth"*

**Generated node**:
```json
{
  "node_id": "post_to_crm",
  "component": "oci://ghcr.io/greenticai/component/component-http:0.1.0",
  "config": {
    "base_url": "https://crm.example.com",
    "auth_type": "bearer",
    "auth_token": "secret:CRM_TOKEN",
    "timeout_ms": 15000,
    "default_headers": { "Content-Type": "application/json" }
  },
  "inputs": { "method": "POST", "path": "/api/leads" },
  "rationale": "Bearer auth from intent keyword; CRM_TOKEN inferred."
}
```

### Example 2 — curl command import

**User**: pastes `curl -X POST https://api.github.com/repos/x/y/issues -H 'Authorization: Bearer xxx' -d '{"title":"bug"}'`

**Action**: call `curl_to_node`. The raw token `xxx` is replaced with `secret:HTTP_TOKEN`; a warning diagnostic is attached.

### Example 3 — Post-card-submit ticket creation

**User flow**: card with `Input.Text` fields `subject` / `description` / `priority` → wants to send to `/api/tickets`.

**Action**: call `generate_from_card_submit`. Body template auto-generated as `{"subject":"${submit.subject}","description":"${submit.description}","priority":"${submit.priority}"}`.

### Example 4 — Unknown API requires clarification

**User intent**: *"Call our internal ERP over HTTP"*

**Action**: call `suggest_auth`. If `confidence: low`, ask user: "What auth does this API use — Bearer token, API key in header, Basic auth, or no auth?"
