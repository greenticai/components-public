//! Static metadata for the Stripe tools: names, JSON schemas, capability
//! flags, the `agentic_worker` metadata blob, and the secret requirements
//! each tool declares. Pure (no WIT imports) so it is fully host-testable;
//! `lib.rs` maps [`ToolMeta`] onto the WIT `ToolDefinition` shape (which has
//! no `secret_requirements` field — that list is consumed by the
//! `describe.json` authoring step, in a later task).
//!
//! This is the template the remaining Stripe tool domains (invoices,
//! subscriptions, checkout_sessions, refunds) extend: add a
//! `const <NAME>_TOOL` name, a builder function returning a [`ToolMeta`], and
//! push it onto the `vec![...]` in [`all_tools`]. Wire the matching dispatch
//! arm in `lib.rs::invoke_tool`.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
pub const CUSTOMERS_TOOL: &str = "stripe_customers";
pub const PRODUCTS_TOOL: &str = "stripe_products";
pub const PRICES_TOOL: &str = "stripe_prices";
pub const PAYMENT_LINKS_TOOL: &str = "stripe_payment_links";
pub const INVOICES_TOOL: &str = "stripe_invoices";
pub const SUBSCRIPTIONS_TOOL: &str = "stripe_subscriptions";
pub const CHECKOUT_SESSIONS_TOOL: &str = "stripe_checkout_sessions";
pub const REFUNDS_TOOL: &str = "stripe_refunds";
pub const PAYMENT_INTENTS_TOOL: &str = "stripe_payment_intents";
pub const DISPUTES_TOOL: &str = "stripe_disputes";
pub const COUPONS_TOOL: &str = "stripe_coupons";
pub const PROMOTION_CODES_TOOL: &str = "stripe_promotion_codes";
pub const FILES_TOOL: &str = "stripe_files";

/// Plain (non-WIT) description of one tool definition.
pub struct ToolMeta {
    pub name: String,
    pub description: String,
    pub input_schema_json: String,
    pub output_schema_json: String,
    pub capabilities: Vec<String>,
    pub agentic_worker_metadata: String,
    pub secret_requirements: Vec<SecretRequirement>,
}

/// One secret the tool may read, surfaced to `describe.json` authoring so
/// the runtime can grant/prompt for it. Mirrors the shape used in Jira's and
/// HubSpot's `describe.json` `contributions.tools[].secret_requirements`.
pub struct SecretRequirement {
    /// Secret key, without the `secret://` scheme (e.g. `"stripe/secret_key"`).
    pub key: String,
    pub required: bool,
    pub description: String,
    pub format: String,
}

/// The single `stripe/secret_key` secret every Stripe tool reads. Stripe has
/// one credential shape (no auth-mode selector, unlike Jira), so every tool
/// in this extension declares exactly this one requirement.
/// `required:false` mirrors the other extensions' convention (the runtime
/// prompts/grants it, but does not hard-fail `describe()` authoring on its
/// absence).
fn stripe_secret_requirements() -> Vec<SecretRequirement> {
    vec![SecretRequirement {
        key: "stripe/secret_key".to_string(),
        required: false,
        description: "Stripe secret API key (sk_live_… / sk_test_…), used as the HTTP Bearer credential for every Stripe REST call. Resolved by the host from secret://stripe/secret_key and never returned to the model.".to_string(),
        format: "text".to_string(),
    }]
}

fn customers_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "search", "get", "update", "delete"], "description": "Which Stripe customer action to perform." },
    "id": { "type": "string", "description": "Customer id (e.g. \"cus_123\"). Required for get, update, and delete." },
    "params": { "type": "object", "additionalProperties": true, "description": "Customer fields to set (e.g. email, name, description, metadata). Required for create and update; passed through to Stripe as-is." },
    "query": { "type": "string", "description": "Stripe search-query string (e.g. \"email:'jane@example.com'\"). Required for search." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for search." }
  }
}"#
    .to_string()
}

fn customers_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get/update: a single customer {id,email,name,created}. For search: {total,results:[{id,email,name,created}]}. For delete: {ok,id}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "email": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "created": { "type": ["integer", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" }
  }
}"#
    .to_string()
}

fn customers_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Stripe customers. Set 'operation' to create, search, get, update, or delete. Search by email before creating to avoid duplicates; confirm with the user before delete.",
  "examples": [
    { "when": "creating a new customer", "input": { "operation": "create", "params": { "email": "jane@example.com", "name": "Jane Doe" } } },
    { "when": "finding a customer by email", "input": { "operation": "search", "query": "email:'jane@example.com'" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn customers_tool() -> ToolMeta {
    ToolMeta {
        name: CUSTOMERS_TOOL.to_string(),
        description:
            "Create, search, get, update, or delete Stripe customers. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: customers_input_schema(),
        output_schema_json: customers_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: customers_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn products_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "list", "get", "update"], "description": "Which Stripe product action to perform." },
    "id": { "type": "string", "description": "Product id (e.g. \"prod_123\"). Required for get and update." },
    "params": { "type": "object", "additionalProperties": true, "description": "Product fields to set. Required for create (must include 'name') and update; passed through to Stripe as-is." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list." },
    "active": { "type": "boolean", "description": "Filter by active status, for list." }
  }
}"#
    .to_string()
}

fn products_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get/update: a single product {id,name,active}. For list: {total,results:[{id,name,active}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "active": { "type": ["boolean", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn products_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Stripe products (the sellable items priced by stripe_prices). Set 'operation' to create (params must include 'name'), list (optionally filtered by 'active'), get, or update.",
  "examples": [
    { "when": "creating a new product", "input": { "operation": "create", "params": { "name": "Pro Plan" } } },
    { "when": "listing active products", "input": { "operation": "list", "active": true } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn products_tool() -> ToolMeta {
    ToolMeta {
        name: PRODUCTS_TOOL.to_string(),
        description:
            "Create, list, get, or update Stripe products. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: products_input_schema(),
        output_schema_json: products_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: products_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn prices_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "list", "get"], "description": "Which Stripe price action to perform." },
    "id": { "type": "string", "description": "Price id (e.g. \"price_123\"). Required for get." },
    "params": { "type": "object", "additionalProperties": true, "description": "Price fields to set. Required for create; must include 'unit_amount', 'currency', and 'product'; passed through to Stripe as-is." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list." },
    "product": { "type": "string", "description": "Filter by product id, for list." }
  }
}"#
    .to_string()
}

fn prices_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get: a single price {id,unit_amount,currency,product}. For list: {total,results:[{id,unit_amount,currency,product}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "unit_amount": { "type": ["integer", "null"] },
    "currency": { "type": ["string", "null"] },
    "product": { "type": ["string", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn prices_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Stripe prices (what a product costs). Set 'operation' to create (params must include 'unit_amount', 'currency', and 'product'), list (optionally filtered by 'product'), or get.",
  "examples": [
    { "when": "pricing a product at $10/month", "input": { "operation": "create", "params": { "unit_amount": 1000, "currency": "usd", "product": "prod_123", "recurring": { "interval": "month" } } } },
    { "when": "listing prices for a product", "input": { "operation": "list", "product": "prod_123" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn prices_tool() -> ToolMeta {
    ToolMeta {
        name: PRICES_TOOL.to_string(),
        description:
            "Create, list, or get Stripe prices. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: prices_input_schema(),
        output_schema_json: prices_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: prices_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn payment_links_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "get", "list", "deactivate"], "description": "Which Stripe payment link action to perform." },
    "id": { "type": "string", "description": "Payment link id (e.g. \"plink_123\"). Required for get and deactivate." },
    "line_items": { "type": "array", "items": { "type": "object", "properties": { "price": { "type": "string" }, "quantity": { "type": "integer" } }, "required": ["price", "quantity"] }, "description": "Array of {price,quantity}. Required for create." },
    "params": { "type": "object", "additionalProperties": true, "description": "Additional create fields (e.g. after_completion), merged with line_items; optional." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list." }
  }
}"#
    .to_string()
}

fn payment_links_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get/deactivate: a single payment link {id,url,active}. For list: {total,results:[{id,url,active}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "url": { "type": ["string", "null"] },
    "active": { "type": ["boolean", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn payment_links_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Stripe payment links (shareable checkout URLs). Set 'operation' to create (needs 'line_items'), get, list, or deactivate. Confirm with the user before deactivate.",
  "examples": [
    { "when": "creating a payment link for one item", "input": { "operation": "create", "line_items": [{ "price": "price_123", "quantity": 1 }] } },
    { "when": "listing existing payment links", "input": { "operation": "list" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn payment_links_tool() -> ToolMeta {
    ToolMeta {
        name: PAYMENT_LINKS_TOOL.to_string(),
        description:
            "Create, get, list, or deactivate Stripe payment links. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: payment_links_input_schema(),
        output_schema_json: payment_links_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: payment_links_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn invoices_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "list", "get", "finalize", "send", "void"], "description": "Which Stripe invoice action to perform." },
    "id": { "type": "string", "description": "Invoice id (e.g. \"in_123\"). Required for get, finalize, send, and void." },
    "customer": { "type": "string", "description": "Customer id to bill. Required for create; optional filter for list." },
    "params": { "type": "object", "additionalProperties": true, "description": "Additional create fields, merged with customer; optional." },
    "status": { "type": "string", "description": "Filter by invoice status (e.g. \"draft\", \"open\", \"paid\"), for list." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list." }
  }
}"#
    .to_string()
}

fn invoices_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get/finalize/send/void: a single invoice {id,customer,status,total,hosted_invoice_url}. For list: {total,results:[{id,customer,status,total,hosted_invoice_url}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "customer": { "type": ["string", "null"] },
    "status": { "type": ["string", "null"] },
    "total": { "type": ["integer", "null"] },
    "hosted_invoice_url": { "type": ["string", "null"] },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn invoices_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Stripe invoices. Set 'operation' to create (needs 'customer'), list (optionally filtered by 'customer'/'status'), get, finalize, send, or void. Confirm with the user before finalize, send, or void — they change customer-visible billing state.",
  "examples": [
    { "when": "drafting an invoice for a customer", "input": { "operation": "create", "customer": "cus_123" } },
    { "when": "finalizing a draft invoice", "input": { "operation": "finalize", "id": "in_123" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": true
}"#
    .to_string()
}

fn invoices_tool() -> ToolMeta {
    ToolMeta {
        name: INVOICES_TOOL.to_string(),
        description:
            "Create, list, get, finalize, send, or void Stripe invoices. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: invoices_input_schema(),
        output_schema_json: invoices_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: invoices_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn subscriptions_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "get", "list", "update", "cancel"], "description": "Which Stripe subscription action to perform." },
    "id": { "type": "string", "description": "Subscription id (e.g. \"sub_123\"). Required for get, update, and cancel." },
    "customer": { "type": "string", "description": "Customer id to subscribe. Required for create; optional filter for list." },
    "items": { "type": "array", "items": { "type": "object", "properties": { "price": { "type": "string" } }, "required": ["price"] }, "description": "Array of {price}. Required for create." },
    "params": { "type": "object", "additionalProperties": true, "description": "Additional create fields, merged with customer/items; the full update body for update." },
    "status": { "type": "string", "description": "Filter by subscription status (e.g. \"active\", \"canceled\", \"past_due\"), for list." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list." }
  }
}"#
    .to_string()
}

fn subscriptions_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get/update/cancel: a single subscription {id,customer,status,current_period_end}. For list: {total,results:[{id,customer,status,current_period_end}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "customer": { "type": ["string", "null"] },
    "status": { "type": ["string", "null"] },
    "current_period_end": { "type": ["integer", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn subscriptions_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Stripe subscriptions. Set 'operation' to create (needs 'customer' and 'items'), get, list (optionally filtered by 'customer'/'status'), update, or cancel. Confirm with the user before cancel.",
  "examples": [
    { "when": "subscribing a customer to a price", "input": { "operation": "create", "customer": "cus_123", "items": [{ "price": "price_123" }] } },
    { "when": "canceling a subscription", "input": { "operation": "cancel", "id": "sub_123" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": true
}"#
    .to_string()
}

fn subscriptions_tool() -> ToolMeta {
    ToolMeta {
        name: SUBSCRIPTIONS_TOOL.to_string(),
        description:
            "Create, get, list, update, or cancel Stripe subscriptions. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: subscriptions_input_schema(),
        output_schema_json: subscriptions_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: subscriptions_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn checkout_sessions_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "get"], "description": "Which Stripe Checkout Session action to perform." },
    "id": { "type": "string", "description": "Checkout Session id (e.g. \"cs_123\"). Required for get." },
    "params": { "type": "object", "additionalProperties": true, "description": "Checkout Session fields (e.g. mode, line_items, success_url). Required for create; passed through to Stripe as-is." }
  }
}"#
    .to_string()
}

fn checkout_sessions_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "A single Checkout Session {id,url,payment_status,mode}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "url": { "type": ["string", "null"] },
    "payment_status": { "type": ["string", "null"] },
    "mode": { "type": ["string", "null"] }
  }
}"#
    .to_string()
}

fn checkout_sessions_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Create or look up a Stripe Checkout Session (a hosted payment page). Set 'operation' to create (params must include 'mode', 'line_items', and 'success_url') or get.",
  "examples": [
    { "when": "creating a one-time payment checkout page", "input": { "operation": "create", "params": { "mode": "payment", "line_items": [{ "price": "price_123", "quantity": 1 }], "success_url": "https://example.com/success" } } },
    { "when": "checking a checkout session's payment status", "input": { "operation": "get", "id": "cs_123" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn checkout_sessions_tool() -> ToolMeta {
    ToolMeta {
        name: CHECKOUT_SESSIONS_TOOL.to_string(),
        description:
            "Create or get Stripe Checkout Sessions (hosted payment pages). The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: checkout_sessions_input_schema(),
        output_schema_json: checkout_sessions_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: checkout_sessions_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn refunds_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "get", "list"], "description": "Which Stripe refund action to perform." },
    "id": { "type": "string", "description": "Refund id (e.g. \"re_123\"). Required for get." },
    "payment_intent": { "type": "string", "description": "Payment intent to refund. create/list require at least one of payment_intent/charge." },
    "charge": { "type": "string", "description": "Charge to refund. create/list require at least one of payment_intent/charge." },
    "amount": { "type": "integer", "description": "Amount to refund, in the currency's smallest unit. Optional for create — omit to refund the full remaining amount." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list." }
  }
}"#
    .to_string()
}

fn refunds_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get: a single refund {id,amount,status,payment_intent,charge}. For list: {total,results:[{id,amount,status,payment_intent,charge}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "amount": { "type": ["integer", "null"] },
    "status": { "type": ["string", "null"] },
    "payment_intent": { "type": ["string", "null"] },
    "charge": { "type": ["string", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn refunds_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Issue and look up Stripe refunds. Money-moving — always confirm with the user before create. Set 'operation' to create (needs 'payment_intent' or 'charge'; 'amount' is optional and defaults to a full refund), get, or list.",
  "examples": [
    { "when": "refunding a payment in full", "input": { "operation": "create", "payment_intent": "pi_123" } },
    { "when": "partially refunding a charge", "input": { "operation": "create", "charge": "ch_123", "amount": 500 } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": true
}"#
    .to_string()
}

fn refunds_tool() -> ToolMeta {
    ToolMeta {
        name: REFUNDS_TOOL.to_string(),
        description:
            "Create, get, or list Stripe refunds. Money-moving — the auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: refunds_input_schema(),
        output_schema_json: refunds_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: refunds_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn payment_intents_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "get", "list", "confirm", "cancel"], "description": "Which Stripe PaymentIntent action to perform." },
    "id": { "type": "string", "description": "PaymentIntent id (e.g. \"pi_123\"). Required for get, confirm, and cancel." },
    "amount": { "type": "integer", "description": "Amount in the currency's smallest unit (e.g. cents). Required for create." },
    "currency": { "type": "string", "description": "Three-letter ISO 4217 currency code (e.g. \"usd\"). Required for create." },
    "customer": { "type": "string", "description": "Customer id to associate with the PaymentIntent. Optional for create; optional filter for list." },
    "params": { "type": "object", "additionalProperties": true, "description": "Additional fields merged into the create body; passed verbatim on confirm/cancel." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list." }
  }
}"#
    .to_string()
}

fn payment_intents_output_schema() -> String {
    r#"{"type":"object","description":"For create/get/confirm/cancel: {id,amount,currency,status,client_secret}. For list: {total,results:[...]}.","properties":{"id":{"type":["string","null"]},"amount":{"type":["integer","null"]},"currency":{"type":["string","null"]},"status":{"type":["string","null"]},"client_secret":{"type":["string","null"]},"total":{"type":"integer"},"results":{"type":"array","items":{"type":"object"}}}}"#
        .to_string()
}

fn payment_intents_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Stripe PaymentIntents: create (needs amount + currency), get, list, confirm, or cancel. Confirm before charging; treat client_secret as sensitive.",
  "examples": [
    { "when": "creating a payment intent", "input": { "operation": "create", "amount": 2000, "currency": "usd" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": true
}"#
    .to_string()
}

fn payment_intents_tool() -> ToolMeta {
    ToolMeta {
        name: PAYMENT_INTENTS_TOOL.to_string(),
        description:
            "Create, retrieve, list, confirm, or cancel Stripe PaymentIntents. The Stripe secret key is injected by the host and never returned."
                .to_string(),
        input_schema_json: payment_intents_input_schema(),
        output_schema_json: payment_intents_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: payment_intents_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn disputes_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["get", "list", "update", "close"], "description": "Which Stripe dispute action to perform." },
    "id": { "type": "string", "description": "Dispute id (e.g. \"dp_123\"). Required for get, update, and close." },
    "charge": { "type": "string", "description": "Charge id to filter disputes on list." },
    "payment_intent": { "type": "string", "description": "Payment intent id to filter disputes on list." },
    "params": { "type": "object", "additionalProperties": true, "description": "Evidence and metadata fields for update (e.g. evidence, metadata). Required for update; passed through to Stripe as-is." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list." }
  }
}"#
    .to_string()
}

fn disputes_output_schema() -> String {
    r#"{"type":"object","description":"For get/update/close: {id,amount,currency,status,reason,charge}. For list: {total,results}."}"#
        .to_string()
}

fn disputes_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Stripe disputes (chargebacks): get, list, update to submit evidence (params.evidence), or close. Closing forfeits the dispute — always confirm with the user before close.",
  "examples": [
    { "when": "listing open disputes", "input": { "operation": "list" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": true
}"#
    .to_string()
}

fn disputes_tool() -> ToolMeta {
    ToolMeta {
        name: DISPUTES_TOOL.to_string(),
        description:
            "Get, list, update (submit evidence), or close Stripe disputes (chargebacks). The Stripe secret key is injected by the host and never returned."
                .to_string(),
        input_schema_json: disputes_input_schema(),
        output_schema_json: disputes_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: disputes_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn coupons_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "get", "list", "delete"], "description": "Which Stripe coupon action to perform." },
    "id": { "type": "string", "description": "Coupon id. Required for get and delete." },
    "params": { "type": "object", "additionalProperties": true, "description": "Coupon fields for create (must include 'duration' and one of 'percent_off'/'amount_off'). Required for create; passed through to Stripe as-is." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list." }
  }
}"#
    .to_string()
}

fn coupons_output_schema() -> String {
    r#"{"type":"object","description":"For create/get: {id,name,percent_off,amount_off,duration,valid}. For delete: {id,deleted}. For list: {total,results}."}"#
        .to_string()
}

fn coupons_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Stripe coupons: create (params needs duration + percent_off or amount_off), get, list, or delete.",
  "examples": [
    { "when": "creating a 25% forever coupon", "input": { "operation": "create", "params": { "percent_off": 25, "duration": "forever" } } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn coupons_tool() -> ToolMeta {
    ToolMeta {
        name: COUPONS_TOOL.to_string(),
        description:
            "Create, get, list, or delete Stripe coupons. The Stripe secret key is injected by the host and never returned."
                .to_string(),
        input_schema_json: coupons_input_schema(),
        output_schema_json: coupons_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: coupons_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn promotion_codes_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "get", "list", "update"], "description": "Which Stripe promotion code action to perform." },
    "id": { "type": "string", "description": "Promotion code id (e.g. \"promo_123\"). Required for get and update." },
    "coupon": { "type": "string", "description": "Coupon id this promotion code redeems. Required for create; optional filter for list." },
    "params": { "type": "object", "additionalProperties": true, "description": "Additional create fields merged with coupon (e.g. code, max_redemptions, expires_at). Required for update; passed through to Stripe as-is." },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list." }
  }
}"#
    .to_string()
}

fn promotion_codes_output_schema() -> String {
    r#"{"type":"object","description":"For create/get/update: {id,code,coupon,active}. For list: {total,results}."}"#
        .to_string()
}

fn promotion_codes_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Stripe promotion codes: create (needs a coupon id), get, list, or update. A promotion code is the customer-facing code that redeems a coupon.",
  "examples": [
    { "when": "creating a promo code for a coupon", "input": { "operation": "create", "coupon": "coup_1", "params": { "code": "SUMMER25" } } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn promotion_codes_tool() -> ToolMeta {
    ToolMeta {
        name: PROMOTION_CODES_TOOL.to_string(),
        description:
            "Create, get, list, or update Stripe promotion codes (customer-facing codes that redeem a coupon). The Stripe secret key is injected by the host and never returned."
                .to_string(),
        input_schema_json: promotion_codes_input_schema(),
        output_schema_json: promotion_codes_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: promotion_codes_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

fn files_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["file_base64"],
  "properties": {
    "file_base64": { "type": "string", "description": "The file bytes, base64-encoded. The host decodes this and uploads it as binary to Stripe's Files API." },
    "filename": { "type": "string", "description": "Suggested filename for the uploaded file (e.g. \"evidence.pdf\"). Optional — defaults to \"upload\"." },
    "purpose": { "type": "string", "description": "Stripe file purpose (e.g. \"dispute_evidence\"). Optional — defaults to \"dispute_evidence\"." }
  }
}"#
    .to_string()
}

fn files_output_schema() -> String {
    r#"{"type":"object","description":"The uploaded Stripe file: {id,purpose,size,type,url}."}"#
        .to_string()
}

fn files_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Upload a file to Stripe (multipart) and get its id — e.g. dispute evidence to attach via stripe_disputes update params.evidence. purpose defaults to dispute_evidence.",
  "examples": [
    { "when": "uploading dispute evidence", "input": { "file_base64": "<base64 bytes>", "filename": "evidence.pdf", "purpose": "dispute_evidence" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn files_tool() -> ToolMeta {
    ToolMeta {
        name: FILES_TOOL.to_string(),
        description:
            "Upload a file to Stripe (e.g. dispute evidence) and get its file id. Provide file_base64 (the file bytes, base64-encoded). The Stripe secret key is injected by the host and never returned."
                .to_string(),
        input_schema_json: files_input_schema(),
        output_schema_json: files_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: files_agentic_worker_metadata(),
        secret_requirements: stripe_secret_requirements(),
    }
}

/// All Stripe tool definitions this extension offers.
#[must_use]
pub fn all_tools() -> Vec<ToolMeta> {
    vec![
        customers_tool(),
        products_tool(),
        prices_tool(),
        payment_links_tool(),
        invoices_tool(),
        subscriptions_tool(),
        checkout_sessions_tool(),
        refunds_tool(),
        payment_intents_tool(),
        disputes_tool(),
        coupons_tool(),
        promotion_codes_tool(),
        files_tool(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_all_thirteen_stripe_tools() {
        let names: Vec<String> = all_tools().into_iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            vec![
                CUSTOMERS_TOOL,
                PRODUCTS_TOOL,
                PRICES_TOOL,
                PAYMENT_LINKS_TOOL,
                INVOICES_TOOL,
                SUBSCRIPTIONS_TOOL,
                CHECKOUT_SESSIONS_TOOL,
                REFUNDS_TOOL,
                PAYMENT_INTENTS_TOOL,
                DISPUTES_TOOL,
                COUPONS_TOOL,
                PROMOTION_CODES_TOOL,
                FILES_TOOL,
            ]
        );
    }

    #[test]
    fn money_moving_tools_require_confirmation() {
        let tools = all_tools();
        for tool in &tools {
            let meta: serde_json::Value = serde_json::from_str(&tool.agentic_worker_metadata)
                .unwrap_or_else(|_| panic!("{} aw metadata invalid", tool.name));
            let flag = meta
                .get("confirmation_required")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or_else(|| panic!("{} missing confirmation_required", tool.name));
            let expected = matches!(
                tool.name.as_str(),
                "stripe_refunds"
                    | "stripe_payment_intents"
                    | "stripe_disputes"
                    | "stripe_subscriptions"
                    | "stripe_invoices"
            );
            assert_eq!(
                flag, expected,
                "{} confirmation_required should be {expected}",
                tool.name
            );
        }
    }

    #[test]
    fn every_tool_declares_agentic_worker() {
        for tool in all_tools() {
            assert!(
                tool.capabilities.iter().any(|cap| cap == "agentic_worker"),
                "{} must opt into the agentic worker",
                tool.name
            );
        }
    }

    #[test]
    fn every_tool_declares_exactly_one_optional_stripe_secret() {
        for tool in all_tools() {
            assert_eq!(tool.secret_requirements.len(), 1);
            let req = &tool.secret_requirements[0];
            assert_eq!(req.key, "stripe/secret_key");
            assert!(!req.required);
        }
    }

    #[test]
    fn all_schemas_and_metadata_are_valid_json() {
        for tool in all_tools() {
            serde_json::from_str::<serde_json::Value>(&tool.input_schema_json)
                .unwrap_or_else(|_| panic!("{} input schema invalid", tool.name));
            serde_json::from_str::<serde_json::Value>(&tool.output_schema_json)
                .unwrap_or_else(|_| panic!("{} output schema invalid", tool.name));
            serde_json::from_str::<serde_json::Value>(&tool.agentic_worker_metadata)
                .unwrap_or_else(|_| panic!("{} aw metadata invalid", tool.name));
        }
    }
}
