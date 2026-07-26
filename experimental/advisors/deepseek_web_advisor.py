#!/usr/bin/env python3
"""Standalone experimental DeepSeek web advisor adapter.

This adapter is intentionally outside the released CatDesk worker pipeline. It
accepts a bounded AdviceRequestV1 over authenticated JSON Lines and returns an
AdviceResponseV1-shaped object. It never receives CatDesk tool definitions and
does not expose filesystem, shell, Git, patch, job, verification, or MCP tools.
"""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import enum
import html.parser
import json
import os
import random
import re
import sys
import threading
import time
from pathlib import Path
from typing import Any, Iterable, TextIO
from urllib.parse import urlparse


SCHEMA_VERSION = 1
DEFAULT_MAX_REQUEST_BYTES = 24 * 1024
DEFAULT_MAX_PROMPT_BYTES = 12 * 1024
DEFAULT_MAX_RESPONSE_BYTES = 4 * 1024
REQUEST_ID_RE = re.compile(r"^[A-Za-z0-9_.:-]{1,120}$")
TRUSTED_SCHEME = "https"
TRUSTED_HOST = "chat.deepseek.com"
TRUSTED_ORIGIN = f"{TRUSTED_SCHEME}://{TRUSTED_HOST}"
REMOTE_DISCLOSURE_CLASSIFICATION = "REMOTE_ALLOWED"
DEEPSEEK_COMPOSER_SEND_SELECTOR = "deepseek:composer-send"
DEEPSEEK_COMPOSER_STOP_SELECTOR = "deepseek:composer-stop"
DEEPSEEK_CONVERSATION_BLOCKS_SELECTOR = "deepseek:conversation-visible-blocks"


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

ALLOWED_REQUEST_KEYS = {
    "schemaversion",
    "requestid",
    "runid",
    "objective",
    "currentstep",
    "specificquestion",
    "constraints",
    "latestfailure",
    "boundedsourceexcerpts",
    "boundedpatchordiffsummary",
    "verificationsummary",
    "disclosureclassification",
    "maximumresponselength",
}


SECRET_PATTERNS = [
    re.compile(
        r"(?i)(password|passwd|pwd|token|secret|api[_-]?key|authorization)\s*[:=]\s*[\"']?[^\"'\s,;]+"
    ),
    re.compile(r"(?i)bearer\s+[A-Za-z0-9._~+/=-]{12,}"),
    re.compile(r"(?i)(gho|ghp|sk|xoxb|xoxp)_[A-Za-z0-9_=-]{12,}"),
]


DEEPSEEK_COMPOSER_ACTION_AVAILABLE_SCRIPT = r"""
(() => {
  const textarea = document.querySelector('textarea');
  if (!textarea) return false;
  const textareaRect = textarea.getBoundingClientRect();
  const candidates = Array.from(document.querySelectorAll('button,[role="button"]'))
    .filter((element) => {
      const rect = element.getBoundingClientRect();
      const style = window.getComputedStyle(element);
      if (rect.width <= 0 || rect.height <= 0) return false;
      if (style.visibility === 'hidden' || style.display === 'none') return false;
      if (element.disabled || element.getAttribute('aria-disabled') === 'true') return false;
      return rect.top >= textareaRect.top - 140
        && rect.bottom <= textareaRect.bottom + 180
        && rect.right >= textareaRect.left
        && rect.left <= window.innerWidth - 24;
    })
    .sort((left, right) => {
      const a = left.getBoundingClientRect();
      const b = right.getBoundingClientRect();
      return (a.top - b.top) || (a.left - b.left);
    });
  return candidates.length > 0;
})()
"""


DEEPSEEK_COMPOSER_ACTION_CLICK_SCRIPT = r"""
(() => {
  const textarea = document.querySelector('textarea');
  if (!textarea) return false;
  const textareaRect = textarea.getBoundingClientRect();
  const candidates = Array.from(document.querySelectorAll('button,[role="button"]'))
    .filter((element) => {
      const rect = element.getBoundingClientRect();
      const style = window.getComputedStyle(element);
      if (rect.width <= 0 || rect.height <= 0) return false;
      if (style.visibility === 'hidden' || style.display === 'none') return false;
      if (element.disabled || element.getAttribute('aria-disabled') === 'true') return false;
      return rect.top >= textareaRect.top - 140
        && rect.bottom <= textareaRect.bottom + 180
        && rect.right >= textareaRect.left
        && rect.left <= window.innerWidth - 24;
    })
    .sort((left, right) => {
      const a = left.getBoundingClientRect();
      const b = right.getBoundingClientRect();
      return (a.top - b.top) || (a.left - b.left);
    });
  const target = candidates[candidates.length - 1];
  if (!target) return false;
  target.click();
  return true;
})()
"""


DEEPSEEK_VISIBLE_CONVERSATION_BLOCKS_SCRIPT = r"""
(() => {
  const textarea = document.querySelector('textarea');
  if (!textarea) return [];
  const textareaRect = textarea.getBoundingClientRect();
  const candidates = Array.from(document.querySelectorAll('main article,main section,main div,main p,main pre'))
    .filter((element) => {
      if (element.contains(textarea)) return false;
      const rect = element.getBoundingClientRect();
      const style = window.getComputedStyle(element);
      if (rect.width <= 0 || rect.height <= 0) return false;
      if (style.visibility === 'hidden' || style.display === 'none') return false;
      if (rect.bottom >= textareaRect.top - 8) return false;
      const horizontalOverlap = Math.min(rect.right, textareaRect.right) - Math.max(rect.left, textareaRect.left);
      if (horizontalOverlap <= Math.min(rect.width, textareaRect.width) * 0.20) return false;
      const text = (element.innerText || '').replace(/\s+/g, ' ').trim();
      return text.length >= 12 && text.length <= 6000;
    })
    .sort((left, right) => {
      const a = left.getBoundingClientRect();
      const b = right.getBoundingClientRect();
      return (a.top - b.top) || (a.left - b.left);
    });
  const output = [];
  for (const element of candidates) {
    const text = (element.innerText || '').replace(/\s+/g, ' ').trim();
    if (!text) continue;
    if (output.some((existing) => existing === text || existing.includes(text))) continue;
    output.push(text);
  }
  return output.slice(-20);
})()
"""


@dataclasses.dataclass(frozen=True)
class SelectorConfig:
    start_url: str
    response_container_selectors: list[str]
    prompt_input_selectors: list[str]
    send_button_selectors: list[str]
    stop_button_selectors: list[str]
    login_required_selectors: list[str]
    login_email_selectors: list[str]
    login_password_selectors: list[str]
    login_submit_selectors: list[str]
    takeover_required_selectors: list[str]
    cookie_banner_selectors: list[str]
    cookie_reject_selectors: list[str]
    cookie_accept_selectors: list[str]
    rate_limit_selectors: list[str]
    completion_markers: list[str]
    chat_menu_button_selectors: list[str]
    chat_item_selectors: list[str]
    current_chat_title_selectors: list[str]
    text_stability_seconds: float = 2.0
    stable_sample_count: int = 3
    timeout_seconds: float = 120.0
    typing_min_interval_seconds: float = 0.002
    typing_max_interval_seconds: float = 0.008
    typing_newline_pause_seconds: float = 0.04
    typing_timeout_seconds: float = 90.0
    bounded_dom_evidence_bytes: int = 2048

    @classmethod
    def from_file(cls, path: Path) -> "SelectorConfig":
        with path.open("r", encoding="utf-8") as handle:
            value = json.load(handle)
        value.setdefault("login_email_selectors", [])
        value.setdefault("login_password_selectors", [])
        value.setdefault("login_submit_selectors", [])
        value.setdefault("cookie_banner_selectors", [])
        value.setdefault("cookie_reject_selectors", [])
        value.setdefault("cookie_accept_selectors", [])
        value.setdefault("rate_limit_selectors", [])
        value.setdefault("chat_menu_button_selectors", [])
        value.setdefault("chat_item_selectors", [])
        value.setdefault("current_chat_title_selectors", [])
        value.setdefault("stable_sample_count", 3)
        value.setdefault("typing_min_interval_seconds", 0.002)
        value.setdefault("typing_max_interval_seconds", 0.008)
        value.setdefault("typing_newline_pause_seconds", 0.04)
        value.setdefault("typing_timeout_seconds", 90.0)
        value.pop("rate_limit_markers", None)
        return cls(**value)


@dataclasses.dataclass(frozen=True)
class AdviceResponseV1:
    schema_version: int
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


class ResponseTextHtml(html.parser.HTMLParser):
    def __init__(self, selectors: Iterable[str]) -> None:
        super().__init__(convert_charrefs=True)
        self.selectors = list(selectors)
        self.responses: list[str] = []
        self._depth = 0
        self._parts: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        normalized_attrs = {k.lower(): v or "" for k, v in attrs}
        if self._depth > 0:
            self._depth += 1
            return
        if (
            any(selector_matches(selector, tag.lower(), normalized_attrs) for selector in self.selectors)
            and attrs_are_visible(normalized_attrs)
        ):
            self._depth = 1
            self._parts = []

    def handle_endtag(self, _tag: str) -> None:
        if self._depth == 0:
            return
        self._depth -= 1
        if self._depth == 0:
            self.responses.append(normalize_plain_text(" ".join(self._parts)))
            self._parts = []

    def handle_data(self, data: str) -> None:
        if self._depth > 0 and data.strip():
            self._parts.append(data.strip())


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

    def visible_selector_count(self, selector: str) -> int:
        return sum(
            1
            for tag, attrs in self.parsed.nodes
            if selector_matches(selector, tag, attrs) and attrs_are_visible(attrs)
        )

    def selector_is_visible(self, selector: str) -> bool:
        return self.visible_selector_count(selector) > 0

    def selector_is_enabled(self, selector: str) -> bool:
        for tag, attrs in self.parsed.nodes:
            if selector_matches(selector, tag, attrs) and attrs_are_visible(attrs):
                if attrs.get("disabled") is not None:
                    return False
                if attrs.get("aria-disabled", "").lower() == "true":
                    return False
                return True
        return False

    def any_selector(self, selectors: Iterable[str]) -> tuple[bool, str | None]:
        for selector in selectors:
            if self.selector_is_visible(selector):
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

    def response_texts(self, selectors: Iterable[str]) -> list[str]:
        parser = ResponseTextHtml(selectors)
        parser.feed(self.html)
        return parser.responses


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
    if selector.startswith("deepseek:"):
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


def attrs_are_visible(attrs: dict[str, str]) -> bool:
    if "hidden" in attrs:
        return False
    if attrs.get("aria-hidden", "").lower() == "true":
        return False
    style = attrs.get("style", "").lower().replace(" ", "")
    return "display:none" not in style and "visibility:hidden" not in style


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
        takeover, takeover_selector = snapshot.any_selector(
            self.selectors.takeover_required_selectors
        )
        login_required, login_selector = snapshot.any_selector(
            self.selectors.login_required_selectors
        )
        rate_limited, rate_limit_selector = snapshot.any_selector(self.selectors.rate_limit_selectors)
        response_texts = snapshot.response_texts(self.selectors.response_container_selectors)
        newest_text = response_texts[-1] if response_texts else ""
        response_count = len(response_texts)
        stop_visible, stop_selector = snapshot.any_selector(self.selectors.stop_button_selectors)
        send_visible, send_selector = snapshot.any_selector(self.selectors.send_button_selectors)
        marker_seen = any(
            marker.lower() in newest_text.lower() for marker in self.selectors.completion_markers
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
            failed_selector = rate_limit_selector
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


@dataclasses.dataclass(frozen=True)
class ValidatedAdviceRequest:
    raw: dict[str, Any]
    request_id: str
    objective: str
    current_step: str
    specific_question: str
    constraints: list[str]
    latest_failure: str | None
    bounded_source_excerpts: list[dict[str, str]]
    bounded_patch_or_diff_summary: str | None
    verification_summary: str | None
    maximum_response_length: int


def normalize_key(value: str) -> str:
    return re.sub(r"[^a-z0-9]", "", value.lower())


def get_field(request: dict[str, Any], normalized_name: str, default: Any = None) -> Any:
    for key, value in request.items():
        if normalize_key(str(key)) == normalized_name:
            return value
    return default


def validate_advice_request(request: Any) -> ValidatedAdviceRequest:
    if not isinstance(request, dict):
        raise ValueError("REQUEST_MUST_BE_OBJECT")
    serialized = json.dumps(request, separators=(",", ":"), sort_keys=True)
    if len(serialized.encode("utf-8")) > DEFAULT_MAX_REQUEST_BYTES:
        raise ValueError("REQUEST_TOO_LARGE")
    authority_error = contains_prohibited_authority(request)
    if authority_error:
        raise ValueError(f"PROHIBITED_AUTHORITY:{authority_error}")
    unexpected = sorted(
        str(key)
        for key in request
        if normalize_key(str(key)) not in ALLOWED_REQUEST_KEYS
    )
    if unexpected:
        raise ValueError(f"UNSUPPORTED_REQUEST_KEYS:{','.join(unexpected)}")
    if get_field(request, "schemaversion") != SCHEMA_VERSION:
        raise ValueError("UNSUPPORTED_SCHEMA_VERSION")
    disclosure_classification = str(
        get_field(request, "disclosureclassification", "")
    ).strip()
    if disclosure_classification != REMOTE_DISCLOSURE_CLASSIFICATION:
        raise ValueError("DISCLOSURE_DENIED")

    request_id = str(get_field(request, "requestid", "")).strip()
    if not REQUEST_ID_RE.match(request_id):
        raise ValueError("INVALID_REQUEST_ID")
    objective = str(get_field(request, "objective", "")).strip()
    specific_question = str(get_field(request, "specificquestion", "")).strip()
    if not objective:
        raise ValueError("MISSING_OBJECTIVE")
    if not specific_question:
        raise ValueError("MISSING_SPECIFIC_QUESTION")

    maximum_response_length = get_field(
        request,
        "maximumresponselength",
        DEFAULT_MAX_RESPONSE_BYTES,
    )
    try:
        maximum_response_length = int(maximum_response_length)
    except (TypeError, ValueError):
        raise ValueError("INVALID_MAXIMUM_RESPONSE_LENGTH") from None
    maximum_response_length = max(1, min(maximum_response_length, DEFAULT_MAX_RESPONSE_BYTES))

    constraints = get_field(request, "constraints", [])
    if constraints is None:
        constraints = []
    if not isinstance(constraints, list):
        raise ValueError("INVALID_CONSTRAINTS")
    excerpts = get_field(request, "boundedsourceexcerpts", [])
    if excerpts is None:
        excerpts = []
    if not isinstance(excerpts, list):
        raise ValueError("INVALID_EXCERPTS")

    normalized_excerpts = []
    for excerpt in excerpts:
        if not isinstance(excerpt, dict):
            raise ValueError("INVALID_EXCERPT")
        normalized_excerpts.append(
            {
                "source": bound_text(redact(str(get_field(excerpt, "source", ""))), 512),
                "summary": bound_text(redact(str(get_field(excerpt, "summary", ""))), 1024),
                "content": bound_text(redact(str(get_field(excerpt, "content", ""))), 2048),
            }
        )

    return ValidatedAdviceRequest(
        raw=request,
        request_id=request_id,
        objective=bound_text(redact(objective), 2048),
        current_step=bound_text(redact(str(get_field(request, "currentstep", ""))), 1024),
        specific_question=bound_text(redact(specific_question), 2048),
        constraints=[bound_text(redact(str(item)), 512) for item in constraints],
        latest_failure=optional_bounded(request, "latestfailure"),
        bounded_source_excerpts=normalized_excerpts,
        bounded_patch_or_diff_summary=optional_bounded(
            request,
            "boundedpatchordiffsummary",
        ),
        verification_summary=optional_bounded(request, "verificationsummary"),
        maximum_response_length=maximum_response_length,
    )


def optional_bounded(request: dict[str, Any], normalized_name: str) -> str | None:
    value = get_field(request, normalized_name)
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        return None
    return bound_text(redact(text), 2048)


def build_advisory_prompt(request: ValidatedAdviceRequest) -> str:
    sections = [
        "You are an external advisory model. You have no CatDesk tools and no execution authority.",
        "Return concise debugging advice only. Do not claim to inspect files that are not included.",
        f"Request ID: {request.request_id}",
        f"Objective:\n{request.objective}",
        f"Current step:\n{request.current_step}",
        f"Specific question:\n{request.specific_question}",
    ]
    if request.constraints:
        sections.append("Constraints:\n" + "\n".join(f"- {item}" for item in request.constraints))
    if request.latest_failure:
        sections.append(f"Latest failure:\n{request.latest_failure}")
    if request.bounded_patch_or_diff_summary:
        sections.append(f"Patch or diff summary:\n{request.bounded_patch_or_diff_summary}")
    if request.verification_summary:
        sections.append(f"Verification summary:\n{request.verification_summary}")
    for index, excerpt in enumerate(request.bounded_source_excerpts, start=1):
        sections.append(
            "\n".join(
                [
                    f"Bounded source excerpt {index}:",
                    f"Source: {excerpt['source']}",
                    f"Summary: {excerpt['summary']}",
                    excerpt["content"],
                ]
            )
        )
    return bound_text("\n\n".join(sections), DEFAULT_MAX_PROMPT_BYTES)


def normalize_advice_response(
    request: ValidatedAdviceRequest,
    advisor_id: str,
    status: AdvisorStatus,
    text: str,
    *,
    confidence: str = "MEDIUM",
) -> AdviceResponseV1:
    return AdviceResponseV1(
        schema_version=SCHEMA_VERSION,
        request_id=request.request_id,
        advisor_id=advisor_id,
        status=status.value,
        diagnosis=bound_text(normalize_plain_text(redact(text)), request.maximum_response_length),
        recommendations=[],
        risks=[],
        assumptions_or_questions=[],
        confidence=confidence,
        raw_artifact_reference=None,
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
        allow_env_login: bool = False,
        chat_name: str | None = None,
        rng: random.Random | None = None,
    ) -> None:
        self.selectors = selectors
        self.profile_dir = profile_dir
        self.headed = headed
        self.allow_env_login = allow_env_login
        self.chat_name = chat_name.strip() if chat_name else None
        self.rng = rng or random.Random()
        self.state = AdapterState.STOPPED
        self._sb_context: Any = None
        self._sb: Any = None
        self.detector = CompletionDetector(selectors)
        self.cancel_requested = threading.Event()

    def start(self) -> AdapterState:
        self.profile_dir.mkdir(parents=True, exist_ok=True)
        self.state = AdapterState.STARTING
        try:
            if not self.selector_start_url_is_trusted():
                self.state = AdapterState.DEGRADED
                return self.state
            with contextlib.redirect_stdout(sys.stderr):
                SB = self._import_seleniumbase()
                self._sb_context = SB(
                    uc=True,
                    test=True,
                    headless=not self.headed,
                    user_data_dir=str(self.profile_dir),
                )
                self._sb = self._sb_context.__enter__()
                self._sb.activate_cdp_mode(self.selectors.start_url)
            self._handle_cookie_banner()
            return self.refresh_state()
        except Exception as exc:
            sys.stderr.write(f"DeepSeek advisor startup failed: {type(exc).__name__}\n")
            self._cleanup_browser_context()
            self.state = AdapterState.DEGRADED
            return self.state

    def _import_seleniumbase(self) -> Any:
        from seleniumbase import SB  # type: ignore[import-not-found]

        return SB

    def stop(self) -> None:
        self.state = AdapterState.STOPPED
        self._cleanup_browser_context()
        self.cancel_requested.clear()

    def _cleanup_browser_context(self) -> None:
        if self._sb_context is not None:
            with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                self._sb_context.__exit__(None, None, None)
        self._sb_context = None
        self._sb = None

    def refresh_state(self) -> AdapterState:
        if self._sb is None:
            self.state = AdapterState.STOPPED
            return self.state
        try:
            snapshot = self._snapshot()
        except Exception:
            self.state = AdapterState.DEGRADED
            return self.state
        if snapshot.origin != self.expected_origin:
            self.state = AdapterState.DEGRADED
            return self.state
        if self._handle_cookie_banner(snapshot):
            snapshot = self._snapshot()
        takeover, _selector = snapshot.any_selector(self.selectors.takeover_required_selectors)
        if takeover:
            self.state = AdapterState.TAKEOVER_REQUIRED
            return self.state
        login_required, _selector = snapshot.any_selector(self.selectors.login_required_selectors)
        if login_required:
            if self.allow_env_login and self._attempt_env_login():
                snapshot = self._snapshot()
                login_required, _selector = snapshot.any_selector(
                    self.selectors.login_required_selectors
                )
                takeover, _selector = snapshot.any_selector(
                    self.selectors.takeover_required_selectors
                )
                if takeover:
                    self.state = AdapterState.TAKEOVER_REQUIRED
                    return self.state
                if not login_required:
                    return self.refresh_state()
            self.state = AdapterState.LOGIN_REQUIRED
            return self.state
        rate_limited, _selector = snapshot.any_selector(self.selectors.rate_limit_selectors)
        if rate_limited:
            self.state = AdapterState.RATE_LIMITED
            return self.state
        stop_visible = self._any_visible(snapshot, self.selectors.stop_button_selectors)
        prompt_ready, _selector = self._any_visible_selector(
            snapshot,
            self.selectors.prompt_input_selectors,
        )
        if prompt_ready and not stop_visible:
            if self.chat_name and not self._ensure_chat_selected(self.chat_name):
                self.state = AdapterState.DEGRADED
                return self.state
            self.state = AdapterState.READY
            return self.state
        self.state = AdapterState.DEGRADED
        return self.state

    @property
    def expected_origin(self) -> str:
        return TRUSTED_ORIGIN

    def selector_start_url_is_trusted(self) -> bool:
        parsed = urlparse(self.selectors.start_url)
        return (
            parsed.scheme == TRUSTED_SCHEME
            and parsed.hostname == TRUSTED_HOST
            and parsed.username is None
            and parsed.password is None
        )

    def advise(self, request: dict[str, Any]) -> AdviceResponseV1:
        validated = validate_advice_request(request)
        try:
            if self.refresh_state() != AdapterState.READY:
                return self.advice_unavailable(validated.request_id)
            snapshot = self._snapshot()
            if snapshot.origin != self.expected_origin:
                self.state = AdapterState.DEGRADED
                return self.advice_unavailable(validated.request_id)

            prompt = build_advisory_prompt(validated)
            baseline_texts = self._visible_response_texts(snapshot)
            baseline_count = len(baseline_texts)
            prompt_selector = self._first_enabled_visible_selector(
                snapshot,
                self.selectors.prompt_input_selectors,
            )
            if prompt_selector is None:
                self.state = AdapterState.DEGRADED
                return self.advice_unavailable(validated.request_id)

            self.cancel_requested.clear()
            self.state = AdapterState.SENDING
            self._type_prompt(prompt_selector, prompt)
            if self.cancel_requested.is_set():
                self.state = AdapterState.CANCELLED
                return normalize_advice_response(
                    validated,
                    self.advisor_id,
                    AdvisorStatus.CANCELLED,
                    "DeepSeek advisory generation was cancelled.",
                    confidence="LOW",
                )
            if not self._click_first_enabled(self.selectors.send_button_selectors):
                self._press_enter(prompt_selector)
            self.state = AdapterState.WAITING_FOR_RESPONSE
            return self._wait_for_response(validated, baseline_texts)
        except Exception as exc:
            sys.stderr.write(f"DeepSeek advisor operation failed: {type(exc).__name__}\n")
            self.state = AdapterState.DEGRADED
            return self.advice_failure(validated.request_id)

    def cancel(self, request_id: str | None = None) -> AdviceResponseV1:
        self.cancel_requested.set()
        if self.state in {AdapterState.SENDING, AdapterState.WAITING_FOR_RESPONSE}:
            self._try_click_stop()
        self.state = AdapterState.CANCELLED
        return AdviceResponseV1(
            schema_version=SCHEMA_VERSION,
            request_id=request_id or "cancel",
            advisor_id=self.advisor_id,
            status=AdvisorStatus.CANCELLED.value,
            diagnosis="DeepSeek advisory generation was cancelled.",
            recommendations=[],
            risks=[],
            assumptions_or_questions=[],
            confidence="LOW",
            raw_artifact_reference=None,
        )

    def _wait_for_response(
        self,
        request: ValidatedAdviceRequest,
        baseline_texts: list[str],
    ) -> AdviceResponseV1:
        started_at = self._now()
        last_text = ""
        last_text_change_at = started_at
        stable_samples = 0
        while self._now() - started_at <= self.selectors.timeout_seconds:
            if self.cancel_requested.is_set():
                self._try_click_stop()
                self.state = AdapterState.CANCELLED
                return normalize_advice_response(
                    request,
                    self.advisor_id,
                    AdvisorStatus.CANCELLED,
                    "DeepSeek advisory generation was cancelled.",
                    confidence="LOW",
                )
            snapshot = self._snapshot()
            if snapshot.origin != self.expected_origin:
                self.state = AdapterState.DEGRADED
                return self.advice_unavailable(request.request_id)
            takeover, _selector = snapshot.any_selector(self.selectors.takeover_required_selectors)
            if takeover:
                self.state = AdapterState.TAKEOVER_REQUIRED
                return self.advice_unavailable(request.request_id)
            login_required, _selector = snapshot.any_selector(self.selectors.login_required_selectors)
            if login_required:
                self.state = AdapterState.LOGIN_REQUIRED
                return self.advice_unavailable(request.request_id)
            rate_limited, _selector = snapshot.any_selector(self.selectors.rate_limit_selectors)
            if rate_limited:
                self.state = AdapterState.RATE_LIMITED
                return self.advice_unavailable(request.request_id)

            response_texts = self._visible_response_texts(snapshot)
            newest_text = newest_new_response_text(response_texts, baseline_texts)
            if newest_text != last_text:
                last_text = newest_text
                last_text_change_at = self._now()
                stable_samples = 0
            elif newest_text:
                stable_samples += 1
            stable_for = self._now() - last_text_change_at
            stop_visible = self._any_visible(snapshot, self.selectors.stop_button_selectors)
            send_ready = self._first_enabled_visible_selector(
                snapshot,
                self.selectors.send_button_selectors,
            ) is not None
            if not send_ready and not self._any_visible(
                snapshot,
                self.selectors.send_button_selectors,
            ):
                send_ready = self._any_visible(snapshot, self.selectors.prompt_input_selectors)
            marker_seen = any(
                marker.lower() in newest_text.lower() for marker in self.selectors.completion_markers
            )

            if len(response_texts) > len(baseline_texts) and not newest_text and not stop_visible:
                self.state = AdapterState.FAILED
                return self.advice_unavailable(request.request_id)
            if (
                newest_text
                and not stop_visible
                and send_ready
                and marker_seen
                and stable_for >= self.selectors.text_stability_seconds
                and stable_samples >= max(1, self.selectors.stable_sample_count)
            ):
                self.state = AdapterState.COMPLETED
                return normalize_advice_response(
                    request,
                    self.advisor_id,
                    AdvisorStatus.COMPLETED,
                    newest_text,
                )
            self._sleep(0.25)
        self.state = AdapterState.TIMED_OUT
        return self.advice_unavailable(request.request_id)

    def _try_click_stop(self) -> None:
        try:
            self._click_first(self.selectors.stop_button_selectors)
        except Exception as exc:
            sys.stderr.write(f"DeepSeek advisor stop control failed: {type(exc).__name__}\n")

    def _snapshot(self) -> PageSnapshot:
        if self._sb is None:
            raise RuntimeError("browser not started")
        with contextlib.redirect_stdout(sys.stderr):
            html = call_first(self._sb, ["get_page_source", "get_page_html"]) or ""
            title = call_first(self._sb, ["get_page_title", "get_title"]) or ""
            url = call_first(self._sb, ["get_current_url"]) or getattr(
                getattr(self._sb, "driver", None),
                "current_url",
                "",
            )
        return PageSnapshot(html=str(html), title=str(title), url=str(url))

    def _find_prompt_selector(self, snapshot: PageSnapshot) -> str | None:
        for selector in self.selectors.prompt_input_selectors:
            if snapshot.selector_is_visible(selector):
                return selector
        return None

    def _type_prompt(self, selector: str, prompt: str) -> None:
        if self._sb is None:
            raise RuntimeError("browser not started")
        with contextlib.redirect_stdout(sys.stderr):
            if hasattr(self._sb, "click"):
                self._sb.click(selector)
            self._clear_prompt(selector)
            existing = self._prompt_value(selector)
            if existing not in {None, ""}:
                raise RuntimeError("prompt input retained text after clear")
        self._type_text_paced(selector, prompt)

    def _type_text_paced(self, selector: str, text: str) -> None:
        if self._sb is None:
            raise RuntimeError("browser not started")
        started_at = self._now()
        minimum = max(0.0, self.selectors.typing_min_interval_seconds)
        maximum = max(minimum, self.selectors.typing_max_interval_seconds)
        for char in text:
            if self.cancel_requested.is_set():
                return
            if self._now() - started_at > self.selectors.typing_timeout_seconds:
                raise TimeoutError("paced typing timed out")
            with contextlib.redirect_stdout(sys.stderr):
                if hasattr(self._sb, "press_keys"):
                    self._sb.press_keys(selector, char)
                elif hasattr(self._sb, "type"):
                    self._sb.type(selector, char)
            delay = self.selectors.typing_newline_pause_seconds if char == "\n" else self.rng.uniform(minimum, maximum)
            self._sleep(delay)

    def _clear_prompt(self, selector: str) -> None:
        if self._sb is None:
            raise RuntimeError("browser not started")
        if hasattr(self._sb, "clear"):
            self._sb.clear(selector)
            return
        if hasattr(self._sb, "clear_text"):
            self._sb.clear_text(selector)
            return
        if hasattr(self._sb, "set_value"):
            self._sb.set_value(selector, "")
            return
        if hasattr(self._sb, "press_keys"):
            self._sb.press_keys(selector, "\ue009a")
            self._sb.press_keys(selector, "\ue003")

    def _prompt_value(self, selector: str) -> str | None:
        if self._sb is None:
            return None
        for method_name in ["get_attribute", "get_element_attribute"]:
            method = getattr(self._sb, method_name, None)
            if callable(method):
                with contextlib.suppress(Exception):
                    value = method(selector, "value")
                    if value is not None:
                        return str(value)
        return None

    def _press_enter(self, selector: str) -> None:
        if self._sb is None:
            return
        with contextlib.redirect_stdout(sys.stderr):
            if hasattr(self._sb, "press_keys"):
                self._sb.press_keys(selector, "\n")

    def _click_first(self, selectors: Iterable[str]) -> bool:
        return self._click_first_enabled(selectors)

    def _click_first_enabled(self, selectors: Iterable[str]) -> bool:
        if self._sb is None:
            return False
        snapshot = self._snapshot()
        for selector in selectors:
            if selector == DEEPSEEK_COMPOSER_SEND_SELECTOR:
                if self._click_deepseek_composer_action():
                    return True
                continue
            if (
                self._selector_is_visible(snapshot, selector)
                and self._selector_is_enabled(snapshot, selector)
                and hasattr(self._sb, "click")
            ):
                with contextlib.redirect_stdout(sys.stderr):
                    self._sb.click(selector)
                return True
        return False

    def _any_visible(self, snapshot: PageSnapshot, selectors: Iterable[str]) -> bool:
        return any(self._selector_is_visible(snapshot, selector) for selector in selectors)

    def _any_visible_selector(
        self,
        snapshot: PageSnapshot,
        selectors: Iterable[str],
    ) -> tuple[bool, str | None]:
        for selector in selectors:
            if self._selector_is_visible(snapshot, selector):
                return True, selector
        return False, None

    def _first_enabled_visible_selector(
        self,
        snapshot: PageSnapshot,
        selectors: Iterable[str],
    ) -> str | None:
        for selector in selectors:
            if self._selector_is_visible(snapshot, selector) and self._selector_is_enabled(
                snapshot,
                selector,
            ):
                return selector
        return None

    def _selector_is_visible(self, snapshot: PageSnapshot, selector: str) -> bool:
        if selector in {DEEPSEEK_COMPOSER_SEND_SELECTOR, DEEPSEEK_COMPOSER_STOP_SELECTOR}:
            return self._deepseek_composer_action_available()
        if self._sb is not None:
            method = getattr(self._sb, "is_element_visible", None)
            if callable(method):
                with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                    return bool(method(selector))
        return snapshot.selector_is_visible(selector)

    def _selector_is_enabled(self, snapshot: PageSnapshot, selector: str) -> bool:
        if selector in {DEEPSEEK_COMPOSER_SEND_SELECTOR, DEEPSEEK_COMPOSER_STOP_SELECTOR}:
            return self._deepseek_composer_action_available()
        if self._sb is not None:
            method = getattr(self._sb, "is_element_enabled", None)
            if callable(method):
                with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                    return bool(method(selector))
        return snapshot.selector_is_enabled(selector)

    def _visible_response_texts(self, snapshot: PageSnapshot) -> list[str]:
        if self._sb is not None:
            texts: list[str] = []
            for selector in self.selectors.response_container_selectors:
                if selector == DEEPSEEK_CONVERSATION_BLOCKS_SELECTOR:
                    texts.extend(self._deepseek_visible_conversation_blocks())
                    continue
                elements = self._find_elements(selector)
                for element in elements:
                    if self._element_is_displayed(element):
                        text = normalize_plain_text(str(getattr(element, "text", "") or ""))
                        if text:
                            texts.append(text)
            if texts:
                return texts
        return snapshot.response_texts(self.selectors.response_container_selectors)

    def _find_elements(self, selector: str) -> list[Any]:
        if self._sb is None:
            return []
        for method_name in ["find_elements", "find_visible_elements"]:
            method = getattr(self._sb, method_name, None)
            if callable(method):
                with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                    result = method(selector)
                    return list(result or [])
        driver = getattr(self._sb, "driver", None)
        method = getattr(driver, "find_elements", None)
        if callable(method):
            with contextlib.suppress(Exception):
                return list(method("css selector", selector) or [])
        return []

    def _element_is_displayed(self, element: Any) -> bool:
        method = getattr(element, "is_displayed", None)
        if callable(method):
            with contextlib.suppress(Exception):
                return bool(method())
        return True

    def _execute_browser_script(self, script: str) -> Any:
        if self._sb is None:
            return None
        cdp = getattr(self._sb, "cdp", None)
        method = getattr(cdp, "evaluate", None)
        if callable(method):
            with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                return method(script)
        method = getattr(self._sb, "execute_script", None)
        if callable(method):
            with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                return method(script)
        driver = getattr(self._sb, "driver", None)
        method = getattr(driver, "execute_script", None)
        if callable(method):
            with contextlib.suppress(Exception):
                return method(script)
        return None

    def _deepseek_composer_action_available(self) -> bool:
        return bool(self._execute_browser_script(DEEPSEEK_COMPOSER_ACTION_AVAILABLE_SCRIPT))

    def _click_deepseek_composer_action(self) -> bool:
        return bool(self._execute_browser_script(DEEPSEEK_COMPOSER_ACTION_CLICK_SCRIPT))

    def _deepseek_visible_conversation_blocks(self) -> list[str]:
        value = self._execute_browser_script(DEEPSEEK_VISIBLE_CONVERSATION_BLOCKS_SCRIPT)
        if not isinstance(value, list):
            return []
        return [
            normalize_plain_text(str(item))
            for item in value
            if normalize_plain_text(str(item))
        ]

    def _handle_cookie_banner(self, snapshot: PageSnapshot | None = None) -> bool:
        snapshot = snapshot or self._snapshot()
        if snapshot.origin != self.expected_origin:
            return False
        banner_visible = self._any_visible(snapshot, self.selectors.cookie_banner_selectors)
        if not banner_visible:
            return False
        if self._click_first_enabled(self.selectors.cookie_reject_selectors):
            return True
        return self._click_first_enabled(self.selectors.cookie_accept_selectors)

    def _attempt_env_login(self) -> bool:
        email = os.environ.get("CATDESK_ADVISOR_DEEPSEEK_EMAIL", "")
        password = os.environ.get("CATDESK_ADVISOR_DEEPSEEK_PASSWORD", "")
        if not email or not password:
            return False
        snapshot = self._snapshot()
        if snapshot.origin != self.expected_origin:
            return False
        if snapshot.any_selector(self.selectors.takeover_required_selectors)[0]:
            self.state = AdapterState.TAKEOVER_REQUIRED
            return False
        email_selector = self._first_enabled_visible_selector(
            snapshot,
            self.selectors.login_email_selectors,
        )
        password_selector = self._first_enabled_visible_selector(
            snapshot,
            self.selectors.login_password_selectors,
        )
        if email_selector is None or password_selector is None:
            return False
        self._type_login_field(email_selector, email)
        self._type_login_field(password_selector, password)
        return self._click_first_enabled(self.selectors.login_submit_selectors)

    def _type_login_field(self, selector: str, value: str) -> None:
        if self._sb is None:
            return
        with contextlib.redirect_stdout(sys.stderr):
            if hasattr(self._sb, "click"):
                self._sb.click(selector)
            self._clear_prompt(selector)
        self._type_text_paced(selector, value)

    def _ensure_chat_selected(self, chat_name: str) -> bool:
        snapshot = self._snapshot()
        if snapshot.origin != self.expected_origin:
            return False
        if self._visible_text_exact(self.selectors.current_chat_title_selectors, chat_name):
            return True
        if not self._click_first_enabled(self.selectors.chat_menu_button_selectors):
            return False
        return self._click_exact_text(self.selectors.chat_item_selectors, chat_name)

    def _visible_text_exact(self, selectors: Iterable[str], expected: str) -> bool:
        expected_normalized = normalize_plain_text(expected)
        for selector in selectors:
            for element in self._find_elements(selector):
                if self._element_is_displayed(element):
                    text = normalize_plain_text(str(getattr(element, "text", "") or ""))
                    if text == expected_normalized:
                        return True
        return False

    def _click_exact_text(self, selectors: Iterable[str], expected: str) -> bool:
        expected_normalized = normalize_plain_text(expected)
        if self._sb is None:
            return False
        for selector in selectors:
            for element in self._find_elements(selector):
                if not self._element_is_displayed(element):
                    continue
                text = normalize_plain_text(str(getattr(element, "text", "") or ""))
                if text == expected_normalized:
                    with contextlib.redirect_stdout(sys.stderr):
                        element.click()
                    return True
        return False

    def _now(self) -> float:
        return time.monotonic()

    def _sleep(self, seconds: float) -> None:
        time.sleep(seconds)

    def advice_unavailable(self, request_id: str) -> AdviceResponseV1:
        status = STATE_TO_STATUS.get(self.state, AdvisorStatus.UNAVAILABLE)
        return AdviceResponseV1(
            schema_version=1,
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

    def advice_failure(self, request_id: str) -> AdviceResponseV1:
        return AdviceResponseV1(
            schema_version=SCHEMA_VERSION,
            request_id=request_id,
            advisor_id=self.advisor_id,
            status=AdvisorStatus.FAILED.value,
            diagnosis="DeepSeek web advisor failed during a bounded browser operation.",
            recommendations=[],
            risks=["No browser advice was safely obtained."],
            assumptions_or_questions=[],
            confidence="LOW",
            raw_artifact_reference=None,
        )


class JsonLinesAdvisorProtocol:
    def __init__(self, adapter: DeepSeekWebAdvisorAdapter, auth_token: str) -> None:
        if not auth_token:
            raise ValueError("auth_token must be non-empty")
        self.adapter = adapter
        self.auth_token = auth_token
        self._lock = threading.Lock()
        self._active_thread: threading.Thread | None = None
        self._active_request_id: str | None = None
        self._cancelled_request_ids: set[str] = set()

    def handle(
        self,
        message: dict[str, Any],
        emit: Any | None = None,
    ) -> dict[str, Any]:
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
            with self._lock:
                active = self._thread_is_active_locked()
                request_id = self._active_request_id
            state = self.adapter.state.value if active else self.adapter.refresh_state().value
            return {
                "ok": True,
                "state": state,
                "active": active,
                "request_id": request_id,
            }
        if command == "start":
            return {"ok": True, "state": self.adapter.start().value}
        if command == "cancel":
            with self._lock:
                request_id = self._active_request_id
                if request_id is None or not self._thread_is_active_locked():
                    return {"ok": False, "error": "NO_ACTIVE_ADVICE"}
                self._cancelled_request_ids.add(request_id)
            response = self.adapter.cancel(request_id)
            return {
                "ok": True,
                "state": self.adapter.state.value,
                "request_id": request_id,
                "response": response.to_dict(),
            }
        if command == "shutdown":
            with self._lock:
                request_id = self._active_request_id
                if request_id is not None:
                    self._cancelled_request_ids.add(request_id)
            response = self.adapter.cancel(request_id)
            self._join_active(timeout=5.0)
            self.adapter.stop()
            return {"ok": True, "state": self.adapter.state.value, "response": response.to_dict()}
        if command == "advise":
            request = message.get("request") or {}
            try:
                validated = validate_advice_request(request)
            except ValueError as exc:
                return {"ok": False, "error": str(exc)}
            with self._lock:
                if self._thread_is_active_locked():
                    return {
                        "ok": False,
                        "error": "ADVICE_ALREADY_ACTIVE",
                        "request_id": self._active_request_id,
                    }
                state = self.adapter.refresh_state()
                if state != AdapterState.READY:
                    return {
                        "ok": False,
                        "error": f"ADVISOR_NOT_READY:{state.value}",
                        "state": state.value,
                    }
                self._active_request_id = validated.request_id
                self._cancelled_request_ids.discard(validated.request_id)
                thread = threading.Thread(
                    target=self._run_advice_worker,
                    args=(request, validated.request_id, emit),
                    name=f"deepseek-advice-{validated.request_id}",
                    daemon=True,
                )
                self._active_thread = thread
                thread.start()
            return {
                "ok": True,
                "accepted": True,
                "request_id": validated.request_id,
                "state": self.adapter.state.value,
            }
        return {"ok": False, "error": "UNKNOWN_COMMAND"}

    def _thread_is_active_locked(self) -> bool:
        return self._active_thread is not None and self._active_thread.is_alive()

    def _join_active(self, timeout: float) -> None:
        with self._lock:
            thread = self._active_thread
        if thread is not None:
            thread.join(timeout=timeout)

    def _run_advice_worker(
        self,
        request: dict[str, Any],
        request_id: str,
        emit: Any | None,
    ) -> None:
        try:
            response = self.adapter.advise(request)
            event = {
                "ok": True,
                "event": "advice_completed",
                "request_id": request_id,
                "response": response.to_dict(),
            }
        except Exception as exc:
            event = {
                "ok": False,
                "event": "advice_failed",
                "request_id": request_id,
                "error": f"FAILED:{type(exc).__name__}",
            }
        finally:
            with self._lock:
                was_cancelled = request_id in self._cancelled_request_ids
                self._cancelled_request_ids.discard(request_id)
                self._active_request_id = None
                self._active_thread = None
        if was_cancelled:
            return
        if emit is not None:
            emit(event)

    def serve(self, input_stream: TextIO, output_stream: TextIO) -> None:
        output_lock = threading.Lock()

        def emit(event: dict[str, Any]) -> None:
            with output_lock:
                output_stream.write(json.dumps(event, separators=(",", ":")) + "\n")
                output_stream.flush()

        for line in input_stream:
            if not line.strip():
                continue
            try:
                message = json.loads(line)
                with contextlib.redirect_stdout(sys.stderr):
                    response = self.handle(message, emit)
            except ValueError as exc:
                response = {"ok": False, "error": str(exc)}
            except Exception as exc:
                response = {"ok": False, "error": f"FAILED:{type(exc).__name__}"}
            emit(response)
        self._join_active(timeout=5.0)


def contains_prohibited_authority(value: Any) -> str | None:
    if isinstance(value, dict):
        for key, nested in value.items():
            if normalize_key(str(key)) in {normalize_key(item) for item in PROHIBITED_AUTHORITY_KEYS}:
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


def call_first(target: Any, names: Iterable[str]) -> Any:
    for name in names:
        method = getattr(target, name, None)
        if callable(method):
            return method()
    return None


def normalize_plain_text(value: str) -> str:
    return re.sub(r"\s+", " ", value).strip()


def newest_new_response_text(
    response_texts: list[str],
    baseline_texts: list[str],
) -> str:
    normalized_baseline = [normalize_plain_text(text) for text in baseline_texts]
    normalized_responses = [normalize_plain_text(text) for text in response_texts]
    if len(normalized_responses) <= len(normalized_baseline):
        return ""
    newest = normalized_responses[-1]
    if not newest:
        return ""
    if newest in normalized_baseline:
        return ""
    return newest


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
    parser.add_argument(
        "--unsafe-headless-dev",
        action="store_true",
        help="Development only; manual login and security flows must not run headlessly.",
    )
    parser.add_argument(
        "--allow-env-login",
        action="store_true",
        help="Use CATDESK_ADVISOR_DEEPSEEK_EMAIL/PASSWORD for explicit opt-in login.",
    )
    parser.add_argument(
        "--chat-name",
        default=None,
        help="Optional exact chat name to select through configured chat selectors.",
    )
    args = parser.parse_args(argv)

    auth_token = os.environ.get("CATDESK_ADVISOR_AUTH_TOKEN", "")
    if not auth_token:
        sys.stderr.write("CATDESK_ADVISOR_AUTH_TOKEN is required\n")
        return 2
    selectors = SelectorConfig.from_file(args.selectors)
    adapter = DeepSeekWebAdvisorAdapter(
        selectors,
        args.profile_dir,
        headed=not args.unsafe_headless_dev,
        allow_env_login=args.allow_env_login,
        chat_name=args.chat_name,
    )
    JsonLinesAdvisorProtocol(adapter, auth_token).serve(sys.stdin, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
