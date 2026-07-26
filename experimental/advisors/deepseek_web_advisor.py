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
import re
import sys
import time
from pathlib import Path
from typing import Any, Iterable, TextIO
from urllib.parse import urlparse


SCHEMA_VERSION = 1
DEFAULT_MAX_REQUEST_BYTES = 24 * 1024
DEFAULT_MAX_PROMPT_BYTES = 12 * 1024
DEFAULT_MAX_RESPONSE_BYTES = 4 * 1024
REQUEST_ID_RE = re.compile(r"^[A-Za-z0-9_.:-]{1,120}$")


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
        if any(selector_matches(selector, tag.lower(), normalized_attrs) for selector in self.selectors):
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
    ) -> None:
        self.selectors = selectors
        self.profile_dir = profile_dir
        self.headed = headed
        self.state = AdapterState.STOPPED
        self._sb_context: Any = None
        self._sb: Any = None
        self.detector = CompletionDetector(selectors)
        self.cancel_requested = False

    def start(self) -> AdapterState:
        self.profile_dir.mkdir(parents=True, exist_ok=True)
        self.state = AdapterState.STARTING
        try:
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
            return self.refresh_state()
        except Exception as exc:
            sys.stderr.write(f"DeepSeek advisor startup failed: {type(exc).__name__}\n")
            self.state = AdapterState.DEGRADED
            return self.state

    def _import_seleniumbase(self) -> Any:
        from seleniumbase import SB  # type: ignore[import-not-found]

        return SB

    def stop(self) -> None:
        self.state = AdapterState.STOPPED
        if self._sb_context is not None:
            with contextlib.redirect_stdout(sys.stderr):
                self._sb_context.__exit__(None, None, None)
        self._sb_context = None
        self._sb = None
        self.cancel_requested = False

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
        takeover, _selector = snapshot.any_selector(self.selectors.takeover_required_selectors)
        if takeover:
            self.state = AdapterState.TAKEOVER_REQUIRED
            return self.state
        login_required, _selector = snapshot.any_selector(self.selectors.login_required_selectors)
        if login_required:
            self.state = AdapterState.LOGIN_REQUIRED
            return self.state
        prompt_ready, _selector = snapshot.any_selector(self.selectors.prompt_input_selectors)
        if prompt_ready:
            self.state = AdapterState.READY
            return self.state
        self.state = AdapterState.DEGRADED
        return self.state

    @property
    def expected_origin(self) -> str:
        parsed = urlparse(self.selectors.start_url)
        return f"{parsed.scheme}://{parsed.netloc}"

    def advise(self, request: dict[str, Any]) -> AdviceResponseV1:
        validated = validate_advice_request(request)
        if self.refresh_state() != AdapterState.READY:
            return self.advice_unavailable(validated.request_id)
        snapshot = self._snapshot()
        if snapshot.origin != self.expected_origin:
            self.state = AdapterState.DEGRADED
            return self.advice_unavailable(validated.request_id)

        prompt = build_advisory_prompt(validated)
        baseline_texts = snapshot.response_texts(self.selectors.response_container_selectors)
        baseline_count = len(baseline_texts)
        prompt_selector = self._find_prompt_selector(snapshot)
        if prompt_selector is None:
            self.state = AdapterState.DEGRADED
            return self.advice_unavailable(validated.request_id)

        self.cancel_requested = False
        self.state = AdapterState.SENDING
        self._type_prompt(prompt_selector, prompt)
        if not self._click_first(self.selectors.send_button_selectors):
            self._press_enter(prompt_selector)
        self.state = AdapterState.WAITING_FOR_RESPONSE
        return self._wait_for_response(validated, baseline_count)

    def cancel(self) -> AdviceResponseV1:
        self.cancel_requested = True
        if self.state in {AdapterState.SENDING, AdapterState.WAITING_FOR_RESPONSE}:
            self._click_first(self.selectors.stop_button_selectors)
        self.state = AdapterState.CANCELLED
        return AdviceResponseV1(
            schema_version=SCHEMA_VERSION,
            request_id="cancel",
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
        baseline_count: int,
    ) -> AdviceResponseV1:
        started_at = self._now()
        last_text = ""
        last_text_change_at = started_at
        while self._now() - started_at <= self.selectors.timeout_seconds:
            if self.cancel_requested:
                self._click_first(self.selectors.stop_button_selectors)
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
            page_text = snapshot.parsed.text.lower()
            if any(marker.lower() in page_text for marker in self.selectors.rate_limit_markers):
                self.state = AdapterState.RATE_LIMITED
                return self.advice_unavailable(request.request_id)

            response_texts = snapshot.response_texts(self.selectors.response_container_selectors)
            newest_text = response_texts[baseline_count] if len(response_texts) > baseline_count else ""
            newest_text = normalize_plain_text(newest_text)
            if newest_text != last_text:
                last_text = newest_text
                last_text_change_at = self._now()
            stable_for = self._now() - last_text_change_at
            stop_visible = self._any_visible(snapshot, self.selectors.stop_button_selectors)
            send_visible = self._any_visible(snapshot, self.selectors.send_button_selectors)
            marker_seen = any(
                marker.lower() in page_text for marker in self.selectors.completion_markers
            )

            if len(response_texts) > baseline_count and not newest_text and not stop_visible:
                self.state = AdapterState.FAILED
                return self.advice_unavailable(request.request_id)
            if (
                newest_text
                and not stop_visible
                and send_visible
                and marker_seen
                and stable_for >= self.selectors.text_stability_seconds
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
            if snapshot.selector_count(selector) > 0:
                return selector
        return None

    def _type_prompt(self, selector: str, prompt: str) -> None:
        if self._sb is None:
            raise RuntimeError("browser not started")
        with contextlib.redirect_stdout(sys.stderr):
            if hasattr(self._sb, "click"):
                self._sb.click(selector)
            if hasattr(self._sb, "press_keys"):
                self._sb.press_keys(selector, prompt)
            elif hasattr(self._sb, "type"):
                self._sb.type(selector, prompt)

    def _press_enter(self, selector: str) -> None:
        if self._sb is None:
            return
        with contextlib.redirect_stdout(sys.stderr):
            if hasattr(self._sb, "press_keys"):
                self._sb.press_keys(selector, "\n")

    def _click_first(self, selectors: Iterable[str]) -> bool:
        if self._sb is None:
            return False
        snapshot = self._snapshot()
        for selector in selectors:
            if snapshot.selector_count(selector) > 0 and hasattr(self._sb, "click"):
                with contextlib.redirect_stdout(sys.stderr):
                    self._sb.click(selector)
                return True
        return False

    def _any_visible(self, snapshot: PageSnapshot, selectors: Iterable[str]) -> bool:
        return any(snapshot.selector_count(selector) > 0 for selector in selectors)

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
            return {"ok": True, "state": self.adapter.refresh_state().value}
        if command == "start":
            return {"ok": True, "state": self.adapter.start().value}
        if command == "cancel":
            response = self.adapter.cancel()
            return {"ok": True, "state": self.adapter.state.value, "response": response.to_dict()}
        if command == "shutdown":
            self.adapter.stop()
            return {"ok": True, "state": self.adapter.state.value}
        if command == "advise":
            request = message.get("request") or {}
            try:
                response = self.adapter.advise(request)
            except ValueError as exc:
                return {"ok": False, "error": str(exc)}
            return {"ok": True, "response": response.to_dict()}
        return {"ok": False, "error": "UNKNOWN_COMMAND"}

    def serve(self, input_stream: TextIO, output_stream: TextIO) -> None:
        for line in input_stream:
            if not line.strip():
                continue
            try:
                message = json.loads(line)
                with contextlib.redirect_stdout(sys.stderr):
                    response = self.handle(message)
            except ValueError as exc:
                response = {"ok": False, "error": str(exc)}
            except Exception as exc:
                response = {"ok": False, "error": f"FAILED:{type(exc).__name__}"}
            output_stream.write(json.dumps(response, separators=(",", ":")) + "\n")
            output_stream.flush()


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
    )
    JsonLinesAdvisorProtocol(adapter, auth_token).serve(sys.stdin, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
