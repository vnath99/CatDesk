"""Use real Git worktrees to qualify read-only backup intake."""

import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


CHECKER = Path(__file__).resolve().parents[1] / "scripts/check-linked-worktree-backup.py"


class LinkedBackupTest(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix="catdesk-linked-backup-test-")
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        self.repo = self.root / "original"
        self.repo.mkdir()
        self.git("init", "-q")
        (self.repo / "tracked.txt").write_text("intake fixture\n", encoding="utf-8")
        self.git("add", "tracked.txt")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.test",
                 "-c", "commit.gpgsign=false", "commit", "-qm", "fixture")
        head = self.git("rev-parse", "HEAD").decode().strip()
        links = []
        for label in ("control", "registered-web", "assigned-web"):
            tree = self.root / label
            self.git("worktree", "add", "-q", "-b", label, str(tree))
            marker = tree / ".git"
            admin = Path(marker.read_text(encoding="utf-8").strip().removeprefix("gitdir: "))
            links.append({"label": label, "adminId": admin.name, "originalMarker": str(marker),
                          "expectedBranch": label, "expectedHead": head})
        self.manifest = self.root / "pins.json"
        self.manifest.write_text(json.dumps({"originalCommonGitDir": str(self.repo / ".git"),
                                             "links": links}), encoding="utf-8")
        self.stage = self.root / "staged" / ".git"
        shutil.copytree(self.repo / ".git", self.stage)

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args],
                                       stderr=subprocess.STDOUT, timeout=15)

    def intake(self, candidate=None):
        run = subprocess.run([sys.executable, "-I", "-B", str(CHECKER), "--candidate",
                              str(candidate or self.stage), "--manifest", str(self.manifest)],
                             capture_output=True, timeout=40)
        self.assertEqual(run.stderr, b"")
        row = json.loads(run.stdout)
        self.assertTrue(row["readOnly"])
        self.assertFalse(row["restoreAuthorized"])
        return run.returncode, row

    def stage_hashes(self):
        return {path.relative_to(self.stage): hashlib.sha256(path.read_bytes()).digest()
                for path in self.stage.rglob("*") if path.is_file()}

    def test_reads_all_original_links_without_writing(self):
        # The real recovery has neither the original store nor the working roots.
        (self.repo / ".git").rename(self.repo / "offline-original.git")
        for label in ("control", "registered-web", "assigned-web"):
            (self.root / label).rename(self.root / ("offline-" + label))
        prior = self.stage_hashes()
        code, row = self.intake()
        self.assertEqual(code, 0)
        self.assertEqual(row["state"], "STAGED_METADATA_READABLE")
        self.assertEqual(set(row["worktrees"]), {"control", "registered-web", "assigned-web"})
        self.assertEqual(self.stage_hashes(), prior)

    def test_refuses_changed_links_indexes_and_in_place_store(self):
        admin = self.stage / "worktrees/assigned-web"
        for name, payload, expected in (
            ("gitdir", str(self.root / "other/.git").encode() + b"\n", "WORKTREE_BACKLINK_MISMATCH"),
            ("commondir", b"../other/../..\n", "WORKTREE_COMMONDIR_UNVERIFIED"),
            ("HEAD", b"ref: refs/heads/control\n", "WORKTREE_BRANCH_MISMATCH"),
            ("index", b"invalid index", "GIT_METADATA_UNVERIFIED"),
        ):
            with self.subTest(metadata=name):
                path = admin / name
                original = path.read_bytes()
                path.write_bytes(payload)
                code, row = self.intake()
                self.assertEqual(code, 2)
                self.assertEqual(row["worktrees"]["assigned-web"], expected)
                path.write_bytes(original)
        (admin / "index").unlink()
        self.assertEqual(self.intake()[1]["worktrees"]["assigned-web"], "WORKTREE_INDEX_MISSING_OR_UNSAFE")
        self.assertEqual(self.intake(self.repo / ".git")[1]["state"], "SCRATCH_STORE_REQUIRED")
        (self.stage / "objects/info/alternates").write_text(str(self.repo / ".git/objects"), encoding="utf-8")
        self.assertEqual(self.intake()[1]["state"], "CANDIDATE_OBJECT_ALTERNATES_PRESENT")


if __name__ == "__main__":
    unittest.main()
