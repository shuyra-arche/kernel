"""CI partition contract: no lost tests, duplicate execution, or false success."""

import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / "shard.py"
SPEC = importlib.util.spec_from_file_location("hardware_shard", SOURCE)
shard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(shard)


class Partition(unittest.TestCase):
    def setUp(self):
        self.groups = {"left": ("alpha", "nested::beta"), "right": ("gamma",)}
        self.actual = ("alpha", "nested::beta", "gamma")

    def test_inventory_ignores_summaries_and_benchmarks(self):
        self.assertEqual(
            shard.inventory("alpha: test\nnested::beta: test\ncost: benchmark\n\n2 tests, 1 benchmark\n"),
            ("alpha", "nested::beta"),
        )

    def test_partition_selects_only_exact_members(self):
        self.assertEqual(shard.select("left", self.actual, self.groups), ("alpha", "nested::beta"))

    def test_added_test_requires_an_assignment(self):
        with self.assertRaisesRegex(ValueError, "unassigned=.*delta"):
            shard.select("left", (*self.actual, "delta"), self.groups)

    def test_removed_or_renamed_test_fails(self):
        with self.assertRaisesRegex(ValueError, "stale=.*gamma"):
            shard.select("left", self.actual[:-1], self.groups)

    def test_duplicate_assignment_fails(self):
        groups = {**self.groups, "extra": ("alpha",)}
        with self.assertRaisesRegex(ValueError, "duplicate tests in partition"):
            shard.select("left", self.actual, groups)

    def test_duplicate_inventory_fails(self):
        with self.assertRaisesRegex(ValueError, "duplicate tests in inventory"):
            shard.select("left", (*self.actual, "alpha"), self.groups)

    def test_empty_inventory_fails(self):
        with self.assertRaisesRegex(ValueError, "empty"):
            shard.select("left", (), self.groups)

    def test_empty_shard_fails(self):
        with self.assertRaisesRegex(ValueError, "empty"):
            shard.select("left", self.actual, {**self.groups, "empty": ()})

    def test_unknown_shard_fails(self):
        with self.assertRaisesRegex(ValueError, "unknown"):
            shard.select("typo", self.actual, self.groups)

    def test_real_partition_is_disjoint_and_nonempty(self):
        actual = tuple(test for tests in shard.SHARDS.values() for test in tests)
        selected = tuple(test for group in shard.SHARDS for test in shard.select(group, actual))
        self.assertCountEqual(selected, actual)
        self.assertEqual(len(selected), len(set(selected)))


    def test_ci_matrix_runs_every_shard_once(self):
        import re

        workflow = (shard.ROOT / ".github/workflows/check.yml").read_text(encoding="utf-8")
        match = re.search(
            r"^[ \t]*shard:\s*\[([a-z0-9_, -]+)\][ \t]*$",
            workflow,
            re.MULTILINE,
        )
        self.assertIsNotNone(match, "CI matrix must enumerate QEMU shards")
        configured = tuple(name.strip() for name in match.group(1).split(","))
        self.assertCountEqual(configured, shard.SHARDS.keys())

class Execution(unittest.TestCase):
    def listing(self):
        return subprocess.CompletedProcess(
            [], 0, stdout="".join(f"{test}: test\n" for tests in shard.SHARDS.values() for test in tests)
        )

    @patch.object(shard.subprocess, "run")
    def test_cargo_filters_preserve_flags_and_failure(self, execute):
        execute.side_effect = [self.listing(), subprocess.CompletedProcess([], 101)]
        self.assertEqual(shard.run("context"), 101)
        listing, running = execute.call_args_list
        self.assertEqual(listing.args[0], [*shard.CARGO, "--", "--ignored", "--list", "--format", "terse"])
        self.assertTrue(listing.kwargs["check"])
        self.assertEqual(
            running.args[0],
            [*shard.CARGO, "--", "--ignored", "--exact", "--nocapture", "--test-threads=1", *shard.SHARDS["context"]],
        )
        self.assertEqual(running.kwargs["cwd"], shard.ROOT)
        self.assertFalse(running.kwargs["check"])

    @patch.object(shard.subprocess, "run")
    def test_inventory_failure_never_runs_tests(self, execute):
        execute.side_effect = subprocess.CalledProcessError(101, ["cargo"])
        self.assertEqual(shard.main(["memory"]), 1)
        execute.assert_called_once()

    @patch.object(shard.subprocess, "run")
    def test_drift_never_runs_tests(self, execute):
        listed = self.listing()
        listed.stdout += "new::regression: test\n"
        execute.return_value = listed
        self.assertEqual(shard.main(["memory"]), 1)
        execute.assert_called_once()

    @patch.object(shard.subprocess, "run")
    def test_signal_is_a_failure(self, execute):
        execute.side_effect = [self.listing(), subprocess.CompletedProcess([], -15)]
        self.assertEqual(shard.run("io"), 1)

    @patch.object(shard.subprocess, "run")
    def test_success_is_preserved(self, execute):
        execute.side_effect = [self.listing(), subprocess.CompletedProcess([], 0)]
        self.assertEqual(shard.run("scheduling"), 0)


if __name__ == "__main__":
    unittest.main()
