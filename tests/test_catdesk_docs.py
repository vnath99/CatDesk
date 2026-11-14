import copy
import hashlib
import importlib.util
import json
import pathlib
import os
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("catdesk_docs", ROOT / "scripts/catdesk_docs.py")
docs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(docs)


class ContinuityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix="catdesk-docs-test-")
        cls.root = pathlib.Path(cls.temp.name) / "source"
        subprocess.run(["git", "clone", "--quiet", "--shared", str(ROOT), str(cls.root)], check=True, capture_output=True, timeout=45)
        paths = subprocess.run(["git", "diff", "--name-only", "HEAD"], cwd=ROOT, check=True, capture_output=True).stdout.decode().splitlines()
        paths += subprocess.run(["git", "ls-files", "--others", "--exclude-standard"], cwd=ROOT, check=True, capture_output=True).stdout.decode().splitlines()
        for name in paths:
            if name.startswith("tests/") or name == "scripts/catdesk_docs.py": continue
            p = ROOT / name
            if p.is_file():
                target = cls.root / name; target.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(p, target)
        subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "add", "--", *docs.PRIMARY, "docs", "README.md", ".gitattributes"], cwd=cls.root, check=True, capture_output=True)
        subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-qm", "Isolated documentation fixture"], cwd=cls.root, check=True, capture_output=True)
        cls.baseline = {p: (cls.root / p).read_bytes() for p in docs.PRIMARY}
        cls.head = docs.git(cls.root, "rev-parse", "HEAD").decode().strip()

    @classmethod
    def tearDownClass(cls):
        # Only this newly allocated temporary tree is eligible for cleanup.
        assert pathlib.Path(cls.temp.name).resolve().name.startswith("catdesk-docs-test-")
        cls.temp.cleanup()

    def setUp(self):
        for p, raw in self.baseline.items(): (self.root / p).write_bytes(raw)
        self.event_path = pathlib.Path(self.temp.name) / "event.json"
        evidence = "docs/verification/WIDGET_RESOURCE_DIAGNOSTIC_2026_11_14.json"
        self.event = dict(event_id="test-progress", timestamp_utc="2026-11-14T17:00:00Z", agent="Fixture",
            ticket="DOC-CONTINUITY", milestone="M-CONTINUITY", source_sha=self.head,
            status="IN_PROGRESS", milestone_status="ACTIVE", maturity="SOURCE_TESTED", outcome="partial",
            summary="Mechanical source checks passed; installed acceptance unchanged", next_action="Review documentation and verify publication",
            evidence=[dict(path=evidence, sha256=hashlib.sha256((self.root / evidence).read_bytes()).hexdigest(), command="readonly resource observation", exit_status=0)])

    def write_event(self):
        self.event_path.write_text(json.dumps(self.event), encoding="utf-8")

    def test_complete_documents_and_references(self):
        result = docs.inspect(self.root)
        self.assertTrue(result["passed"], result)
        self.assertGreater(result["tickets"], 430)

    def test_preview_writes_nothing(self):
        self.write_event(); r = docs.record(self.root, self.event_path)
        self.assertEqual(r["state"], "VALIDATED_PREVIEW")
        self.assertEqual(self.baseline, {p:(self.root/p).read_bytes() for p in docs.PRIMARY})

    def test_apply_updates_four_documents_and_exact_repeat_is_noop(self):
        self.write_event(); result = docs.record(self.root, self.event_path, True)
        self.assertEqual(len(result["changedFiles"]), 4)
        self.assertTrue(docs.inspect(self.root)["passed"])
        before = {p:(self.root/p).read_bytes() for p in docs.PRIMARY}
        self.assertEqual(docs.record(self.root, self.event_path, True)["state"], "ALREADY_RECORDED")
        self.assertEqual(before, {p:(self.root/p).read_bytes() for p in docs.PRIMARY})

    def test_same_id_changed_payload_refused(self):
        self.write_event(); docs.record(self.root, self.event_path, True)
        self.event["summary"] = "Changed material claim"; self.write_event()
        with self.assertRaisesRegex(ValueError, "EVENT_ID_REUSED"): docs.record(self.root, self.event_path, True)

    def test_exact_repeat_after_git_checkpoint_is_noop(self):
        target=pathlib.Path(self.temp.name)/"checkpoint-copy"
        subprocess.run(["git","clone","--quiet","--shared",str(self.root),str(target)],check=True,capture_output=True,timeout=30)
        try:
            self.write_event();docs.record(target,self.event_path,True)
            subprocess.run(["git","add","--",*docs.PRIMARY],cwd=target,check=True,capture_output=True)
            subprocess.run(["git","-c","user.name=Fixture","-c","user.email=fixture@example.invalid","commit","-qm","Fixture checkpoint"],cwd=target,check=True,capture_output=True)
            self.assertEqual(docs.record(target,self.event_path,True)["state"],"ALREADY_RECORDED")
        finally:
            assert target.resolve().parent==pathlib.Path(self.temp.name).resolve()
            # The owning TemporaryDirectory handles Windows read-only Git
            # objects when it cleans the complete fixture at class teardown.

    def test_interrupted_update_marker_does_not_fake_completion(self):
        self.write_event(); docs.record(self.root, self.event_path, True)
        (self.root/docs.PRIMARY[4]).write_bytes(self.baseline[docs.PRIMARY[4]])
        with self.assertRaisesRegex(ValueError, "PARTIAL_EVENT_UPDATE"): docs.record(self.root, self.event_path, True)

    def test_source_identity_must_be_current(self):
        self.event["source_sha"] = "0"*40; self.write_event()
        with self.assertRaisesRegex(ValueError, "EVENT_SOURCE_NOT_CURRENT"): docs.record(self.root, self.event_path, True)

    def test_evidence_digest_mismatch_refused(self):
        self.event["evidence"][0]["sha256"] = "0"*64; self.write_event()
        with self.assertRaisesRegex(ValueError, "EVIDENCE_IDENTITY_MISMATCH"): docs.record(self.root, self.event_path, True)

    def test_evidence_path_escape_refused(self):
        self.event["evidence"][0]["path"] = "../outside.json"; self.write_event()
        with self.assertRaisesRegex(ValueError, "EVIDENCE_OUTSIDE"): docs.record(self.root, self.event_path, True)

    def test_unknown_task_cannot_replace_current_handoff(self):
        self.event["ticket"] = "FOREIGN-TASK"; self.write_event()
        with self.assertRaisesRegex(ValueError, "NOT_CURRENT_HANDOFF"): docs.record(self.root, self.event_path, True)

    def test_completion_is_not_automatic_review_authority(self):
        self.event["status"] = "DONE"; self.write_event()
        with self.assertRaisesRegex(ValueError, "ACCEPTANCE_TRANSITION"): docs.record(self.root, self.event_path, True)

    def test_source_event_cannot_establish_installed_acceptance(self):
        self.event["maturity"] = "INSTALLED_VERIFIED"; self.write_event()
        with self.assertRaisesRegex(ValueError, "LIVE_ACCEPTANCE_TRANSITION"): docs.record(self.root, self.event_path, True)

    def test_private_payload_refused(self):
        self.event["summary"] = "Bearer private-value"; self.write_event()
        with self.assertRaisesRegex(ValueError, "PRIVATE_EVENT"): docs.record(self.root, self.event_path, True)

    def test_duplicate_ticket_and_status_conflicts_detected(self):
        p = self.root/docs.PRIMARY[4]; t=p.read_text(); row=next(x for x in t.splitlines() if x.startswith("| DOC-CONTINUITY |"));p.write_text(t+'\n'+row+'\n')
        self.assertIn("DUPLICATE_TICKET_ID", docs.inspect(self.root)["errors"])

    def test_missing_link_detected(self):
        p=self.root/docs.PRIMARY[0];p.write_text(p.read_text()+'\n[missing](not-present.md)\n')
        self.assertTrue(any(x.startswith("MISSING_LINK:") for x in docs.inspect(self.root)["errors"]))

    def test_unknown_handoff_commit_detected(self):
        p=self.root/docs.PRIMARY[0];p.write_text(p.read_text().replace('"source_sha":"'+docs.machine_handoff(p.read_text())["source_sha"]+'"','"source_sha":"'+'0'*40+'"'))
        self.assertIn("HANDOFF_IDENTITY_UNPROVEN", docs.inspect(self.root)["errors"])

    @unittest.skipUnless(shutil.which("pwsh"), "PowerShell 7 unavailable")
    def test_existing_development_hook_checks_docs_before_unchanged_rust_commands(self):
        # Execute only the real extracted function with an inert command seam.
        script = r'''$ErrorActionPreference='Stop'; $a=[Management.Automation.Language.Parser]::ParseFile((Join-Path $env:CATDESK_DOCS_TEST_SOURCE 'scripts/catdesk-dev-lane.ps1'),[ref]$null,[ref]$null); $f=@($a.FindAll({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Invoke-VerificationCommands'},$true)); if ($f.Count -ne 1) { throw 'AMBIGUOUS_FUNCTION' }; $script:calls=[Collections.Generic.List[object]]::new(); function Invoke-FixedCommand { param($name,$arguments) $script:calls.Add(@{name=$name;arguments=@($arguments)}) }; Invoke-Expression $f[0].Extent.Text; $prior=$env:CARGO_INCREMENTAL; Invoke-VerificationCommands; @{calls=@($script:calls.ToArray());environmentRestored=($env:CARGO_INCREMENTAL -ceq $prior)} | ConvertTo-Json -Depth 5 -Compress'''
        env = {k:v for k,v in os.environ.items() if not k.startswith(("CATDESK_", "CONTROL_PLANE_"))}
        env["CATDESK_DOCS_TEST_SOURCE"] = str(ROOT)
        p=subprocess.run([shutil.which("pwsh"),"-NoProfile","-Command",script],env=env,capture_output=True,timeout=15)
        self.assertEqual(p.returncode,0,p.stderr.decode())
        r=json.loads(p.stdout);self.assertTrue(r["environmentRestored"])
        self.assertEqual(r["calls"][0],dict(name="python",arguments=["-I","-B","scripts/catdesk_docs.py","check"]))
        self.assertEqual([c["arguments"][0] for c in r["calls"][1:]],["fmt","clippy","test"])


if __name__ == "__main__": unittest.main()
