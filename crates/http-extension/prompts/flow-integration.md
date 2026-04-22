# HTTP Nodes in Flow JSON

When building multi-card flows, HTTP nodes sit in the `cards[]` array alongside AdaptiveCard entries.

## HTTP Node Format

```json
{
  "id": "api_<descriptive_name>",
  "type": "http",
  "config": {
    "url": "/api/<path>",
    "method": "POST",
    "body_mapping": {
      "field_name": "${input_id_from_previous_card}"
    }
  }
}
```

## Rules

- HTTP node ID MUST start with `api_` prefix
- `url` is always a relative path (no hostname) — starts with `/`
- `method`: GET, POST, PUT, DELETE, PATCH (default POST)
- `body_mapping` keys use `${field_id}` matching Input element IDs from the previous card. Only include for POST/PUT/PATCH, not GET/DELETE.
- Place HTTP node BETWEEN the form card and the result card in the cards array
- The card before the HTTP node should have `nextCardId` pointing to the HTTP node's ID
- The card after the HTTP node receives the API response data
- Do NOT include `base_url`, `auth_type`, or `auth_token` — these are configured at runtime via `gtc setup`
- Do NOT add HTTP nodes for static navigation (menu → submenu)

## Placement Patterns

- Form submission: `form_card → api_create_X → confirmation_card`
- Fetch list: `menu_card → api_fetch_X_list → list_card`
- Fetch detail: `list_card → api_fetch_X_detail → detail_card`
- Update record: `edit_card → api_update_X → success_card`
- Delete record: `confirm_card → api_delete_X → success_card`

## Example (IT helpdesk with API)

```json
{
  "flow": "it_helpdesk",
  "cards": [
    { "id": "welcome", "card": { "type": "AdaptiveCard", "..." : "..." } },
    { "id": "create_ticket_form", "card": { "type": "AdaptiveCard", "..." : "..." } },
    { "id": "api_create_ticket", "type": "http", "config": {
        "url": "/api/tickets", "method": "POST",
        "body_mapping": { "category": "${category}", "priority": "${priority}" }
    }},
    { "id": "ticket_confirmation", "card": { "type": "AdaptiveCard", "..." : "..." } },
    { "id": "api_fetch_tickets", "type": "http", "config": {
        "url": "/api/tickets", "method": "GET"
    }},
    { "id": "ticket_list", "card": { "type": "AdaptiveCard", "..." : "..." } }
  ]
}
```

## When NOT to add HTTP nodes

- User asks for a static card flow with no API mention
- Navigation between menu screens (use nextCardId only)
- User explicitly says "no API", "static", or "demo mode"
