"""Read-only C0 projection of existing release records; never release authority.

No network, browser, registry initialization, promotion, recovery or output-file
writer. Paths are operator inputs and never appear in the JSON projection.
Existing doctor remains responsible for runtime/liveness diagnostics.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess


ARTIFACTS = {
    "CatDeskBinagotchy.exe": "binagotchySha256",
    "CatDeskWakeHost.exe": "hostSha256",
    "adapter.py": "adapterSha256",
    "wake_bridge.py": "wakeBridgeSha256",
}
HEX = re.compile(r"[a-f0-9]{64}\Z")
PACKAGE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,191}\Z")


def plain(path):
    path = Path(os.path.abspath(path))
    if os.name == "nt" and (not path.drive or str(path).startswith("\\\\")):
        raise ValueError("LOCAL_PATH_REQUIRED")
    for item in (*reversed(path.parents), path):
        info = item.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise ValueError("REPARSE_REFUSED")
    return path


def read(path, maximum=1048576):
    path = plain(path)
    if not stat.S_ISREG(path.stat().st_mode):
        raise ValueError("REGULAR_FILE_REQUIRED")
    with path.open("rb") as stream:
        data = stream.read(maximum + 1)
    if len(data) > maximum:
        raise ValueError("FILE_BOUND_EXCEEDED")
    return data


def digest(path):
    # Fixed bound; compiler images need not fit in memory.
    path = plain(path)
    if not stat.S_ISREG(path.stat().st_mode) or path.stat().st_size > 256 * 1024 * 1024:
        raise ValueError("IMAGE_BOUND_OR_TYPE_INVALID")
    result = hashlib.sha256()
    total = 0
    with path.open("rb") as stream:
        while chunk := stream.read(1048576):
            total += len(chunk)
            if total > 256 * 1024 * 1024:
                raise ValueError("IMAGE_BOUND_OR_TYPE_INVALID")
            result.update(chunk)
    return result.hexdigest()


def document(path):
    value = json.loads(read(path).decode("utf-8-sig"))
    if not isinstance(value, dict):
        raise ValueError("OBJECT_REQUIRED")
    return value


def observe(operation):
    try:
        return operation()
    except FileNotFoundError:
        return {"status": "NOT_INSTALLED", "reason": "REQUIRED_FILE_MISSING"}
    except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.SubprocessError):
        # No exception messages, filenames, arbitrary manifest fields or contents.
        return {"status": "FAIL", "reason": "EVIDENCE_INVALID_OR_UNAVAILABLE"}


def source_inventory(root):
    root = plain(root)

    def git(*args):
        result = subprocess.run(
            ["git", "--no-optional-locks", "-c", "core.fsmonitor=false", "-C", str(root), *args],
            capture_output=True, timeout=15, check=True,
        )
        if len(result.stdout) > 2 * 1024 * 1024:
            raise ValueError("GIT_OUTPUT_BOUND")
        return result.stdout.decode("utf-8").strip()

    head = git("rev-parse", "HEAD")
    if not re.fullmatch(r"[a-f0-9]{40}", head):
        raise ValueError("SOURCE_IDENTITY_INVALID")
    return {
        "status": "PASS", "classification": "SOURCE_ONLY", "sourceCommit": head,
        "dirty": bool(git("status", "--porcelain", "--untracked-files=normal", "--ignore-submodules")),
        "tests": "NOT_RUN", "installedParity": "NOT_INFERRED",
    }


def canonical_inventory(root):
    root = Path(root)
    actual = digest(root / "target/release/catdesk.exe")
    expected = read(root / "target/release/catdesk.exe.sha256", 1024).decode("ascii").strip().lower()
    pointer = document(root / ".catdesk/release-recovery/current.json")
    matches = bool(HEX.fullmatch(expected)) and actual == expected == pointer.get("sha256")
    return {
        "status": "PASS" if matches else "FAIL", "classification": "INSTALLED" if matches else "UNVERIFIED",
        "sha256": actual, "fingerprintMatches": actual == expected,
        "releasePointerMatches": actual == pointer.get("sha256"),
        "generation": pointer.get("generation") if type(pointer.get("generation")) is int else None,
        "runtimeHealth": "NOT_RUN", "promotionAuthority": "NOT_EVALUATED",
    }


def package_inventory(root, development=False):
    root = Path(root)
    authority = document(root / "authority.json") if development else None
    runtime = root / "runtime" if development else root
    pointer = document(runtime / "current.json")
    directory = pointer["directory"]
    if not isinstance(directory, str) or not PACKAGE.fullmatch(directory) or directory in (".", ".."):
        raise ValueError("PACKAGE_ID_INVALID")
    package = runtime / "versions" / directory
    manifest = document(package / "manifest.json")
    if manifest.get("schemaVersion") != 1 or not isinstance(manifest.get("artifacts"), dict):
        raise ValueError("MANIFEST_INVALID")
    rows = {}
    for name, field in ARTIFACTS.items():
        actual = digest(package / name)
        expected = pointer[field]
        if not isinstance(expected, str) or not HEX.fullmatch(expected):
            raise ValueError("DIGEST_INVALID")
        rows[name] = {"sha256": actual, "matches": actual == expected == manifest["artifacts"].get(name)}
    authority_matches = None
    if authority is not None:
        authority_matches = (
            authority.get("packageId") == directory
            and authority.get("manifestSha256") == digest(package / "manifest.json")
            and authority.get("pointerSha256") == digest(runtime / "current.json")
            and all(authority.get("artifacts", {}).get(n) == r["sha256"] for n, r in rows.items())
        )
    matches = all(row["matches"] for row in rows.values()) and authority_matches is not False
    adapter = read(package / "adapter.py")
    bridge = read(package / "wake_bridge.py")
    compatible = b"def bootstrap_attempt(" in adapter and b"class NewConversationSink(" in bridge
    version = manifest.get("version")
    if not isinstance(version, str) or not PACKAGE.fullmatch(version):
        raise ValueError("VERSION_INVALID")
    verification = None
    if development and (package / "verification.json").exists():
        record = document(package / "verification.json")
        commit = record.get("sourceCommit")
        verification = {
            "recordedSourceCommit": commit if isinstance(commit, str) and re.fullmatch(r"[a-f0-9]{40}", commit) else None,
            "matchesSelectedArtifacts": record.get("artifacts") == {n: r["sha256"] for n, r in rows.items()}
            and record.get("packageId") == directory
            and record.get("manifestSha256") == digest(package / "manifest.json"),
            "meaning": "RETAINED_VERIFICATION_RECORD_NOT_FRESH_HEALTH_OR_REVIEW_AUTHORITY",
        }
    return {
        "status": "PASS" if matches else "FAIL",
        "classification": ("SELECTED" if development else "INSTALLED") if matches else "UNVERIFIED",
        "packageId": directory, "version": version, "artifacts": rows,
        "authorityChecksumMatches": authority_matches,
        "authorityPhysicalIdentity": "NOT_EVALUATED",
        "verificationRecord": verification,
        "ordinarySenderBootstrapProtocolPresent": compatible,
        "browserReadiness": "NOT_RUN", "liveAcceptance": "NOT_RUN",
        "meaning": "CHECKSUM_INVENTORY_ONLY_NOT_RELEASE_OR_EXECUTION_AUTHORITY",
    }


def build_inventory(evidence, current_commit):
    record = document(evidence)
    commit = record["sourceCommit"]
    controller = record["controller"]
    if not isinstance(commit, str) or not re.fullmatch(r"[a-f0-9]{40}", commit):
        raise ValueError("BUILD_SOURCE_INVALID")
    expected = controller["sha256"]
    if not isinstance(expected, str) or not HEX.fullmatch(expected):
        raise ValueError("BUILD_DIGEST_INVALID")
    actual = digest(controller["path"])
    matches = record.get("passed") is True and expected == actual
    return {
        "status": "PASS" if matches else "FAIL",
        "classification": "BUILT" if matches else "UNVERIFIED",
        "recordedSourceCommit": commit, "sha256": actual,
        "currentHeadMatchesBuildSource": commit == current_commit,
        "meaning": "RETAINED_BUILD_RECEIPT_AND_CURRENT_BYTES_NOT_INDEPENDENT_PROVENANCE_OR_PROMOTION_AUTHORITY",
    }


def test_inventory(source, evidence, current_commit):
    if not isinstance(current_commit, str) or not re.fullmatch(r"[a-f0-9]{40}", current_commit):
        raise ValueError("CURRENT_SOURCE_REQUIRED")
    record = document(evidence)
    inputs = record["inputHashes"]
    if not isinstance(inputs, dict) or not 1 <= len(inputs) <= 256:
        raise ValueError("TEST_SCOPE_INVALID")
    for name, expected in inputs.items():
        if (not isinstance(name, str) or len(name) > 512 or name.startswith(('/', '-'))
                or '\\' in name or ':' in name or any(p in ('', '.', '..') for p in name.split('/'))
                or not isinstance(expected, str) or not HEX.fullmatch(expected)):
            raise ValueError("TEST_SCOPE_INVALID")
        if digest(Path(source) / name) != expected:
            return {"status": "FAIL", "reason": "TEST_INPUTS_CHANGED"}
    matches = (record.get("sourceCommit") == current_commit and record.get("exitCode") == 0
               and type(record.get("testsRun")) is int and record["testsRun"] > 0)
    return {
        "status": "PASS" if matches else "FAIL",
        "classification": "TESTED_SOURCE" if matches else "UNVERIFIED",
        "reason": "DECLARED_INPUTS_MATCH" if matches else "STALE_OR_FAILED_TEST_EVIDENCE",
        "declaredInputCount": len(inputs), "coverage": "DECLARED_INPUTS_ONLY_NOT_WHOLE_APPLICATION",
        "reviewAuthority": False,
    }


def inventory(source, canonical, development, wake, build_evidence=None, test_evidence=None):
    source_result = observe(lambda: source_inventory(source))
    return {
        "schemaVersion": 1, "gate": "C0", "observedUtc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "readOnly": True, "samplesAtomic": False, "source": source_result,
        "build": observe(lambda: build_inventory(build_evidence, source_result.get("sourceCommit"))) if build_evidence else {"status": "NOT_RUN"},
        "sourceTests": observe(lambda: test_inventory(source, test_evidence, source_result.get("sourceCommit"))) if test_evidence else {"status": "NOT_RUN"},
        "canonical": observe(lambda: canonical_inventory(canonical)),
        "selectedDevelopment": observe(lambda: package_inventory(development, True)),
        "installedWake": observe(lambda: package_inventory(wake)),
        "runtimeAndConnector": "USE_EXISTING_DOCTOR_AND_BOUNDED_OWNER_EVIDENCE",
        "c1": "NOT_RUN", "externalActions": 0,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source", "canonical", "development", "wake"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--build-evidence", type=Path)
    parser.add_argument("--test-evidence", type=Path)
    args = parser.parse_args()
    print(json.dumps(inventory(args.source, args.canonical, args.development, args.wake, args.build_evidence, args.test_evidence), indent=2))
