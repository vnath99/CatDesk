#!/usr/bin/env python3
"""Small offline continuity checks and task-completion updates. No runtime API.

The five Markdown files remain the records. Events are caller-supplied audit
evidence, not new task grants or release authority. Git applies checked patches;
this tool does not stage, commit, push, start processes or touch runtime state.
"""
import argparse
import datetime
import difflib
import hashlib
import json
import pathlib
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit

PRIMARY = (
    "CATDESK_PROJECT_HANDOFF.md", "CATDESK_MILESTONES.md",
    "CATDESK_IMPORTANT_COMMANDS.md", "CATDESK_ACTIVITY_LOG.md", "CATDESK_TICKETS.md",
)
TICKET_STATUSES = {"BLOCKED", "OPEN", "IN_PROGRESS", "IN_REVIEW", "DONE", "CANCELLED"}
MILESTONE_STATUSES = {"PROPOSED", "PLANNED", "ACTIVE", "BLOCKED", "DEFERRED", "COMPLETED_VERIFIED", "CANCELLED"}
MATURITIES = {"SOURCE_TESTED", "CONTROLLED_LIVE", "INSTALLED_VERIFIED", "UNATTENDED_VERIFIED", "UNVERIFIED"}
ROOT = pathlib.Path(__file__).resolve().parents[1]


def git(root, *args, data=None):
    p = subprocess.run(["git", *args], cwd=root, input=data, capture_output=True, timeout=30)
    if p.returncode:
        raise ValueError("GIT_OPERATION_REFUSED: " + args[0])
    return p.stdout


def text_without_code(text):
    text = re.sub(r"(?ms)^```.*?^```\s*$", "", text)
    return re.sub(r"`[^`\n]+`", "", text)


def anchors(text):
    seen = {}; result = set(re.findall(r'<a id="([^"]+)"', text))
    for title in re.findall(r"(?m)^#{1,6}\s+(.+)$", text):
        slug = re.sub(r"[^\w\- ]", "", title.lower()).replace(" ", "-")
        count = seen.get(slug, 0); seen[slug] = count + 1
        result.add(slug + ("-" + str(count) if count else ""))
    return result


def table(text, prefix):
    return [[x.strip() for x in line.strip().strip("|").split("|")]
            for line in text.splitlines() if line.startswith("| " + prefix)]


def machine_handoff(text):
    blocks = re.findall(r"```json\n(.*?)\n```", text, re.S)
    if len(blocks) != 1:
        raise ValueError("HANDOFF_MACHINE_SECTION_AMBIGUOUS")
    return json.loads(blocks[0])


def inspect(root):
    errors = []; checked_links = 0
    for p in PRIMARY:
        if not (root / p).is_file(): errors.append("MISSING_PRIMARY:" + p)
    if errors: return {"passed": False, "errors": errors}
    docs = {p: (root / p).read_text(encoding="utf-8") for p in PRIMARY}
    readme = root / "README.md"
    if not readme.is_file() or any(f"]({p})" not in readme.read_text(encoding="utf-8") for p in PRIMARY):
        errors.append("README_PRIMARY_AUTHORITY_LINKS_MISSING")
    for name in ("CATDESK_PROJECT_HANDOFF.md", "CATDESK_IMPORTANT_COMMANDS.md"):
        prior = root / "docs/orchestrator" / name
        if not prior.is_file() or "SUPERSEDED AS CURRENT AUTHORITY" not in prior.read_text(encoding="utf-8")[:500]:
            errors.append("LEGACY_AUTHORITY_CONFLICT:" + name)
    for sha in set(re.findall(r"(?<![0-9a-f])[0-9a-f]{40}(?![0-9a-f])", docs[PRIMARY[0]])):
        try: git(root, "cat-file", "-e", sha + "^{commit}")
        except ValueError: errors.append("HANDOFF_COMMIT_UNRESOLVED:" + sha)
    tickets = table(docs[PRIMARY[4]], "")
    tickets = [r for r in tickets if r[0] not in {"ID", "---"}]
    milestones = table(docs[PRIMARY[1]], "M-")
    ids = [r[0] for r in tickets]; mids = [r[0] for r in milestones]
    if len(ids) != len(set(ids)): errors.append("DUPLICATE_TICKET_ID")
    if len(mids) != len(set(mids)): errors.append("DUPLICATE_MILESTONE_ID")
    for r in tickets:
        if len(r) != 9: errors.append("TICKET_COLUMNS:" + r[0]); continue
        if r[2] not in mids: errors.append("MISSING_MILESTONE:" + r[0])
        if r[4] not in TICKET_STATUSES: errors.append("TICKET_STATUS:" + r[0])
        if r[5] not in MATURITIES: errors.append("TICKET_MATURITY:" + r[0])
    for r in milestones:
        if len(r) != 7: errors.append("MILESTONE_COLUMNS:" + r[0]); continue
        if r[3] not in MILESTONE_STATUSES: errors.append("MILESTONE_STATUS:" + r[0])
        if r[4] not in MATURITIES: errors.append("MILESTONE_MATURITY:" + r[0])
        if r[3] == "ACTIVE":
            for tid in r[5].split(";"):
                if tid.strip() not in ids: errors.append("ACTIVE_MILESTONE_TICKET:" + tid.strip())
    try:
        handoff = machine_handoff(docs[PRIMARY[0]])
        if handoff["ticket"] not in ids or handoff["milestone"] not in mids:
            errors.append("HANDOFF_UNKNOWN_TASK")
        actual = next(r for r in tickets if r[0] == handoff["ticket"])
        if actual[4] != handoff["status"] or actual[2] != handoff["milestone"]:
            errors.append("HANDOFF_TICKET_CONFLICT")
        if handoff["runtime_status"] not in {"UNVERIFIED", "VERIFIED_AT_RECORDED_TIME"}:
            errors.append("HANDOFF_RUNTIME_STATUS")
        sha = handoff["source_sha"]
        if not re.fullmatch(r"[0-9a-f]{40}", sha): errors.append("SOURCE_SHA_FORMAT")
        else: git(root, "cat-file", "-e", sha + "^{commit}")
    except (ValueError, KeyError, StopIteration): errors.append("HANDOFF_IDENTITY_UNPROVEN")
    paths = list(PRIMARY) + ["docs/archive/PRE_CONSOLIDATION_2026_11_14.md",
        "docs/activity/2026-11.md", "docs/verification/REPOSITORY_RECONCILIATION_2026_11_14.md",
        "docs/orchestrator/tickets/DOC_CONTINUITY_2026_11_14.md"]
    for name in paths:
        p = root / name
        if not p.is_file(): errors.append("MISSING_SUPPORTING:" + name); continue
        body = p.read_text(encoding="utf-8")
        for target in re.findall(r"\]\(([^)]+)\)", text_without_code(body)):
            target = target.strip("<>")
            if urlsplit(target).scheme: continue
            url = urlsplit(target); dest = (p.parent / unquote(url.path)).resolve() if url.path else p
            try: dest.relative_to(root.resolve())
            except ValueError: errors.append("LINK_OUTSIDE_REPOSITORY:" + name); continue
            if not dest.exists(): errors.append("MISSING_LINK:" + name + ":" + target)
            elif url.fragment and dest.is_file() and dest.suffix == ".md":
                if unquote(url.fragment) not in anchors(dest.read_text(encoding="utf-8")):
                    errors.append("MISSING_ANCHOR:" + name + ":" + target)
            checked_links += 1
        if re.search(r"https://chatgpt\.com/(?:c|g)/|[A-Z]:[\\/]Users[\\/]|-----BEGIN [A-Z ]*PRIVATE KEY-----|\bgh[pousr]_[A-Za-z0-9]{20,}|\bsk-[A-Za-z0-9_-]{24,}", body):
            errors.append("POTENTIALLY_PRIVATE_CONTENT:" + name)
    return {"passed": not errors, "primaryDocuments": len(PRIMARY), "tickets": len(ids),
        "milestones": len(mids), "internalLinksChecked": checked_links, "errors": sorted(set(errors)),
        "scope": "FIVE_PRIMARY_AND_NEW_SUPPORTING_DOCS; LEGACY_LINKS_GRANDFATHERED_NOT_SILENTLY_FIXED"}


def safe_text(value, limit=1200):
    if not isinstance(value, str) or not value or len(value) > limit or any(x in value for x in ("\n", "\r", "|", "`", "<!--", "-->", "[", "]")):
        raise ValueError("UNSAFE_OR_OVERSIZED_EVENT_TEXT")
    if re.search(r"chatgpt\.com/(c|g)/|[A-Z]:[\\/]Users[\\/]|Bearer\s|PRIVATE KEY|\bsk-", value):
        raise ValueError("PRIVATE_EVENT_TEXT_REFUSED")
    return value


def record(root, event_path, apply=False):
    raw = event_path.read_bytes()
    if len(raw) > 16384: raise ValueError("EVENT_TOO_LARGE")
    event = json.loads(raw); digest = hashlib.sha256(raw).hexdigest()
    required = {"event_id", "timestamp_utc", "agent", "ticket", "milestone", "source_sha",
                "status", "milestone_status", "maturity", "outcome", "summary", "next_action", "evidence"}
    if set(event) != required: raise ValueError("EVENT_SCHEMA_MISMATCH")
    eid = event["event_id"]
    if not re.fullmatch(r"[a-zA-Z0-9_-]{1,80}", eid): raise ValueError("EVENT_ID_INVALID")
    when = datetime.datetime.fromisoformat(event["timestamp_utc"].replace("Z", "+00:00"))
    if when.utcoffset() != datetime.timedelta(0): raise ValueError("UTC_TIMESTAMP_REQUIRED")
    for key in ["agent", "ticket", "milestone", "summary", "next_action"]: safe_text(event[key])
    if event["status"] not in TICKET_STATUSES or event["milestone_status"] not in MILESTONE_STATUSES or event["maturity"] not in MATURITIES:
        raise ValueError("EVENT_STATUS_INVALID")
    if event["status"] in {"DONE", "CANCELLED"} or event["milestone_status"] in {"COMPLETED_VERIFIED", "CANCELLED"}:
        raise ValueError("ACCEPTANCE_TRANSITION_REQUIRES_SEPARATE_REVIEW")
    if event["outcome"] not in {"success", "partial", "blocked", "failed", "inconclusive"}: raise ValueError("OUTCOME_INVALID")
    current_source = git(root, "rev-parse", "HEAD").decode().strip() == event["source_sha"]
    if not isinstance(event["evidence"], list) or not 1 <= len(event["evidence"]) <= 12: raise ValueError("EVIDENCE_REQUIRED")
    references = []
    for evidence in event["evidence"]:
        if set(evidence) != {"path", "sha256", "command", "exit_status"}: raise ValueError("EVIDENCE_SCHEMA_INVALID")
        path = pathlib.Path(evidence["path"])
        resolved = (root / path).resolve()
        if path.is_absolute() or resolved == root.resolve() or not resolved.is_relative_to(root.resolve()) or path.parts[0] in {".git", ".catdesk"}:
            raise ValueError("EVIDENCE_OUTSIDE_APPROVED_SOURCE")
        if any(part in {".git", ".catdesk"} for part in resolved.relative_to(root.resolve()).parts):
            raise ValueError("EVIDENCE_OUTSIDE_APPROVED_SOURCE")
        if not resolved.is_file() or resolved.is_symlink() or hashlib.sha256(resolved.read_bytes()).hexdigest() != evidence["sha256"]:
            raise ValueError("EVIDENCE_IDENTITY_MISMATCH")
        command = safe_text(evidence["command"])
        if type(evidence["exit_status"]) is not int or not 0 <= evidence["exit_status"] <= 255: raise ValueError("EVIDENCE_EXIT_INVALID")
        references.append(f'[{path.as_posix()}]({path.as_posix()}); command: {command}; exit {evidence["exit_status"]}; SHA-256 {evidence["sha256"]}')
    documents = {p: (root / p).read_text(encoding="utf-8") for p in PRIMARY}
    marker = f'<!-- catdesk-activity:{eid} sha256:{digest} -->'
    activity = documents[PRIMARY[3]]
    if f'<!-- catdesk-activity:{eid} ' in activity:
        if marker not in activity: raise ValueError("EVENT_ID_REUSED_WITH_CHANGED_PAYLOAD")
        if not re.fullmatch(r"[a-f0-9]{40}", event["source_sha"]): raise ValueError("EVENT_SOURCE_NOT_CURRENT_HEAD")
        # A publication commit advances HEAD after recording. A no-write exact
        # repeat is still legitimate when its original source is an ancestor.
        git(root, "merge-base", "--is-ancestor", event["source_sha"], "HEAD")
        checked = inspect(root)
        current = machine_handoff(documents[PRIMARY[0]])
        matches = table(documents[PRIMARY[4]], event["ticket"] + " ")
        mile = table(documents[PRIMARY[1]], event["milestone"] + " ")
        if not checked["passed"] or current["status"] != event["status"] or current["next_action"] != event["next_action"] or len(matches) != 1 or matches[0][5] != event["maturity"] or event["summary"] not in matches[0][7] or len(mile) != 1 or mile[0][3] != event["milestone_status"]:
            raise ValueError("PARTIAL_EVENT_UPDATE_REQUIRES_RECONCILIATION")
        return {"state": "ALREADY_RECORDED", "writes": 0, "event_id": eid}
    if not current_source: raise ValueError("EVENT_SOURCE_NOT_CURRENT_HEAD")
    validation = inspect(root)
    if not validation["passed"]: raise ValueError("CURRENT_DOC_STATE_INVALID")
    handoff = machine_handoff(documents[PRIMARY[0]])
    if handoff["ticket"] != event["ticket"] or handoff["milestone"] != event["milestone"]:
        raise ValueError("EVENT_NOT_CURRENT_HANDOFF_TASK")
    updated = dict(documents)
    handoff.update(status=event["status"],next_action=event["next_action"])
    updated[PRIMARY[0]] = re.sub(r"```json\n.*?\n```", '```json\n' + json.dumps(handoff, separators=(",", ":")) + '\n```', updated[PRIMARY[0]], count=1, flags=re.S)
    rows = table(updated[PRIMARY[4]], event["ticket"] + " ")
    if len(rows) != 1 or rows[0][2] != event["milestone"]: raise ValueError("TICKET_IDENTITY_AMBIGUOUS")
    row = rows[0]; old = "| " + " | ".join(row) + " |"
    if event["maturity"] not in {"UNVERIFIED", "SOURCE_TESTED", row[5]}:
        raise ValueError("LIVE_ACCEPTANCE_TRANSITION_REQUIRES_SEPARATE_REVIEW")
    row[4] = event["status"]; row[5] = event["maturity"]; row[7] = event["summary"] + " / next: " + event["next_action"]
    if updated[PRIMARY[4]].count(old) != 1: raise ValueError("TICKET_PREIMAGE_CHANGED")
    updated[PRIMARY[4]] = updated[PRIMARY[4]].replace(old, "| " + " | ".join(row) + " |", 1)
    rows = table(updated[PRIMARY[1]], event["milestone"] + " ")
    if len(rows) != 1: raise ValueError("MILESTONE_IDENTITY_AMBIGUOUS")
    row = rows[0]; old = "| " + " | ".join(row) + " |"; row[3] = event["milestone_status"]
    row[6] = row[6].split(" <br> Latest task update:", 1)[0] + " <br> Latest task update: " + event["summary"] + "; next: " + event["next_action"]
    if updated[PRIMARY[1]].count(old) != 1: raise ValueError("MILESTONE_PREIMAGE_CHANGED")
    updated[PRIMARY[1]] = updated[PRIMARY[1]].replace(old, "| " + " | ".join(row) + " |", 1)
    updated[PRIMARY[3]] += f'\n{marker}\n\n## {event["timestamp_utc"]} — {event["ticket"]}\n\nAgent: {event["agent"]}. Source: {event["source_sha"]}. Outcome: {event["outcome"]}. {event["summary"]}. Next: {event["next_action"]}.\n\nEvidence: ' + "; ".join(references) + '.\n'
    patch = "".join("".join(difflib.unified_diff(documents[p].splitlines(True), updated[p].splitlines(True), fromfile="a/"+p, tofile="b/"+p)) for p in PRIMARY)
    git(root, "apply", "--check", "--whitespace=error", "-", data=patch.encode())
    if apply:
        # Git checks all patch preimages before writing. Never use --reject,
        # forced replacement or an automatic conflict-resolution fallback.
        git(root, "apply", "--whitespace=error", "-", data=patch.encode())
    return {"state": "RECORDED" if apply else "VALIDATED_PREVIEW", "event_id": eid,
            "event_sha256": digest, "changedFiles": [p for p in PRIMARY if documents[p] != updated[p]],
            "acceptanceAuthority": False, "runtimeActions": 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "record"))
    parser.add_argument("--root", type=pathlib.Path, default=ROOT)
    parser.add_argument("--event", type=pathlib.Path)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    try:
        if args.command == "check":
            result = inspect(args.root.resolve()); code = 0 if result["passed"] else 1
        else:
            if args.event is None: raise ValueError("EVENT_PATH_REQUIRED")
            result = record(args.root.resolve(), args.event, args.apply); code = 0
    except (ValueError, OSError, subprocess.SubprocessError, json.JSONDecodeError) as e:
        # Fixed categories only; never leak a private path/event/command body.
        category = str(e) if isinstance(e, ValueError) and re.fullmatch(r"[A-Z_]+(?:: [a-z-]+)?",str(e)) else type(e).__name__
        result = {"state": "REFUSED", "error": category, "runtimeActions": 0}; code = 1
    print(json.dumps(result,sort_keys=True)); return code


if __name__ == "__main__":
    sys.exit(main())
