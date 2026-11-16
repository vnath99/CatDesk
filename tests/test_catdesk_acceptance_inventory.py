import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("acceptance_inventory", Path(__file__).parents[1] / "scripts/catdesk_acceptance_inventory.py")
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


class InventoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def save(self, path, value):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value), encoding="utf-8")

    def package(self, development=False):
        runtime = self.root / "runtime" if development else self.root
        package = runtime / "versions/dev.85"
        package.mkdir(parents=True)
        artifacts = {}
        for name in module.ARTIFACTS:
            data = b"fixture " + name.encode()
            (package / name).write_bytes(data)
            artifacts[name] = hashlib.sha256(data).hexdigest()
        pointer = {"directory": "dev.85", **{field: artifacts[name] for name, field in module.ARTIFACTS.items()}}
        self.save(runtime / "current.json", pointer)
        self.save(package / "manifest.json", {"schemaVersion": 1, "version": "dev.85", "artifacts": artifacts})
        if development:
            self.save(self.root / "authority.json", {
                "packageId": "dev.85", "artifacts": artifacts,
                "manifestSha256": module.digest(package / "manifest.json"),
                "pointerSha256": module.digest(runtime / "current.json"),
            })
        return package

    def test_missing_installation_is_not_pass(self):
        result = module.observe(lambda: module.package_inventory(self.root))
        self.assertEqual("NOT_INSTALLED", result["status"])

    def test_binary_mismatch_is_not_installed_acceptance(self):
        package = self.package()
        (package / "CatDeskWakeHost.exe").write_bytes(b"changed")
        result = module.package_inventory(self.root)
        self.assertEqual("FAIL", result["status"])
        self.assertEqual("UNVERIFIED", result["classification"])
        self.assertEqual("NOT_RUN", result["liveAcceptance"])

    def test_development_selection_does_not_claim_install_or_health(self):
        self.package(True)
        result = module.package_inventory(self.root, True)
        self.assertEqual("PASS", result["status"])
        self.assertEqual("SELECTED", result["classification"])
        self.assertEqual("NOT_EVALUATED", result["authorityPhysicalIdentity"])
        self.assertFalse(result["bootstrapProtocolPresent"])

    def test_changed_authority_rejects_matching_artifact_bytes(self):
        self.package(True)
        authority = module.document(self.root / "authority.json")
        authority["manifestSha256"] = "0" * 64
        self.save(self.root / "authority.json", authority)
        self.assertEqual("FAIL", module.package_inventory(self.root, True)["status"])

    def test_manifest_cannot_export_private_content(self):
        package = self.package()
        manifest = module.document(package / "manifest.json")
        manifest["credential"] = "SECRET_FIXTURE_MUST_NOT_EXPORT"
        self.save(package / "manifest.json", manifest)
        self.assertNotIn("SECRET_FIXTURE", json.dumps(module.package_inventory(self.root)))

    def test_traversal_pointer_fails_before_outside_read(self):
        self.save(self.root / "current.json", {"directory": "../foreign"})
        self.assertEqual("FAIL", module.observe(lambda: module.package_inventory(self.root))["status"])

    def test_malformed_manifest_is_bounded_diagnostic(self):
        package = self.package()
        (package / "manifest.json").write_text('["private-text"]', encoding="utf-8")
        result = module.observe(lambda: module.package_inventory(self.root))
        self.assertEqual({"status": "FAIL", "reason": "EVIDENCE_INVALID_OR_UNAVAILABLE"}, result)

    def test_matching_canonical_pair_does_not_establish_recovery_authority(self):
        binary = self.root / "target/release/catdesk.exe"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"fixture")
        value = module.digest(binary)
        binary.with_suffix(".exe.sha256").write_text(value, encoding="ascii")
        self.save(self.root / ".catdesk/release-recovery/current.json", {"sha256": value, "generation": 4})
        result = module.canonical_inventory(self.root)
        self.assertEqual("PASS", result["status"])
        self.assertEqual("NOT_EVALUATED", result["promotionAuthority"])
        self.assertEqual("NOT_RUN", result["runtimeHealth"])
        binary.write_bytes(b"different")
        self.assertEqual("FAIL", module.canonical_inventory(self.root)["status"])

    def test_stale_test_claim_never_promotes_source_status(self):
        fake_head = "a" * 40
        with patch.object(module, "plain", return_value=self.root), patch.object(module.subprocess, "run") as run:
            run.side_effect = [type("R", (), {"stdout": fake_head.encode()})(), type("R", (), {"stdout": b""})()]
            result = module.source_inventory(self.root)
        self.assertEqual("SOURCE_ONLY", result["classification"])
        self.assertEqual("NOT_RUN", result["tests"])

    def test_test_evidence_requires_current_commit_and_exact_declared_inputs(self):
        source = self.root / "example.py"
        source.write_bytes(b"source")
        evidence = self.root / "test-evidence.json"
        record = {"sourceCommit": "a" * 40, "exitCode": 0, "testsRun": 1, "inputHashes": {"example.py": module.digest(source)}}
        self.save(evidence, record)
        self.assertEqual("FAIL", module.test_inventory(self.root, evidence, "b" * 40)["status"])
        result = module.test_inventory(self.root, evidence, "a" * 40)
        self.assertEqual("TESTED_SOURCE", result["classification"])
        self.assertFalse(result["reviewAuthority"])
        source.write_bytes(b"changed")
        self.assertEqual("TEST_INPUTS_CHANGED", module.test_inventory(self.root, evidence, "a" * 40)["reason"])

    def test_build_record_requires_actual_artifact_checksum(self):
        image = self.root / "fixture.exe"
        image.write_bytes(b"fixture")
        evidence = self.root / "build.json"
        self.save(evidence, {"sourceCommit": "a" * 40, "passed": True, "controller": {"path": str(image), "sha256": module.digest(image)}})
        result = module.build_inventory(evidence, "b" * 40)
        self.assertEqual("BUILT", result["classification"])
        self.assertFalse(result["currentHeadMatchesBuildSource"])
        image.write_bytes(b"unknown")
        self.assertEqual("FAIL", module.build_inventory(evidence, "a" * 40)["status"])


if __name__ == "__main__":
    unittest.main()
