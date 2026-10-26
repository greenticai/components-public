# greentic.email-trigger

Start a Greentic flow when mail arrives in an IMAP mailbox.

Carries one node type, **Email Trigger** (`trigger.email`), under trigger
contract v1 and the `email_imap` addendum. The runtime (greentic-start) polls
the mailbox and starts the flow for each new message; the flow is entered at
the node after the trigger. Nothing listens, so there is no public address and
no inbound port.

## The node

Required: `host`, `username`, `password_ref`.

| field | meaning |
|---|---|
| `host` | IMAP server name, e.g. `imap.gmail.com` |
| `tls` | `tls` (default, port 993) or `starttls` (port 143). Certificates are always verified |
| `username` | mailbox login |
| `password_ref` | the **name** of the secret holding the password, as `<provider>/<key>` (e.g. `email/imap_password`). Never the password |
| `mailbox`, `port`, `poll_interval_s` (≥ 30) | where and how often |
| `search` | `{unseen, from, subject}`; default unread mail |
| `on_success` | `mark_seen` (default), `move` (needs `move_to`) or `none` (needs `search.unseen: false`) |
| `max_messages_per_poll`, `max_message_bytes`, `include_html`, `attachments` | limits and body shape |
| `trigger_id`, `enabled` | stable id and kill switch |

The password is set once in the deployment's secrets under the name the node
points at; it is not stored in the flow, the pack or any export.

## Tool

`validate_email_trigger` checks a node with the same rules greentic-designer's
emitter and greentic-start's parser apply, so a mistake is named on the canvas
instead of refusing the whole pack at deploy.

## Status

Unverified against a real mail server: the host's IMAP client has only been
exercised against an in-process fake server and mocks.
Needs a designer carrying the `email_imap` gate opened (greentic-designer
#3794) and a deploy of it.

## Build

```bash
crates/email-trigger-extension/build.sh   # needs cargo-component, wasm32-wasip2
```

Publish by pushing a tag `email-trigger-ext-vX.Y.Z`.
