#!/usr/bin/env bash
# Start a local Sorx business test page with an embedded WebChat GUI and a
# same-origin proxy for exercising component-sorx-business operations.
#
# Usage:
#   scripts/test_sorx.sh [--port 8798] [--host 127.0.0.1] [--sorx-url http://127.0.0.1:8787] [--no-open]
#                        [--webchat-assets PATH] [--skin default]
#
# Environment:
#   PORT                Default port when --port is omitted.
#   SORX_URL            Default Sorx URL shown in the page.
#   SORX_TEST_OPEN=0    Do not try to open a browser.
#   WEBCHAT_ASSET_DIR   Directory containing webchat-gui assets.
#   SORX_BUSINESS_COMPONENT_CLI
#                       Optional path to a prebuilt component-sorx-business-cli.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-8798}"
SORX_URL="${SORX_URL:-http://127.0.0.1:8787}"
OPEN_BROWSER="${SORX_TEST_OPEN:-1}"
SKIN="default"
WEBCHAT_ASSETS="${WEBCHAT_ASSET_DIR:-}"

usage() {
  sed -n '2,11p' "$0" >&2
}

while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --host)
      shift
      HOST="${1:-}"
      ;;
    --host=*)
      HOST="${1#--host=}"
      ;;
    --port)
      shift
      PORT="${1:-}"
      ;;
    --port=*)
      PORT="${1#--port=}"
      ;;
    --sorx-url)
      shift
      SORX_URL="${1:-}"
      ;;
    --sorx-url=*)
      SORX_URL="${1#--sorx-url=}"
      ;;
    --skin)
      shift
      SKIN="${1:-}"
      ;;
    --skin=*)
      SKIN="${1#--skin=}"
      ;;
    --webchat-assets)
      shift
      WEBCHAT_ASSETS="${1:-}"
      ;;
    --webchat-assets=*)
      WEBCHAT_ASSETS="${1#--webchat-assets=}"
      ;;
    --no-open)
      OPEN_BROWSER=0
      ;;
    -*)
      echo "unknown argument: $1" >&2
      usage
      exit 2
      ;;
    *)
      echo "unexpected argument: $1" >&2
      usage
      exit 2
      ;;
  esac
  shift
done

if [ -z "${HOST}" ] || [ -z "${PORT}" ] || [ -z "${SORX_URL}" ] || [ -z "${SKIN}" ]; then
  echo "--host, --port, --sorx-url, and --skin require non-empty values" >&2
  exit 2
fi

if [ -z "${WEBCHAT_ASSETS}" ]; then
  for candidate in \
    "../greentic-messaging-providers/packs/messaging-webchat-gui/assets/webchat-gui" \
    "../greentic-messaging-providers/dist/packs/messaging-webchat-gui/assets/webchat-gui" \
    "../greentic-webchat/dist/webchat-gui" \
    "../greentic-webchat/assets/webchat-gui"
  do
    if [ -f "${candidate}/embed.js" ]; then
      WEBCHAT_ASSETS="$(cd "$(dirname "${candidate}")" && pwd)/$(basename "${candidate}")"
      break
    fi
  done
fi

WORK_DIR="${TMPDIR:-/tmp}/greentic-sorx-business-test-${PORT}"
WWW_DIR="${WORK_DIR}/www"
rm -rf "${WORK_DIR}"
mkdir -p "${WWW_DIR}/v1/web/webchat" "${WWW_DIR}/skins"

HAS_WEBCHAT_ASSETS=0
if [ -n "${WEBCHAT_ASSETS}" ] && [ -f "${WEBCHAT_ASSETS}/embed.js" ]; then
  HAS_WEBCHAT_ASSETS=1
  ln -s "${WEBCHAT_ASSETS}" "${WWW_DIR}/v1/web/webchat/${SKIN}"
  if [ -d "${WEBCHAT_ASSETS}/skins" ]; then
    rm -rf "${WWW_DIR}/skins"
    ln -s "${WEBCHAT_ASSETS}/skins" "${WWW_DIR}/skins"
  fi
  for asset_subdir in config i18n; do
    if [ -d "${WEBCHAT_ASSETS}/${asset_subdir}" ]; then
      rm -rf "${WWW_DIR}/${asset_subdir}"
      ln -s "${WEBCHAT_ASSETS}/${asset_subdir}" "${WWW_DIR}/${asset_subdir}"
    fi
  done
fi

python3 - "${WWW_DIR}" "${HOST}" "${PORT}" "${SORX_URL}" "${OPEN_BROWSER}" "${SKIN}" "${HAS_WEBCHAT_ASSETS}" "${ROOT_DIR}" <<'PY'
from __future__ import annotations

import base64
import hashlib
import json
import mimetypes
import os
import posixpath
import ssl
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import webbrowser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


www_dir = Path(sys.argv[1]).resolve()
host = sys.argv[2]
port = int(sys.argv[3])
sorx_url = sys.argv[4].rstrip("/")
open_browser = sys.argv[5] != "0"
skin = sys.argv[6]
has_webchat_assets = sys.argv[7] == "1"
root_dir = Path(sys.argv[8]).resolve()
STATE_LOCK = threading.Lock()
CONVERSATIONS: dict[str, list[dict]] = {}
STREAMS: dict[str, list] = {}
WEBCHAT_STATE: dict[str, object] = {
    "baseUrl": sorx_url,
    "secret": "",
    "operation": "invoke_locked_action",
    "payload": {
        "action_ref": {
            "id": "select_action",
            "version": "0.1.0",
            "contract_hash": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        },
        "values": {},
        "options": {},
    },
    "action": {
        "id": "select_action",
        "label": "Select a Sorx action",
        "description": "Use the test controls to discover and send a Sorx business action.",
        "input_schema": {"type": "object", "properties": {}},
    },
}
ACTIVITY_COUNTER = 0


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def dump_json(value: object) -> bytes:
    return json.dumps(value, indent=2, sort_keys=True).encode("utf-8")


def json_text(value: object) -> str:
    return json.dumps(value, indent=2, sort_keys=True)


def next_activity_id(prefix: str = "sorx-business") -> str:
    global ACTIVITY_COUNTER
    ACTIVITY_COUNTER += 1
    return f"{prefix}-{ACTIVITY_COUNTER}"


def now_timestamp() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime())


def normalized_error(status: int, body: object) -> dict:
    error_value = body.get("error", body) if isinstance(body, dict) else body
    if isinstance(error_value, dict):
        code = error_value.get("code") or "sorx_error"
        message = error_value.get("message") or error_value.get("text") or "Sorx request failed"
    else:
        code = "sorx_error"
        message = str(error_value or "Sorx request failed")
    return {"ok": False, "error": {"code": code, "message": message}, "sorx": {"status": status, "details": body}}


def parse_body(raw: bytes) -> object:
    if not raw:
        return {}
    try:
        return json.loads(raw.decode("utf-8"))
    except Exception:
        return raw.decode("utf-8", errors="replace")


def normalize_response(status: int, body: object, action_ref: object | None) -> dict:
    if status >= 400:
        return normalized_error(status, body)
    output: dict[str, object] = {"ok": True}
    if action_ref is not None:
        output["action_ref"] = action_ref
    if isinstance(body, dict):
        output["result"] = body.get("result", body)
        sorx = {"status": status}
        for key in ("audit_event_id", "policy_decision", "approval_required"):
            if key in body:
                sorx[key] = body[key]
        output["sorx"] = sorx
        if "explain" in body:
            output["explain"] = body["explain"]
    else:
        output["result"] = body
        output["sorx"] = {"status": status}
    return output


def action_id(payload: dict) -> str:
    value = payload.get("action_id")
    if not value and isinstance(payload.get("action_ref"), dict):
        value = payload["action_ref"].get("id")
    if not isinstance(value, str) or not value.strip():
        raise ValueError("action_id or action_ref.id is required")
    return value.strip()


def action_version(payload: dict) -> str:
    value = payload.get("action_version") or payload.get("version")
    if not value and isinstance(payload.get("action_ref"), dict):
        value = payload["action_ref"].get("version")
    if not isinstance(value, str) or not value.strip():
        raise ValueError("action_version or action_ref.version is required")
    return value.strip()


def action_body(payload: dict) -> dict:
    action_ref = payload.get("action_ref")
    if not isinstance(action_ref, dict):
        raise ValueError("action_ref is required for this operation")
    return {
        "action_ref": action_ref,
        "values": payload.get("values") if isinstance(payload.get("values"), dict) else {},
        "options": payload.get("options") if isinstance(payload.get("options"), dict) else {},
    }


def adaptive_input_for_property(name: str, schema: dict, value: object) -> dict:
    property_type = schema.get("type")
    if isinstance(property_type, list):
        property_type = property_type[0] if property_type else "string"
    title = name.replace("_", " ")
    if isinstance(schema.get("enum"), list) and schema["enum"]:
        return {
            "type": "Input.ChoiceSet",
            "id": name,
            "label": title,
            "value": str(value if value is not None else schema["enum"][0]),
            "choices": [{"title": str(item), "value": str(item)} for item in schema["enum"]],
        }
    if property_type in {"integer", "number"}:
        item = {"type": "Input.Number", "id": name, "label": title}
        if isinstance(value, (int, float)):
            item["value"] = value
        return item
    if property_type == "boolean":
        return {
            "type": "Input.Toggle",
            "id": name,
            "title": title,
            "value": "true" if value is True else "false",
            "valueOn": "true",
            "valueOff": "false",
        }
    return {
        "type": "Input.Text",
        "id": name,
        "label": title,
        "value": "" if value is None else str(value),
        "isMultiline": property_type in {"object", "array"},
    }


def coerce_card_value(raw: object, schema: dict) -> object:
    property_type = schema.get("type")
    if isinstance(property_type, list):
        property_type = property_type[0] if property_type else "string"
    if raw is None:
        return None
    if property_type == "boolean":
        return raw is True or str(raw).lower() == "true"
    if property_type == "integer":
        try:
            return int(raw)
        except Exception:
            return raw
    if property_type == "number":
        try:
            return float(raw)
        except Exception:
            return raw
    if property_type in {"object", "array"} and isinstance(raw, str):
        try:
            return json.loads(raw)
        except Exception:
            return raw
    return raw


def merge_card_values(payload: dict, submitted: dict, schema: dict) -> dict:
    merged = dict(payload.get("values") if isinstance(payload.get("values"), dict) else {})
    properties = schema.get("properties") if isinstance(schema, dict) else {}
    if not isinstance(properties, dict):
        properties = {}
    for name, property_schema in properties.items():
        if name in submitted:
            merged[name] = coerce_card_value(submitted.get(name), property_schema if isinstance(property_schema, dict) else {})
    return merged


def business_card_activity(state: dict) -> dict:
    action = state.get("action") if isinstance(state.get("action"), dict) else {}
    payload = state.get("payload") if isinstance(state.get("payload"), dict) else {}
    action_ref = payload.get("action_ref") if isinstance(payload.get("action_ref"), dict) else {}
    values = payload.get("values") if isinstance(payload.get("values"), dict) else {}
    input_schema = action.get("input_schema") if isinstance(action.get("input_schema"), dict) else {}
    properties = input_schema.get("properties") if isinstance(input_schema.get("properties"), dict) else {}
    body = [
        {
            "type": "TextBlock",
            "text": str(action.get("label") or action_ref.get("id") or "Sorx business action"),
            "weight": "Bolder",
            "size": "Medium",
            "wrap": True,
        },
        {
            "type": "TextBlock",
            "text": str(action.get("description") or "Generated from the selected sorx-business action envelope."),
            "isSubtle": True,
            "wrap": True,
        },
        {
            "type": "FactSet",
            "facts": [
                {"title": "Component", "value": "component-sorx-business"},
                {"title": "Operation", "value": str(state.get("operation") or "invoke_locked_action")},
                {"title": "Action", "value": str(action_ref.get("id") or action.get("id") or "")},
                {"title": "Sorx URL", "value": str(state.get("baseUrl") or "")},
            ],
        },
    ]
    for name, property_schema in properties.items():
        body.append(adaptive_input_for_property(name, property_schema if isinstance(property_schema, dict) else {}, values.get(name)))
    if not properties:
        body.append({
            "type": "TextBlock",
            "text": "No input parameters are required for this business operation.",
            "wrap": True,
        })
    body.append({
        "type": "TextBlock",
        "text": "Envelope preview",
        "weight": "Bolder",
        "spacing": "Medium",
        "wrap": True,
    })
    body.append({
        "type": "TextBlock",
        "text": json_text(payload),
        "fontType": "Monospace",
        "size": "Small",
        "wrap": True,
    })
    return {
        "type": "message",
        "id": next_activity_id("sorx-card"),
        "timestamp": now_timestamp(),
        "from": {"id": "sorx-business-test-bot", "name": "Sorx Business Test"},
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "content": {
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "type": "AdaptiveCard",
                "version": "1.5",
                "body": body,
                "actions": [{
                    "type": "Action.Submit",
                    "title": {
                        "dry_run_locked_action": "Dry Run",
                        "explain_business_action_mapping": "Explain",
                        "get_business_action_schema": "Show Schema",
                    }.get(str(state.get("operation") or ""), "Invoke"),
                    "data": {
                        "action": "sorx_business_component",
                        "operation": str(state.get("operation") or "invoke_locked_action"),
                    },
                }],
            },
        }],
    }


def result_activity(result: dict, operation: str) -> dict:
    ok = bool(result.get("ok")) if isinstance(result, dict) else False
    return {
        "type": "message",
        "id": next_activity_id("sorx-result"),
        "timestamp": now_timestamp(),
        "from": {"id": "sorx-business-test-bot", "name": "Sorx Business Test"},
        "text": f"{operation} {'succeeded' if ok else 'failed'}\n\n```json\n{json_text(result)}\n```",
    }


def sync_webchat_state(request: dict) -> dict:
    state = {
        "baseUrl": str(request.get("baseUrl") or sorx_url),
        "secret": str(request.get("secret") or ""),
        "operation": str(request.get("operation") or "invoke_locked_action"),
        "payload": request.get("payload") if isinstance(request.get("payload"), dict) else {},
        "action": request.get("action") if isinstance(request.get("action"), dict) else {},
    }
    with STATE_LOCK:
        WEBCHAT_STATE.clear()
        WEBCHAT_STATE.update(state)
        card = business_card_activity(WEBCHAT_STATE)
        if not CONVERSATIONS:
            CONVERSATIONS["sorx-business-test"] = [card]
        else:
            for activities in CONVERSATIONS.values():
                activities.append(card)
        snapshots = [(conversation_id, list(activities)) for conversation_id, activities in CONVERSATIONS.items()]
    for conversation_id, activities in snapshots:
        broadcast_activities(conversation_id, activities, str(len(activities)))
    return {"ok": True, "result": {"activity": card, "state": state}}


def activities_for_conversation(conversation_id: str) -> list[dict]:
    with STATE_LOCK:
        if conversation_id not in CONVERSATIONS:
            CONVERSATIONS[conversation_id] = [business_card_activity(WEBCHAT_STATE)]
        return list(CONVERSATIONS[conversation_id])


def append_activity(conversation_id: str, activity: dict) -> None:
    with STATE_LOCK:
        CONVERSATIONS.setdefault(conversation_id, [business_card_activity(WEBCHAT_STATE)]).append(activity)
        activities = list(CONVERSATIONS[conversation_id])
    broadcast_activities(conversation_id, activities, str(len(activities)))


def conversation_id_from_path(path: str) -> str | None:
    marker = "/v3/directline/conversations/"
    if marker not in path:
        return None
    tail = path.split(marker, 1)[1].strip("/")
    if not tail:
        return None
    return tail.split("/", 1)[0]


def handle_webchat_submit(conversation_id: str, body_json: dict) -> dict:
    value = body_json.get("value") if isinstance(body_json.get("value"), dict) else {}
    if value.get("action") != "sorx_business_component":
        append_activity(conversation_id, {
            "type": "message",
            "id": next_activity_id("echo"),
            "timestamp": now_timestamp(),
            "from": {"id": "sorx-business-test-bot", "name": "Sorx Business Test"},
            "text": "Received chat activity.",
        })
        return {"id": next_activity_id("activity")}
    with STATE_LOCK:
        state = dict(WEBCHAT_STATE)
    payload = state.get("payload") if isinstance(state.get("payload"), dict) else {}
    action = state.get("action") if isinstance(state.get("action"), dict) else {}
    schema = action.get("input_schema") if isinstance(action.get("input_schema"), dict) else {}
    merged_payload = dict(payload)
    merged_payload["values"] = merge_card_values(payload, value, schema)
    operation = str(value.get("operation") or state.get("operation") or "invoke_locked_action")
    request = {
        "baseUrl": state.get("baseUrl"),
        "secret": state.get("secret"),
        "operation": operation,
        "payload": merged_payload,
        "timeoutMs": 30000,
        "strictTls": True,
    }
    result = proxy_to_sorx(request)
    append_activity(conversation_id, result_activity(result, operation))
    return {"id": next_activity_id("activity")}


def websocket_frame(payload: dict) -> bytes:
    data = json.dumps(payload).encode("utf-8")
    length = len(data)
    if length < 126:
        header = bytes([0x81, length])
    elif length < 65536:
        header = bytes([0x81, 126, (length >> 8) & 0xFF, length & 0xFF])
    else:
        header = bytes([
            0x81,
            127,
            (length >> 56) & 0xFF,
            (length >> 48) & 0xFF,
            (length >> 40) & 0xFF,
            (length >> 32) & 0xFF,
            (length >> 24) & 0xFF,
            (length >> 16) & 0xFF,
            (length >> 8) & 0xFF,
            length & 0xFF,
        ])
    return header + data


def websocket_ping_frame() -> bytes:
    return b"\x89\x00"


def broadcast_activities(conversation_id: str, activities: list[dict], watermark: str) -> None:
    frame = websocket_frame({"activities": activities, "watermark": watermark})
    streams = STREAMS.get(conversation_id, [])
    for stream in list(streams):
        try:
            stream.write(frame)
            stream.flush()
        except (BrokenPipeError, ConnectionResetError, OSError):
            try:
                streams.remove(stream)
            except ValueError:
                pass


def build_sorx_request(request: dict) -> tuple[str, str, bytes | None, object | None]:
    base_url = str(request.get("baseUrl") or "").strip().rstrip("/")
    if not base_url.startswith(("http://", "https://")):
        raise ValueError("Sorx URL must start with http:// or https://")
    operation = str(request.get("operation") or "invoke_locked_action")
    payload = request.get("payload")
    if not isinstance(payload, dict):
        payload = {}

    if operation == "list_business_actions":
        return "GET", f"{base_url}/v1/sorx/tools", None, None
    if operation == "get_business_action_schema":
        version = action_version(payload)
        return "GET", f"{base_url}/v1/sorx/business-actions/{urllib.parse.quote(action_id(payload), safe='')}/versions/{urllib.parse.quote(version, safe='')}/schema", None, None
    if operation == "dry_run_locked_action":
        body = action_body(payload)
        version = action_version(payload)
        return "POST", f"{base_url}/v1/sorx/business-actions/{urllib.parse.quote(action_id(payload), safe='')}/versions/{urllib.parse.quote(version, safe='')}/dry-run", dump_json(body), payload.get("action_ref")
    if operation == "invoke_locked_action":
        body = action_body(payload)
        version = action_version(payload)
        return "POST", f"{base_url}/v1/sorx/business-actions/{urllib.parse.quote(action_id(payload), safe='')}/versions/{urllib.parse.quote(version, safe='')}/invoke", dump_json(body), payload.get("action_ref")
    if operation == "explain_business_action_mapping":
        body = action_body(payload)
        version = action_version(payload)
        return "POST", f"{base_url}/v1/sorx/business-actions/{urllib.parse.quote(action_id(payload), safe='')}/versions/{urllib.parse.quote(version, safe='')}/dry-run", dump_json(body), payload.get("action_ref")
    raise ValueError(f"Unsupported operation: {operation}")


def send_sorx(method: str, url: str, body: bytes | None, headers: dict[str, str], timeout_ms: int, strict_tls: bool) -> tuple[int, object]:
    context = None
    if url.startswith("https://") and not strict_tls:
        context = ssl._create_unverified_context()
    http_request = urllib.request.Request(url, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(http_request, timeout=max(timeout_ms / 1000, 1), context=context) as response:
            return response.status, parse_body(response.read())
    except urllib.error.HTTPError as error:
        return error.code, parse_body(error.read())


def component_input(request: dict) -> dict:
    base_url = str(request.get("baseUrl") or "").strip().rstrip("/")
    if not base_url.startswith(("http://", "https://")):
        raise ValueError("Sorx URL must start with http:// or https://")
    payload = request.get("payload")
    if not isinstance(payload, dict):
        payload = {}
    secret = str(request.get("secret") or "").strip()
    body = dict(payload)
    body["config"] = {
        "sorx_base_url": base_url,
        "auth": {"kind": "bearer_secret_ref", "secret_ref": "SORX_TEST_TOKEN"} if secret else {"kind": "none"},
        "timeout_ms": int(request.get("timeoutMs") or 30000),
        "strict_tls": bool(request.get("strictTls", True)),
    }
    return body


def component_command() -> list[str]:
    configured = os.environ.get("SORX_BUSINESS_COMPONENT_CLI")
    if configured:
        return [configured]
    binary = root_dir / "target" / "debug" / "component-sorx-business-cli"
    if binary.exists():
        return [str(binary)]
    return [
        "cargo",
        "run",
        "--quiet",
        "-p",
        "component-sorx-business",
        "--bin",
        "component-sorx-business-cli",
        "--",
    ]


def invoke_component_operation(request: dict) -> dict:
    operation = str(request.get("operation") or "invoke_locked_action")
    env = os.environ.copy()
    secret = str(request.get("secret") or "").strip()
    if secret:
        env["SORX_TEST_TOKEN"] = secret
    command = component_command() + [operation]
    try:
        completed = subprocess.run(
            command,
            input=json.dumps(component_input(request)),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=str(root_dir),
            env=env,
            timeout=max(int(request.get("timeoutMs") or 30000) / 1000 + 10, 15),
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {"ok": False, "error": {"code": "component_timeout", "message": "component-sorx-business timed out"}}
    except Exception as error:
        return {"ok": False, "error": {"code": "component_invoke_failed", "message": str(error)}}
    if completed.returncode != 0:
        message = completed.stderr.strip() or completed.stdout.strip() or f"component exited with {completed.returncode}"
        return {"ok": False, "error": {"code": "component_invoke_failed", "message": message}}
    try:
        parsed = json.loads(completed.stdout)
    except Exception as error:
        return {
            "ok": False,
            "error": {"code": "component_output_invalid", "message": f"component output was not JSON: {error}"},
            "stdout": completed.stdout,
            "stderr": completed.stderr,
        }
    return parsed if isinstance(parsed, dict) else {"ok": True, "result": parsed}


def discover_business_actions(request: dict) -> dict:
    base_url = str(request.get("baseUrl") or "").strip().rstrip("/")
    if not base_url.startswith(("http://", "https://")):
        raise ValueError("Sorx URL must start with http:// or https://")
    timeout_ms = int(request.get("timeoutMs") or 30000)
    strict_tls = bool(request.get("strictTls", True))
    headers = {"Accept": "application/json"}
    secret = str(request.get("secret") or "").strip()
    if secret:
        headers["Authorization"] = f"Bearer {secret}"

    component_listing = invoke_component_operation({**request, "operation": "list_business_actions", "payload": {}})
    if not component_listing.get("ok"):
        return component_listing
    status = int(component_listing.get("sorx", {}).get("status") or 200)
    routes_status, routes_body = send_sorx("GET", f"{base_url}/v1/sorx/routes", None, headers, timeout_ms, strict_tls)
    metrics_status, metrics_body = send_sorx("GET", f"{base_url}/v1/sorx/metrics", None, headers, timeout_ms, strict_tls)
    tools = component_listing.get("result", {}).get("actions", []) if isinstance(component_listing.get("result"), dict) else []
    routes = routes_body.get("routes", []) if routes_status < 400 and isinstance(routes_body, dict) else []
    metrics = metrics_body.get("metrics", []) if metrics_status < 400 and isinstance(metrics_body, dict) else []
    route_by_action = {
        value: route
        for route in routes if isinstance(route, dict)
        for value in (route.get("endpoint_id"), route.get("operation_id"))
        if isinstance(value, str)
    }
    metric_by_name = {
        metric.get("name"): metric
        for metric in metrics if isinstance(metric, dict) and isinstance(metric.get("name"), str)
    }
    actions = []
    for tool in tools if isinstance(tools, list) else []:
        if not isinstance(tool, dict):
            continue
        action_id_value = tool.get("id") or tool.get("endpoint_id") or tool.get("name") or tool.get("operation_id")
        if not isinstance(action_id_value, str):
            continue
        operation_id_value = tool.get("operation_id") if isinstance(tool.get("operation_id"), str) else action_id_value
        is_metric_query = operation_id_value.startswith("metrics.query.")
        route = route_by_action.get(action_id_value) or route_by_action.get(operation_id_value)
        if not route and not is_metric_query:
            continue
        metric_name = operation_id_value.removeprefix("metrics.query.") if is_metric_query else ""
        metric = metric_by_name.get(metric_name) if metric_name else None
        display_name = (
            (metric.get("label") if isinstance(metric, dict) else None)
            or tool.get("title")
            or tool.get("label")
            or (route.get("title") if isinstance(route, dict) else None)
            or tool.get("name")
            or action_id_value
        )
        actions.append({
            "id": action_id_value,
            "name": tool.get("name") or action_id_value,
            "display_name": display_name,
            "metric_name": metric_name,
            "kind": "metric_query" if is_metric_query else "agent_route",
            "version": "0.1.0",
            "versions": ["0.1.0"],
            "label": display_name,
            "description": tool.get("description"),
            "risk": tool.get("risk"),
            "input_schema": tool.get("input_schema") or {"type": "object"},
            "contract_hash": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "source": "sorx_tool",
        })

    return {
        "ok": True,
        "result": {
            "schema": "greentic.sorx.business-action-discovery.v1",
            "actions": actions,
            "runtime_tools": tools if isinstance(tools, list) else [],
            "runtime_routes": routes if isinstance(routes, list) else [],
            "summary_count": len(actions),
            "detail_count": len(actions),
            "warnings": [],
            "note": "Discovered Sorx agent endpoint actions from /v1/sorx/tools.",
        },
        "sorx": {"status": status},
    }


def proxy_to_sorx(request: dict) -> dict:
    if str(request.get("operation") or "") == "discover_business_actions":
        return discover_business_actions(request)
    if str(request.get("operation") or "") in {
        "list_business_actions",
        "get_business_action_schema",
        "dry_run_locked_action",
        "explain_business_action_mapping",
        "invoke_locked_action",
    }:
        return invoke_component_operation(request)
    method, url, body, action_ref = build_sorx_request(request)
    headers = {"Accept": "application/json"}
    if body is not None:
        headers["Content-Type"] = "application/json"
    secret = str(request.get("secret") or "").strip()
    if secret:
        headers["Authorization"] = f"Bearer {secret}"
    timeout_ms = int(request.get("timeoutMs") or 30000)
    strict_tls = bool(request.get("strictTls", True))
    try:
        status, parsed = send_sorx(method, url, body, headers, timeout_ms, strict_tls)
    except Exception as error:
        return {"ok": False, "error": {"code": "http_error", "message": str(error)}, "sorx": {"status": None}}
    return normalize_response(status, parsed, action_ref)


def proxy_agent_endpoint_action(request: dict) -> dict:
    base_url = str(request.get("baseUrl") or "").strip().rstrip("/")
    if not base_url.startswith(("http://", "https://")):
        raise ValueError("Sorx URL must start with http:// or https://")
    payload = request.get("payload")
    if not isinstance(payload, dict):
        payload = {}
    operation = str(request.get("operation") or "")
    timeout_ms = int(request.get("timeoutMs") or 30000)
    strict_tls = bool(request.get("strictTls", True))
    headers = {"Accept": "application/json"}
    secret = str(request.get("secret") or "").strip()
    if secret:
        headers["Authorization"] = f"Bearer {secret}"

    action_ref_value = payload.get("action_ref") if isinstance(payload.get("action_ref"), dict) else {}
    action_id_value = action_id(payload)
    version = action_version(payload)
    tools_status, tools_body = send_sorx("GET", f"{base_url}/v1/sorx/tools", None, headers, timeout_ms, strict_tls)
    if tools_status >= 400:
        return normalized_error(tools_status, tools_body)
    tools = tools_body.get("tools", []) if isinstance(tools_body, dict) else []
    tool = next(
        (
            item for item in tools
            if isinstance(item, dict)
            and (item.get("endpoint_id") == action_id_value or item.get("name") == action_id_value or item.get("operation_id") == action_id_value)
        ),
        None,
    )
    if not tool:
        return {"ok": False, "error": {"code": "unknown_action", "message": f"Sorx tool not found: {action_id_value}"}}

    if operation == "get_business_action_schema":
        return {
            "ok": True,
            "result": {
                "schema": "greentic.sorx.agent-endpoint-action-schema.v1",
                "id": action_id_value,
                "version": version,
                "input_schema": tool.get("input_schema") or {"type": "object"},
                "output_schema": tool.get("output_schema") or {"type": "object"},
            },
            "sorx": {"status": tools_status, "source": "/v1/sorx/tools"},
        }

    if operation in {"dry_run_locked_action", "explain_business_action_mapping"}:
        return {
            "ok": True,
            "action_ref": action_ref_value,
            "result": {
                "valid": True,
                "canonical_payload": payload.get("values") if isinstance(payload.get("values"), dict) else {},
                "execution_target": {
                    "endpoint_id": tool.get("endpoint_id") or action_id_value,
                    "operation_id": tool.get("operation_id") or action_id_value,
                    "tool_name": tool.get("name") or action_id_value,
                },
            },
            "explain": {"source": "/v1/sorx/tools", "tool": tool},
            "sorx": {"status": tools_status, "source": "/v1/sorx/tools"},
        }

    if action_id_value.startswith("metrics.query.") or str(tool.get("operation_id") or "").startswith("metrics.query."):
        metric_name = str(tool.get("operation_id") or action_id_value).removeprefix("metrics.query.")
        values = payload.get("values") if isinstance(payload.get("values"), dict) else {}
        invoke_headers = dict(headers)
        invoke_headers["Content-Type"] = "application/json"
        status, parsed = send_sorx(
            "POST",
            f"{base_url}/v1/sorx/metrics/{urllib.parse.quote(metric_name, safe='')}/query",
            dump_json(values),
            invoke_headers,
            timeout_ms,
            strict_tls,
        )
        return normalize_response(status, parsed, action_ref_value)

    routes_status, routes_body = send_sorx("GET", f"{base_url}/v1/sorx/routes", None, headers, timeout_ms, strict_tls)
    if routes_status >= 400:
        return normalized_error(routes_status, routes_body)
    routes = routes_body.get("routes", []) if isinstance(routes_body, dict) else []
    route = next(
        (
            item for item in routes
            if isinstance(item, dict)
            and (item.get("endpoint_id") == action_id_value or item.get("operation_id") == action_id_value)
        ),
        None,
    )
    if not route:
        return {"ok": False, "error": {"code": "unknown_action", "message": f"Sorx route not found for action: {action_id_value}"}}
    route_path = route.get("path")
    method = str(route.get("method") or "POST")
    if not isinstance(route_path, str):
        return {"ok": False, "error": {"code": "invalid_route", "message": "Sorx route is missing path"}}
    if "{" in route_path:
        return {"ok": False, "error": {"code": "route_parameters_required", "message": f"Sorx route path contains parameters: {route_path}"}}
    values = payload.get("values") if isinstance(payload.get("values"), dict) else {}
    body = None if method == "GET" else dump_json(values)
    invoke_headers = dict(headers)
    if body is not None:
        invoke_headers["Content-Type"] = "application/json"
    status, parsed = send_sorx(method, f"{base_url}{route_path}", body, invoke_headers, timeout_ms, strict_tls)
    return normalize_response(status, parsed, action_ref_value)


fallback_webchat = """
class GreenticWebchatFallback extends HTMLElement {
  connectedCallback() {
    this.innerHTML = `<div class="fallback-chat">
      <div class="fallback-chat__head">WebChat GUI assets not found</div>
      <div class="fallback-chat__body">
        <p>Set WEBCHAT_ASSET_DIR or pass --webchat-assets to load the real embedded GUI.</p>
        <div class="bubble">Sorx Business Component tester is ready on the right.</div>
      </div>
      <div class="fallback-chat__input"><input value="Test Sorx business operation" /><button type="button">Send</button></div>
    </div>`;
  }
}
customElements.define('greentic-webchat', GreenticWebchatFallback);
"""

if not has_webchat_assets:
    write_text(www_dir / "v1/web/webchat" / skin / "embed.js", fallback_webchat)

write_text(
    www_dir / "test.html",
    f"""<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Sorx Business Component Test</title>
    <script type="module" src="/v1/web/webchat/{skin}/embed.js"></script>
    <style>
      :root {{
        color-scheme: light;
        font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      }}
      * {{ box-sizing: border-box; }}
      body {{
        margin: 0;
        min-height: 100vh;
        background: #f4f7fb;
        color: #172033;
      }}
      main {{
        width: min(1360px, calc(100vw - 28px));
        min-height: 100vh;
        margin: 0 auto;
        padding: 16px 0 24px;
        display: grid;
        grid-template-rows: auto minmax(0, 1fr);
        gap: 14px;
      }}
      header {{
        display: flex;
        align-items: end;
        justify-content: space-between;
        gap: 16px;
      }}
      h1 {{
        margin: 0;
        font-size: 24px;
        line-height: 1.2;
      }}
      .hint {{
        margin: 4px 0 0;
        color: #5d6b7c;
        font-size: 14px;
      }}
      .status {{
        display: inline-flex;
        align-items: center;
        min-height: 32px;
        padding: 0 10px;
        border: 1px solid #cbd5e1;
        border-radius: 8px;
        background: #fff;
        color: #475569;
        font-size: 13px;
        white-space: nowrap;
      }}
      .layout {{
        min-height: 0;
        display: grid;
        grid-template-columns: minmax(520px, 1fr) 340px;
        gap: 14px;
      }}
      .webchat-shell,
      .tester {{
        min-width: 0;
        min-height: 0;
        border: 1px solid #d4dce8;
        border-radius: 8px;
        background: #fff;
        box-shadow: 0 12px 28px rgba(15, 23, 42, 0.08);
        overflow: hidden;
      }}
      .webchat-shell {{
        display: grid;
        grid-template-rows: auto minmax(0, 1fr);
      }}
      .panel-head {{
        padding: 12px 14px;
        border-bottom: 1px solid #e2e8f0;
        background: #fbfdff;
        font-size: 14px;
        font-weight: 700;
      }}
      .webchat-frame {{
        min-height: 0;
        height: calc(100vh - 136px);
        background: #f8fafc;
      }}
      greentic-webchat {{
        display: block;
        width: 100%;
        height: 100%;
        min-height: 0;
      }}
      .tester {{
        padding: 12px;
        display: grid;
        grid-template-rows: auto;
        align-content: start;
        gap: 10px;
      }}
      .form-grid {{
        display: grid;
        grid-template-columns: 1fr;
        gap: 10px;
      }}
      label {{
        display: grid;
        gap: 5px;
        color: #405064;
        font-size: 12px;
        font-weight: 700;
      }}
      input,
      select,
      textarea {{
        width: 100%;
        border: 1px solid #cbd5e1;
        border-radius: 7px;
        background: #fff;
        color: #111827;
        font: 13px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
      }}
      input,
      select {{
        min-height: 36px;
        padding: 0 10px;
      }}
      textarea {{
        min-height: 120px;
        padding: 10px;
        resize: vertical;
        line-height: 1.45;
      }}
      .wide {{ grid-column: 1 / -1; }}
      .actions {{
        display: grid;
        gap: 8px;
      }}
      .discovery {{
        display: grid;
        grid-template-columns: 1fr;
        gap: 8px;
        align-items: end;
      }}
      .notice {{
        min-height: 34px;
        border: 1px solid #d8e0ec;
        border-radius: 7px;
        padding: 8px 10px;
        background: #f8fafc;
        color: #405064;
        font-size: 12px;
        line-height: 1.35;
      }}
      button {{
        min-height: 36px;
        border: 1px solid #bac6d6;
        border-radius: 7px;
        padding: 0 12px;
        background: #fff;
        color: #172033;
        font: inherit;
        font-size: 13px;
        font-weight: 700;
        cursor: pointer;
      }}
      button.primary {{
        border-color: #116466;
        background: #116466;
        color: #fff;
      }}
      .actions button {{
        width: 100%;
      }}
      .advanced {{
        border-top: 1px solid #e2e8f0;
        padding-top: 10px;
      }}
      .advanced summary {{
        cursor: pointer;
        color: #475569;
        font-size: 13px;
        font-weight: 700;
      }}
      .advanced-body {{
        display: grid;
        gap: 10px;
        margin-top: 10px;
      }}
      .mini-output {{
        max-height: 170px;
        min-height: 92px;
      }}
      pre {{
        margin: 0;
        min-height: 150px;
        overflow: auto;
        border: 1px solid #d8e0ec;
        border-radius: 7px;
        padding: 10px;
        background: #0f172a;
        color: #dbeafe;
        font-size: 12px;
        line-height: 1.45;
      }}
      .fallback-chat {{
        height: 100%;
        display: grid;
        grid-template-rows: auto minmax(0, 1fr) auto;
        background: #eef4f8;
      }}
      .fallback-chat__head {{
        padding: 14px;
        background: #116466;
        color: #fff;
        font-weight: 700;
      }}
      .fallback-chat__body {{
        padding: 14px;
        overflow: auto;
      }}
      .bubble {{
        max-width: 280px;
        margin-top: 14px;
        padding: 10px 12px;
        border-radius: 8px;
        background: #fff;
        border: 1px solid #d4dce8;
      }}
      .fallback-chat__input {{
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto;
        gap: 8px;
        padding: 12px;
        border-top: 1px solid #d4dce8;
        background: #fff;
      }}
      @media (max-width: 900px) {{
        main {{ width: min(100vw - 20px, 720px); }}
        header {{ align-items: start; flex-direction: column; }}
        .layout {{ grid-template-columns: 1fr; }}
        .webchat-frame {{ height: 460px; }}
        .tester {{ grid-template-rows: auto; }}
        .form-grid {{ grid-template-columns: 1fr; }}
        .discovery {{ grid-template-columns: 1fr; }}
      }}
    </style>
  </head>
  <body>
    <main>
      <header>
        <div>
          <h1>Sorx Business Component Test</h1>
          <p class="hint">Pick a Sorx business operation, send it into WebChat, then use the card to dry-run or invoke the component.</p>
        </div>
        <div class="status" id="asset-status">WebChat assets: {"loaded" if has_webchat_assets else "fallback"}</div>
      </header>
      <section class="layout">
        <div class="webchat-shell">
          <div class="panel-head">Embedded WebChat GUI</div>
          <div class="webchat-frame">
            <greentic-webchat tenant="{skin}" public-base-url="http://{host}:{port}" mode="inline" render="iframe" title="Sorx Business Test"></greentic-webchat>
          </div>
        </div>
        <form class="tester" id="tester">
          <div class="panel-head">Business operation</div>
          <div class="discovery">
            <label>Discovered action
              <select id="actionPicker">
                <option value="">No actions discovered</option>
              </select>
            </label>
          </div>
          <div class="form-grid">
            <label>Component operation
              <select id="operation" name="operation">
                <option value="invoke_locked_action">Invoke</option>
                <option value="dry_run_locked_action">Dry run</option>
                <option value="explain_business_action_mapping">Explain mapping</option>
                <option value="get_business_action_schema">Show schema</option>
              </select>
            </label>
          </div>
          <div class="notice" id="discoveryNotice">Discovery has not run yet.</div>
          <div class="actions">
            <button class="primary" id="send-webchat" type="button">Show In WebChat</button>
            <button id="discover-actions" type="button">Refresh Actions</button>
          </div>
          <textarea id="payload" spellcheck="false" hidden></textarea>
          <details class="advanced">
            <summary>Advanced</summary>
            <div class="advanced-body">
              <label>Sorx URL
                <input id="baseUrl" name="baseUrl" placeholder="http://127.0.0.1:8787" autocomplete="url" required>
              </label>
              <label>Bearer secret
                <input id="secret" name="secret" type="password" placeholder="optional">
              </label>
              <label>Envelope / result
                <pre class="mini-output" id="result">{{}}</pre>
              </label>
            </div>
          </details>
        </form>
      </section>
    </main>
    <script>
      const operation = document.getElementById('operation');
      const payload = document.getElementById('payload');
      const result = document.getElementById('result');
      const baseUrl = document.getElementById('baseUrl');
      const secret = document.getElementById('secret');
      const actionPicker = document.getElementById('actionPicker');
      const discoveryNotice = document.getElementById('discoveryNotice');
      let discoveredActions = [];
      let runtimeTools = [];

      const samples = {{
        action: {{
          action_ref: {{
            id: "approve_invoice",
            version: "0.1.0",
            contract_hash: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
          }},
          values: {{
            invoice_id: "INV-1001",
            approved: true
          }},
          options: {{
            require_explanation: true,
            dry_run_first: false
          }}
        }}
      }};

      function pretty(value) {{
        return JSON.stringify(value, null, 2);
      }}

      function parsePayload() {{
        try {{
          return JSON.parse(payload.value || '{{}}');
        }} catch (error) {{
          throw new Error(`Input parameters JSON is invalid: ${{error.message}}`);
        }}
      }}

      function envelope() {{
        const body = parsePayload();
        body.config = {{
          sorx_base_url: baseUrl.value.trim(),
          auth: secret.value.trim() ? {{ kind: "bearer_secret_ref", secret_ref: "SORX_TEST_TOKEN" }} : {{ kind: "none" }},
          timeout_ms: 30000,
          strict_tls: true
        }};
        return body;
      }}

      async function postProxy(operationName, bodyPayload) {{
        const response = await fetch('/__sorx_proxy', {{
          method: 'POST',
          headers: {{ 'Content-Type': 'application/json' }},
          body: pretty({{
            baseUrl: baseUrl.value,
            secret: secret.value,
            operation: operationName,
            payload: bodyPayload,
            timeoutMs: 30000,
            strictTls: true
          }})
        }});
        return await response.json();
      }}

      function selectedAction() {{
        const [id, version] = actionPicker.value.split('@@');
        return discoveredActions.find((item) => item.id === id && item.version === version) || null;
      }}

      function currentPayload() {{
        const action = selectedAction();
        if (action) return payloadForAction(action);
        return parsePayload();
      }}

      function reloadWebChat() {{
        const current = document.querySelector('greentic-webchat');
        if (!current) return;
        const next = current.cloneNode(false);
        current.replaceWith(next);
      }}

      async function sendToWebChat() {{
        const body = currentPayload();
        payload.value = pretty(body);
        const response = await fetch('/__webchat_state', {{
          method: 'POST',
          headers: {{ 'Content-Type': 'application/json' }},
          body: pretty({{
            baseUrl: baseUrl.value,
            secret: secret.value,
            operation: operation.value,
            payload: body,
            action: selectedAction() || {{
              id: body.action_ref && body.action_ref.id,
              label: body.action_ref && body.action_ref.id,
              input_schema: {{ type: "object", properties: {{}} }}
            }}
          }})
        }});
        const data = await response.json();
        result.textContent = pretty(data);
        if (data.ok) {{
          reloadWebChat();
        }}
        return data;
      }}

      function sampleValue(schema) {{
        if (!schema || typeof schema !== 'object') return null;
        const type = Array.isArray(schema.type) ? schema.type[0] : schema.type;
        if (type === 'object' || schema.properties) {{
          const object = {{}};
          const properties = schema.properties || {{}};
          const required = Array.isArray(schema.required) ? schema.required : Object.keys(properties);
          for (const name of required) {{
            object[name] = sampleValue(properties[name] || {{}});
          }}
          return object;
        }}
        if (type === 'array') return [];
        if (type === 'integer' || type === 'number') return schema.default ?? 1;
        if (type === 'boolean') return schema.default ?? true;
        if (schema.enum && schema.enum.length) return schema.enum[0];
        return schema.default ?? "example";
      }}

      function payloadForAction(action) {{
        const values = sampleValue(action.input_schema || {{ type: "object", properties: {{}} }});
        const options = {{}};
        if (action.idempotency && action.idempotency.required) {{
          options.idempotency_key = `${{action.id}}-${{Date.now()}}`;
        }}
        return {{
          action_ref: {{
            id: action.id,
            version: action.version,
            contract_hash: action.contract_hash || "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
          }},
          values: values && typeof values === 'object' && !Array.isArray(values) ? values : {{}},
          options
        }};
      }}

      function renderActionPicker(actions) {{
        actionPicker.innerHTML = "";
        if (!actions.length) {{
          const option = document.createElement('option');
          option.value = "";
          option.textContent = "No Sorx actions discovered";
          actionPicker.append(option);
          return;
        }}
        function displayActionName(action) {{
          const raw = action.display_name || action.label || action.name || action.id || "";
          return String(raw)
            .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
            .replace(/[_-]+/g, " ")
            .replace(/\\s+/g, " ")
            .trim();
        }}
        for (const action of actions) {{
          const option = document.createElement('option');
          option.value = `${{action.id}}@@${{action.version}}`;
          option.textContent = displayActionName(action);
          option.title = `${{action.id}} @ ${{action.version}}`;
          actionPicker.append(option);
        }}
      }}

      async function discoverActions() {{
        result.textContent = "Discovering actions...";
        const data = await postProxy("discover_business_actions", {{}});
        if (!data.ok) {{
          result.textContent = pretty(data);
          renderActionPicker([]);
          return;
        }}
        discoveredActions = (data.result && Array.isArray(data.result.actions)) ? data.result.actions : [];
        runtimeTools = (data.result && Array.isArray(data.result.runtime_tools)) ? data.result.runtime_tools : [];
        renderActionPicker(discoveredActions);
        discoveryNotice.textContent = data.result && data.result.note
          ? data.result.note
          : `${{discoveredActions.length}} Sorx action(s) discovered.`;
        result.textContent = pretty(data);
        if (discoveredActions.length) {{
          payload.value = pretty(payloadForAction(discoveredActions[0]));
        }}
      }}

      function useSelectedAction() {{
        const action = selectedAction();
        if (!action) return;
        operation.value = "invoke_locked_action";
        payload.value = pretty(payloadForAction(action));
        result.textContent = pretty(action);
      }}

      document.getElementById('discover-actions').addEventListener('click', async () => {{
        try {{
          await discoverActions();
        }} catch (error) {{
          result.textContent = pretty({{ ok: false, error: {{ code: "discover_failed", message: error.message }} }});
        }}
      }});

      actionPicker.addEventListener('change', useSelectedAction);

      document.getElementById('send-webchat').addEventListener('click', async () => {{
        try {{
          await sendToWebChat();
        }} catch (error) {{
          result.textContent = pretty({{ ok: false, error: {{ code: "webchat_sync_failed", message: error.message }} }});
        }}
      }});

      payload.value = pretty(samples.action);
      const savedUrl = localStorage.getItem('sorxTestBaseUrl') || '';
      baseUrl.value = savedUrl || {json.dumps(sorx_url)};
      baseUrl.addEventListener('change', () => localStorage.setItem('sorxTestBaseUrl', baseUrl.value));
      discoverActions().catch(() => {{}});
    </script>
  </body>
</html>
""",
)


class Handler(BaseHTTPRequestHandler):
    server_version = "SorxBusinessTest/1.0"

    def log_message(self, fmt: str, *args: object) -> None:
        print(f"{self.address_string()} - {fmt % args}", flush=True)

    def send_bytes(self, status: int, body: bytes, content_type: str, include_body: bool = True) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        if include_body:
            try:
                self.wfile.write(body)
            except BrokenPipeError:
                pass

    def send_json(self, status: int, value: dict) -> None:
        self.send_bytes(status, dump_json(value), "application/json; charset=utf-8")

    def do_POST(self) -> None:
        path = urllib.parse.urlparse(self.path).path
        length = int(self.headers.get("Content-Length", "0"))
        raw_body = self.rfile.read(length) if length else b""
        body_json = {}
        if raw_body:
            try:
                parsed_body = json.loads(raw_body.decode("utf-8"))
                body_json = parsed_body if isinstance(parsed_body, dict) else {}
            except Exception:
                body_json = {}

        if path == "/__webchat_state":
            try:
                response = sync_webchat_state(body_json)
                self.send_bytes(200, dump_json(response), "application/json; charset=utf-8")
            except Exception as error:
                body = dump_json({"ok": False, "error": {"code": "webchat_state_error", "message": str(error)}})
                self.send_bytes(200, body, "application/json; charset=utf-8")
            return

        if (
            path.endswith("/token")
            or path.endswith("/v3/directline/tokens/generate")
            or path.endswith("/v3/directline/tokens/refresh")
        ):
            self.send_json(200, {
                "conversationId": "sorx-business-test",
                "token": "sorx-business-test-token",
                "expires_in": 1800,
            })
            return

        if path.endswith("/v3/directline/conversations"):
            with STATE_LOCK:
                CONVERSATIONS.setdefault("sorx-business-test", [business_card_activity(WEBCHAT_STATE)])
            host_header = self.headers.get("Host", f"{host}:{port}")
            self.send_json(200, {
                "conversationId": "sorx-business-test",
                "token": "sorx-business-test-token",
                "streamUrl": f"ws://{host_header}/v1/messaging/webchat/{skin}/v3/directline/conversations/sorx-business-test/stream",
                "expires_in": 1800,
            })
            return

        if path.endswith("/v3/directline/conversations/sorx-business-test/activities"):
            self.send_json(200, handle_webchat_submit("sorx-business-test", body_json))
            return

        if path != "/__sorx_proxy":
            self.send_bytes(404, b"not found", "text/plain; charset=utf-8")
            return
        try:
            request = body_json
            response = proxy_to_sorx(request if isinstance(request, dict) else {})
            self.send_bytes(200, dump_json(response), "application/json; charset=utf-8")
        except Exception as error:
            body = dump_json({"ok": False, "error": {"code": "proxy_error", "message": str(error)}})
            self.send_bytes(200, body, "application/json; charset=utf-8")

    def do_GET(self) -> None:
        path = urllib.parse.urlparse(self.path).path
        if self.headers.get("Upgrade", "").lower() == "websocket":
            self.handle_websocket(path)
            return
        self.serve_static(include_body=True)

    def do_HEAD(self) -> None:
        self.serve_static(include_body=False)

    def serve_static(self, include_body: bool) -> None:
        path = urllib.parse.urlparse(self.path).path
        if path.endswith("/auth/config"):
            self.send_json(200, {"enabled": False})
            return
        if path.endswith("/undefined"):
            self.send_bytes(204, b"", "text/plain; charset=utf-8", include_body)
            return
        conversation_id = conversation_id_from_path(path)
        if conversation_id and path.endswith("/stream"):
            activities = activities_for_conversation(conversation_id)
            self.send_json(200, {"activities": activities, "watermark": str(len(activities))})
            return
        if conversation_id and path.endswith("/activities"):
            activities = activities_for_conversation(conversation_id)
            self.send_json(200, {"activities": activities, "watermark": str(len(activities))})
            return
        if path == "/":
            path = "/test.html"
        clean = posixpath.normpath(urllib.parse.unquote(path)).lstrip("/")
        requested = www_dir / clean
        target = requested.resolve()
        if requested.exists() and target.is_dir():
            requested = requested / "index.html"
            target = requested.resolve()
        if (not requested.exists() or not target.is_file()) and clean.endswith("/i18n/en-US.json"):
            requested = www_dir / clean.removesuffix("/i18n/en-US.json") / "i18n" / "en.json"
            target = requested.resolve()
        if (not requested.exists() or not target.is_file()) and clean == "favicon.ico":
            requested = www_dir / "skins" / skin / "assets" / "favicon.ico"
            target = requested.resolve()
        if not requested.exists() or not target.is_file():
            self.send_bytes(404, b"not found", "text/plain; charset=utf-8", include_body)
            return
        content_type = mimetypes.guess_type(str(target))[0] or "application/octet-stream"
        self.send_bytes(200, target.read_bytes(), content_type, include_body)

    def handle_websocket(self, path: str) -> None:
        conversation_id = conversation_id_from_path(path) or "sorx-business-test"
        key = self.headers.get("Sec-WebSocket-Key")
        if not key:
            self.send_error(400, "Missing Sec-WebSocket-Key")
            return
        accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode("ascii")).digest()).decode("ascii")
        activities = activities_for_conversation(conversation_id)
        self.send_response(101, "Switching Protocols")
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        STREAMS.setdefault(conversation_id, []).append(self.wfile)
        self.wfile.write(websocket_frame({"activities": activities, "watermark": str(len(activities))}))
        self.wfile.flush()
        try:
            while True:
                time.sleep(15)
                self.wfile.write(websocket_ping_frame())
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, OSError):
            try:
                STREAMS.get(conversation_id, []).remove(self.wfile)
            except ValueError:
                pass


url = f"http://{host}:{port}/test.html"
server = ThreadingHTTPServer((host, port), Handler)
print(f"Sorx business test page: {url}", flush=True)
print(f"WebChat assets: {'loaded from disk' if has_webchat_assets else 'fallback panel'}", flush=True)
print("Press Ctrl-C to stop.", flush=True)
if open_browser:
    threading.Thread(target=lambda: (time.sleep(0.4), webbrowser.open(url)), daemon=True).start()
try:
    server.serve_forever()
except KeyboardInterrupt:
    pass
finally:
    server.server_close()
PY
