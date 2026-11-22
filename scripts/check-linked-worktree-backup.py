"""Read-only intake check for an original Git store staged in a scratch directory."""

import argparse
import json
import os
from pathlib import Path
import re
import stat
import subprocess


class Refusal(Exception):
    """A stable, path-free refusal code."""


def path_key(path):
    spelling = os.fspath(path)
    if os.name == "nt" and spelling.startswith("\\\\?\\"):
        spelling = spelling[4:]
    return os.path.normcase(os.path.normpath(spelling))


def plain(path, directory, error):
    try:
        for parent in reversed(path.parents):
            check_type(parent, True)
        check_type(path, directory)
    except (OSError, ValueError):
        raise Refusal(error) from None


def check_type(path, directory):
    metadata = path.lstat()
    if (getattr(metadata, "st_file_attributes", 0) & 0x400
            or not (stat.S_ISDIR if directory else stat.S_ISREG)(metadata.st_mode)):
        raise ValueError("unsafe path type")


def read_line(path, error):
    plain(path, False, error)
    try:
        with path.open("rb") as stream:
            value = stream.read(4097).decode("utf-8").rstrip("\r\n")
        if not value or len(value) > 4096 or any(c in value for c in "\r\n\0"):
            raise ValueError("invalid metadata line")
        return value
    except (OSError, ValueError):
        raise Refusal(error) from None


def read_git(admin, *args, capture=False):
    environment = {key: value for key, value in os.environ.items()
                   if not key.upper().startswith("GIT_")}
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_NO_LAZY_FETCH="1", GIT_TERMINAL_PROMPT="0")
    try:
        return subprocess.run(
            ["git", "--no-optional-locks", "--no-lazy-fetch", "--no-replace-objects",
             f"--git-dir={admin}", *args],
            cwd=admin, env=environment, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, check=True, timeout=15,
        ).stdout
    except (OSError, subprocess.SubprocessError):
        raise Refusal("GIT_METADATA_UNVERIFIED") from None


def validate_link(candidate, link):
    admin = candidate / "worktrees" / link["adminId"]
    plain(admin, True, "WORKTREE_ADMIN_MISSING_OR_UNSAFE")
    commondir = Path(read_line(admin / "commondir", "WORKTREE_COMMONDIR_UNVERIFIED"))
    if commondir.parts != ("..", ".."):
        raise Refusal("WORKTREE_COMMONDIR_UNVERIFIED")
    backlink = Path(read_line(admin / "gitdir", "WORKTREE_BACKLINK_UNVERIFIED"))
    if (not backlink.is_absolute() or ".." in backlink.parts
            or path_key(backlink) != path_key(link["originalMarker"])):
        raise Refusal("WORKTREE_BACKLINK_MISMATCH")
    plain(admin / "index", False, "WORKTREE_INDEX_MISSING_OR_UNSAFE")
    head = read_line(admin / "HEAD", "WORKTREE_HEAD_UNVERIFIED")
    if "expectedBranch" in link and head != f"ref: refs/heads/{link['expectedBranch']}":
        raise Refusal("WORKTREE_BRANCH_MISMATCH")
    resolved = read_git(admin, "rev-parse", "--verify", "HEAD^{commit}", capture=True).strip()
    if not re.fullmatch(rb"[0-9a-f]{40}|[0-9a-f]{64}", resolved):
        raise Refusal("WORKTREE_HEAD_UNVERIFIED")
    if "expectedHead" in link and resolved != link["expectedHead"].encode("ascii"):
        raise Refusal("WORKTREE_HEAD_MISMATCH")
    # This exercises the retained index without contacting the original working tree.
    read_git(admin, "ls-files", "--stage")


def validate_manifest(manifest):
    original = Path(manifest["originalCommonGitDir"])
    links = manifest["links"]
    if not original.is_absolute() or not isinstance(links, list) or not 1 <= len(links) <= 16:
        raise ValueError("invalid manifest")
    ids = set()
    labels = set()
    for link in links:
        admin_id = link["adminId"]
        label = link["label"]
        marker = Path(link["originalMarker"])
        if (not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", admin_id)
                or admin_id.endswith(".") or admin_id.casefold() in ids
                or not re.fullmatch(r"[a-z][a-z0-9-]{0,63}", label) or label in labels
                or not marker.is_absolute() or marker.name != ".git"):
            raise ValueError("invalid link")
        if "expectedHead" in link and not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", link["expectedHead"]):
            raise ValueError("invalid pinned head")
        ids.add(admin_id.casefold())
        labels.add(label)
    return original, links


def check(candidate, manifest):
    original, links = validate_manifest(manifest)
    if not candidate.is_absolute() or ".." in candidate.parts:
        raise Refusal("ABSOLUTE_SCRATCH_STORE_REQUIRED")
    if path_key(candidate) == path_key(original):
        raise Refusal("SCRATCH_STORE_REQUIRED")
    plain(candidate, True, "CANDIDATE_STORE_MISSING_OR_UNSAFE")
    plain(candidate / "objects", True, "CANDIDATE_OBJECT_STORE_MISSING_OR_UNSAFE")
    # Keep the check inside the staged store. Alternates can consult a different
    # repository, so they require a separate, independently verified restoration.
    for alternate in ("alternates", "http-alternates"):
        try:
            (candidate / "objects/info" / alternate).lstat()
        except FileNotFoundError:
            continue
        raise Refusal("CANDIDATE_OBJECT_ALTERNATES_PRESENT")
    statuses = {}
    for link in links:
        try:
            validate_link(candidate, link)
        except Refusal as error:
            statuses[link["label"]] = str(error)
        else:
            statuses[link["label"]] = "LINKED_ADMIN_AND_GIT_INDEX_READABLE"
    return statuses


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()
    result = {"readOnly": True, "restoreAuthorized": False}
    try:
        plain(args.manifest.absolute(), False, "MANIFEST_MISSING_OR_UNSAFE")
        with args.manifest.open("rb") as stream:
            payload = stream.read(64 * 1024 + 1)
        if len(payload) > 64 * 1024:
            raise ValueError("oversized manifest")
        statuses = check(args.candidate, json.loads(payload))
        passed = all(state == "LINKED_ADMIN_AND_GIT_INDEX_READABLE" for state in statuses.values())
        result.update(state="STAGED_METADATA_READABLE" if passed else "STAGED_METADATA_INCOMPLETE",
                      worktrees=statuses)
    except Refusal as error:
        passed = False
        result["state"] = str(error)
    except (OSError, TypeError, ValueError, KeyError):
        passed = False
        result["state"] = "BACKUP_OR_MANIFEST_UNAVAILABLE"
    print(json.dumps(result, separators=(",", ":")))
    raise SystemExit(0 if passed else 2)


if __name__ == "__main__":
    main()
