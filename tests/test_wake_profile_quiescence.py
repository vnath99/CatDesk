import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("profile_quiescence", Path(__file__).resolve().parents[1] / "scripts/wake_profile_quiescence.py")
probe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(probe)


class ProfileQuiescenceTests(unittest.TestCase):
    def row(self, pid, name, parent=0, created=1, image=None, args=None):
        return dict(pid=pid, name=name, parent=parent, created=created,
                    image=image or name, args=args or [name])

    def classify(self, rows, samefile=lambda a, b: a == b):
        return probe.classify(rows, "selected-profile", "selected-python", samefile)

    def test_no_candidate_process_is_quiescent_not_a_submission_receipt(self):
        self.assertEqual(self.classify([]), "PROFILE_QUIESCENT")

    def test_selected_python_is_active_even_without_browser(self):
        self.assertEqual(self.classify([self.row(1, "python.exe", image="selected-python")]), "PROFILE_HELPER_ACTIVE")

    def test_profile_option_both_spellings_and_alias_identity_are_refused(self):
        for args in (["chrome.exe", "--user-data-dir=selected-profile"],
                     ["chrome.exe", "--user-data-dir", "selected-profile"],
                     ["chrome.exe", "--user-data-dir=physical-alias"]):
            with self.subTest(args=args):
                self.assertEqual(self.classify([self.row(1, "chrome.exe", args=args)],
                    lambda a, b: a == b or (a == "physical-alias" and b == "selected-profile")), "PROFILE_BROWSER_ACTIVE")

    def test_unrelated_browser_and_children_do_not_globally_block_recovery(self):
        rows = [self.row(1, "chrome.exe", args=["chrome.exe", "--user-data-dir=other-profile"]),
                self.row(2, "chrome.exe", parent=1, created=2, args=["chrome.exe", "--type=renderer"]),
                self.row(3, "chrome.exe", parent=2, created=3, args=["chrome.exe", "--type=utility"])]
        self.assertEqual(self.classify(rows), "PROFILE_QUIESCENT")

    def test_default_profile_cannot_own_the_qualified_explicit_profile(self):
        self.assertEqual(self.classify([self.row(1, "chrome.exe")]), "PROFILE_QUIESCENT")

    def test_orphan_browser_or_driver_is_unattributed_not_assumed_unrelated(self):
        for row in [self.row(2, "chrome.exe", parent=1, args=["chrome.exe", "--type=renderer"]),
                    self.row(2, "uc_driver.exe", parent=1), self.row(3, "selenium-manager.exe")]:
            with self.subTest(name=row["name"]):
                self.assertEqual(self.classify([row]), "PROFILE_OWNER_UNATTRIBUTED")

    def test_known_other_project_python_driver_and_browser_are_independent(self):
        rows = [self.row(1, "python.exe", image="other-python"),
                self.row(2, "uc_driver.exe", parent=1, created=2),
                self.row(3, "chrome.exe", parent=2, created=3, args=["chrome.exe", "--user-data-dir=other-profile"])]
        self.assertEqual(self.classify(rows), "PROFILE_QUIESCENT")

    def test_pid_reuse_or_ancestry_cycle_is_not_quiescence(self):
        reused = [self.row(1, "chrome.exe", created=9),
                  self.row(2, "chrome.exe", parent=1, created=2, args=["chrome.exe", "--type=renderer"])]
        self.assertEqual(self.classify(reused), "PROFILE_OWNER_UNATTRIBUTED")
        cycle = [self.row(1, "chrome.exe", parent=2, args=["chrome.exe", "--type=renderer"]),
                 self.row(2, "chrome.exe", parent=1, args=["chrome.exe", "--type=utility"])]
        with self.assertRaises(ValueError):
            self.classify(cycle)
        with self.assertRaises(ValueError):
            self.classify([self.row(1, "chrome.exe"), self.row(1, "chrome.exe")])

    def test_unknown_profile_identity_is_not_quiescence(self):
        def unavailable(a, b): raise OSError("private path is not exported")
        with self.assertRaises(OSError):
            self.classify([self.row(1, "chrome.exe", args=["chrome.exe", "--user-data-dir=unknown"])], unavailable)

    def test_duplicate_missing_or_empty_options_are_ambiguous(self):
        for args in (["chrome.exe", "--user-data-dir"],
                     ["chrome.exe", "--user-data-dir="],
                     ["chrome.exe", "--user-data-dir=a", "--user-data-dir=b"],
                     ["chrome.exe", "--user-data-dir", "--type=renderer"]):
            with self.subTest(args=args), self.assertRaises(ValueError):
                self.classify([self.row(1, "chrome.exe", args=args)])


if __name__ == "__main__":
    unittest.main()
