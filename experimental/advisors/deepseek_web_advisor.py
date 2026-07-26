#!/usr/bin/env python3
"""Standalone experimental DeepSeek web advisor adapter.

This adapter is intentionally outside the released CatDesk worker pipeline. It
accepts a bounded AdviceRequestV1 over authenticated JSON Lines and returns an
AdviceResponseV1-shaped object. It never receives CatDesk tool definitions and
does not expose filesystem, shell, Git, patch, job, verification, or MCP tools.
"""

from __future__ import annotations

import argparse
import dataclasses
import enum
import html.parser
import json
import os
import re
import sys
import time
from pathlib import Path
from typing import Any, Iterable, TextIO
from urllib.parse import urlparse


class AdapterState(str, enum.Enum):
    STOPPED = "STOPPED"
    STARTING = "STARTING"
    LOGIN_REQUIRED = "LOGIN_REQUIRED"
    READY = "READY"
    SENDING = "SENDING"
    WAITING_FOR_RESPONSE = "WAITING_FOR_RESPONSE"
    COMPLETED = "COMPLETED"
    RATE_LIMITED = "RATE_LIMITED"
    TAKEOVER_REQUIRED = "TAKEOVER_REQUIRED"
    TIMED_OUT = "TIMED_OUT"
    CANCELLED = "CANCELLED"
    DEGRADED = "DEGRADED"
    FAILED = "FAILED"


class AdvisorStatus(str, enum.Enum):
    READY = "READY"
    LOGIN_REQUIRED = "LOGIN_REQUIRED"
    TAKEOVER_REQUIRED = "TAKEOVER_REQUIRED"
    RATE_LIMITED = "RATE_LIMITED"
    UNAVAILABLE = "UNAVAILABLE"
    TIMED_OUT = "TIMED_OUT"
    CANCELLED = "CANCELLED"
    COMPLETED = "COMPLETED"
    FAILED = "FAILED"


STATE_TO_STATUS = {
    AdapterState.LOGIN_REQUIRED: AdvisorStatus.LOGIN_REQUIRED,
    AdapterState.TAKEOVER_REQUIRED: AdvisorStatus.TAKEOVER_REQUIRED,
    AdapterState.RATE_LIMITED: AdvisorStatus.RATE_LIMITED,
    AdapterState.TIMED_OUT: AdvisorStatus.TIMED_OUT,
    AdapterState.CANCELLED: AdvisorStatus.CANCELLED,
    AdapterState.COMPLETED: AdvisorStatus.COMPLETED,
    AdapterState.FAILED: AdvisorStatus.FAILED,
    AdapterState.DEGRADED: AdvisorStatus.UNAVAILABLE,
    AdapterState.READY: AdvisorStatus.READY,
}


PROHIBITED_AUTHORITY_KEYS = {
    "tool_definitions",
    "tools",
    "filesystem",
    "shell",
    "git",
    "patch",
    "job",
    "verification",
    "mcp",
}


SECRET_PATTERNS = [
    re.compile(
        r"(?i)(password|passwd|pwd|token|secret|api[_-]?key|authorization)\s*[:=]\s*[\"']?[^\"'\s,;]+"
    ),
    re.compile(r"(?i)bearer\s+[A-Za-z0-9._~+/=-]{12,}"),
    re.compile(r"(?i)(gho|ghp|sk|xoxb|xoxp)_[A-Za-z0-9_=-]{12,}"),
]


@dataclasses.dataclass(frozen=True)
class SelectorConfig:
    start_url: str
    response_container_selectors: list[str]
    prompt_input_selectors: list[str]
    send_button_selectors: list[str]
    stop_button_selectors: list[str]
    login_required_selectors: list[str]
    takeover_required_selectors: list[str]
    rate_limit_markers: list[str]
    completion_markers: list[str]
    text_stability_seconds: float = 2.0
    timeout_seconds: float = 120.0
    bounded_dom_evidence_bytes: int = 2048

    @classmethod
    def from_file(cls, path: Path) -> "SelectorConfig":
        with path.open("r", encoding="utf-8") as handle:
            value = json.load(handle)
        return cls(**value)


@dataclasses.dataclass(frozen=True)
class AdviceResponseV1:
    request_id: str
    advisor_id: str
    status: str
    diagnosis: str
    recommendations: list[str]
    risks: list[str]
    assumptions_or_questions: list[str]
    confidence: str
    raw_artifact_reference: str | None

    def to_dict(self) -> dict[str, Any]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class RedactedDiagnostics:
    state: str
    failed_selector: str | None
    page_title: str
    url_origin: str
    bounded_dom_evidence: str

    def to_dict(self) -> dict[str, str | None]:
        return dataclasses.asdict(self)


class MinimalHtml(html.parser.HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.nodes: list[tuple[str, dict[str, str]]] = []
        self.text_parts: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self.nodes.append((tag.lower(), {k.lower(): v or "" for k, v in attrs}))

    def handle_data(self, data: str) -> None:
        if data.strip():
            self.text_parts.append(data.strip())

    @property
    def text(self) -> str:
        return " ".join(self.text_parts)


@dataclasses.dataclass(frozen=True)
class PageSnapshot:
    html: str
    title: str
    url: str

    @property
    def origin(self) -> str:
        parsed = urlparse(self.url)
        if not parsed.scheme or not parsed.netloc:
            return "unknown"
        return f"{parsed.scheme}://{parsed.netloc}"

    @property
    def parsed(self) -> MinimalHtml:
        parser = MinimalHtml()
        parser.feed(self.html)
        return parser

    def selector_count(self, selector: str) -> int:
        return sum(1 for tag, attrs in self.parsed.nodes if selector_matches(selector, tag, attrs))

    def any_selector(self, selectors: Iterable[str]) -> tuple[bool, str | None]:
        for selector in selectors:
            if self.selector_count(selector) > 0:
                return True, selector
        return False, None

    def redacted_dom_evidence(self, max_bytes: int) -> str:
        evidence_parts = []
        for tag, attrs in self.parsed.nodes:
            safe_attrs = []
            for key in sorted(attrs):
                if key in {"aria-label", "data-testid", "data-ds-role", "data-role", "role", "type"}:
                    safe_attrs.append(f'{key}="{redact(attrs[key])}"')
                elif key in {"id", "class", "src", "href"}:
                    safe_attrs.append(f'{key}="<redacted>"')
            joined = " ".join(safe_attrs)
            evidence_parts.append(f"<{tag}{(' ' + joined) if joined else ''}>")
        return bound_text(" ".join(evidence_parts), max_bytes)


@dataclasses.dataclass(frozen=True)
class CompletionObservation:
    state: AdapterState
    response_count: int
    stop_visible: bool
    send_visible: bool
    stable_for_seconds: float
    marker_seen: bool
    diagnostics: RedactedDiagnostics


def selector_matches(selector: str, tag: str, attrs: dict[str, str]) -> bool:
    selector = selector.strip()
    if not selector:
        return False
    tag_match = re.match(r"^([a-zA-Z][a-zA-Z0-9_-]*)", selector)
    wanted_tag = tag_match.group(1).lower() if tag_match else None
    if wanted_tag and wanted_tag != tag:
        return False
    if selector.startswith("."):
        return selector[1:] in attrs.get("class", "").split()
    if selector.startswith("#"):
        return attrs.get("id") == selector[1:]
    attr_match = re.search(r"\[([a-zA-Z0-9_-]+)([*^$|~]?=)?\"?([^\]\"]*)\"?\]", selector)
    if attr_match:
        name, op, expected = attr_match.groups()
        actual = attrs.get(name.lower())
        if actual is None:
            return False
        if not op:
            return True
        if op == "*=":
            return expected in actual
        return actual == expected
    return wanted_tag == tag


class CompletionDetector:
    def __init__(self, selectors: SelectorConfig) -> None:
        self.selectors = selectors

    def observe(
        self,
        snapshot: PageSnapshot,
        *,
        first_seen_at: float,
        last_text_change_at: float,
        now: float,
    ) -> CompletionObservation:
        text_lower = snapshot.parsed.text.lower()
        takeover, takeover_selector = snapshot.any_selector(
            self.selectors.takeover_required_selectors
        )
        login_required, login_selector = snapshot.any_selector(
            self.selectors.login_required_selectors
        )
        rate_limited = any(
            marker.lower() in text_lower for marker in self.selectors.rate_limit_markers
        )
        response_count = sum(
            snapshot.selector_count(selector)
            for selector in self.selectors.response_container_selectors
        )
        stop_visible, stop_selector = snapshot.any_selector(self.selectors.stop_button_selectors)
        send_visible, send_selector = snapshot.any_selector(self.selectors.send_button_selectors)
        marker_seen = any(
            marker.lower() in text_lower for marker in self.selectors.completion_markers
        )
        stable_for = max(0.0, now - last_text_change_at)
        elapsed = max(0.0, now - first_seen_at)
        failed_selector = None

        if takeover:
            state = AdapterState.TAKEOVER_REQUIRED
            failed_selector = takeover_selector
        elif login_required:
            state = AdapterState.LOGIN_REQUIRED
            failed_selector = login_selector
        elif rate_limited:
            state = AdapterState.RATE_LIMITED
        elif elapsed >= self.selectors.timeout_seconds:
            state = AdapterState.TIMED_OUT
        elif (
            response_count > 0
            and not stop_visible
            and send_visible
            and marker_seen
            and stable_for >= self.selectors.text_stability_seconds
        ):
            state = AdapterState.COMPLETED
        else:
            state = AdapterState.WAITING_FOR_RESPONSE
            if response_count == 0:
                failed_selector = ",".join(self.selectors.response_container_selectors)
            elif stop_visible:
                failed_selector = stop_selector
            elif not send_visible:
                failed_selector = ",".join(self.selectors.send_button_selectors)
            elif not marker_seen:
                failed_selector = "completion_markers"

        diagnostics = RedactedDiagnostics(
            state=state.value,
            failed_selector=failed_selector or send_selector,
            page_title=redact(snapshot.title),
            url_origin=snapshot.origin,
            bounded_dom_evidence=snapshot.redacted_dom_evidence(
                self.selectors.bounded_dom_evidence_bytes
            ),
        )
        return CompletionObservation(
            state=state,
            response_count=response_count,
            stop_visible=stop_visible,
            send_visible=send_visible,
            stable_for_seconds=stable_for,
            marker_seen=marker_seen,
            diagnostics=diagnostics,
        )


class DeepSeekWebAdvisorAdapter:
    """Experimental headed browser adapter using SeleniumBase CDP mode.

    Browser launch is opt-in through start(). The constructor and offline tests
    do not import or start SeleniumBase.
    """

    advisor_id = "deepseek-web-advisor-experimental"

    def __init__(
        self,
        selectors: SelectorConfig,
        profile_dir: Path,
        *,
        headed: bool = True,
    ) -> None:
        self.selectors = selectors
        self.profile_dir = profile_dir
        self.headed = headed
        self.state = AdapterState.STOPPED
        self._sb_context: Any = None
        self._sb: Any = None
        self.detector = CompletionDetector(selectors)

    def start(self) -> AdapterState:
        self.profile_dir.mkdir(parents=True, exist_ok=True)
        self.state = AdapterState.STARTING
        try:
            from seleniumbase import SB  # type: ignore[import-not-found]
        except Exception:
            self.state = AdapterState.DEGRADED
            return self.state
        self._sb_context = SB(
            uc=True,
            test=True,
            headless=not self.headed,
            user_data_dir=str(self.profile_dir),
        )
        self._sb = self._sb_context.__enter__()
        self._sb.activate_cdp_mode(self.selectors.start_url)
        self.state = AdapterState.LOGIN_REQUIRED
        return self.state

    def stop(self) -> None:
        self.state = AdapterState.STOPPED
        if self._sb_context is not None:
            self._sb_context.__exit__(None, None, None)
        self._sb_context = None
        self._sb = None

    def advice_unavailable(self, request_id: str) -> AdviceResponseV1:
        status = STATE_TO_STATUS.get(self.state, AdvisorStatus.UNAVAILABLE)
        return AdviceResponseV1(
            request_id=request_id,
            advisor_id=self.advisor_id,
            status=status.value,
            diagnosis="DeepSeek web advisor is not ready for an advisory turn.",
            recommendations=[],
            risks=["No browser advice was obtained."],
            assumptions_or_questions=["User may need to complete manual login or verification."],
            confidence="LOW",
            raw_artifact_reference=None,
        )


class JsonLinesAdvisorProtocol:
    def __init__(self, adapter: DeepSeekWebAdvisorAdapter, auth_token: str) -> None:
        if not auth_token:
            raise ValueError("auth_token must be non-empty")
        self.adapter = adapter
        self.auth_token = auth_token

    def handle(self, message: dict[str, Any]) -> dict[str, Any]:
        if message.get("auth_token") != self.auth_token:
            return {"ok": False, "error": "UNAUTHORIZED"}
        command = message.get("command")
        if command == "hello":
            return {
                "ok": True,
                "advisor_id": self.adapter.advisor_id,
                "state": self.adapter.state.value,
                "protocol": "catdesk.advisor.deepseek.jsonl.v1",
            }
        if command == "status":
            return {"ok": True, "state": self.adapter.state.value}
        if command == "start":
            return {"ok": True, "state": self.adapter.start().value}
        if command == "shutdown":
            self.adapter.stop()
            return {"ok": True, "state": self.adapter.state.value}
        if command == "advise":
            request = message.get("request") or {}
            authority_error = contains_prohibited_authority(request)
            if authority_error:
                return {"ok": False, "error": f"PROHIBITED_AUTHORITY:{authority_error}"}
            request_id = str(request.get("request_id") or "unknown-request")
            response = self.adapter.advice_unavailable(request_id)
            return {"ok": True, "response": response.to_dict()}
        return {"ok": False, "error": "UNKNOWN_COMMAND"}

    def serve(self, input_stream: TextIO, output_stream: TextIO) -> None:
        for line in input_stream:
            if not line.strip():
                continue
            try:
                message = json.loads(line)
                response = self.handle(message)
            except Exception as exc:
                response = {"ok": False, "error": f"FAILED:{type(exc).__name__}"}
            output_stream.write(json.dumps(response, separators=(",", ":")) + "\n")
            output_stream.flush()


def contains_prohibited_authority(value: Any) -> str | None:
    if isinstance(value, dict):
        for key, nested in value.items():
            if str(key).lower() in PROHIBITED_AUTHORITY_KEYS:
                return str(key)
            found = contains_prohibited_authority(nested)
            if found:
                return found
    elif isinstance(value, list):
        for nested in value:
            found = contains_prohibited_authority(nested)
            if found:
                return found
    return None


def redact(value: str) -> str:
    redacted = value
    for pattern in SECRET_PATTERNS:
        redacted = pattern.sub("<redacted>", redacted)
    return redacted


def bound_text(value: str, max_bytes: int) -> str:
    if len(value.encode("utf-8")) <= max_bytes:
        return value
    suffix = "\n[truncated]"
    budget = max(0, max_bytes - len(suffix.encode("utf-8")))
    output = ""
    for char in value:
        if len((output + char).encode("utf-8")) > budget:
            break
        output += char
    return output + suffix if budget else suffix[:max_bytes]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile-dir", required=True, type=Path)
    parser.add_argument(
        "--selectors",
        default=Path(__file__).with_name("deepseek_selectors.json"),
        type=Path,
    )
    parser.add_argument("--headed", action="store_true")
    args = parser.parse_args(argv)

    auth_token = os.environ.get("CATDESK_ADVISOR_AUTH_TOKEN", "")
    if not auth_token:
        sys.stderr.write("CATDESK_ADVISOR_AUTH_TOKEN is required\n")
        return 2
    selectors = SelectorConfig.from_file(args.selectors)
    adapter = DeepSeekWebAdvisorAdapter(selectors, args.profile_dir, headed=args.headed)
    JsonLinesAdvisorProtocol(adapter, auth_token).serve(sys.stdin, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
