# SPDX-License-Identifier: AGPL-3.0-only
"""
Live integration test connecting TemnionClient to a real temniond process.
"""

import os
import shutil
import socket
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

from temnion import QueryFormat, TemnionClient


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("", 0))
        return s.getsockname()[1]


class TestLiveDaemonIntegration(unittest.TestCase):
    """End-to-end integration test against live temniond daemon."""

    @classmethod
    def setUpClass(cls):
        cls.test_dir = tempfile.mkdtemp(prefix="temnion-py-test-")
        cls.port = find_free_port()

        # Locate temniond binary
        repo_root = Path(__file__).resolve().parent.parent.parent.parent
        daemon_bin = repo_root / "target" / "debug" / ("temniond.exe" if os.name == "nt" else "temniond")

        if not daemon_bin.exists():
            daemon_bin = repo_root / "target" / "release" / ("temniond.exe" if os.name == "nt" else "temniond")

        if not daemon_bin.exists():
            raise unittest.SkipTest(f"temniond binary not found at {daemon_bin}")

        # Start temniond process
        cls.process = subprocess.Popen(
            [
                str(daemon_bin),
                "run",
                "--data-dir", cls.test_dir,
                "--bind", f"127.0.0.1:{cls.port}",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        # Wait for daemon to bind port
        for _ in range(30):
            try:
                with socket.create_connection(("127.0.0.1", cls.port), timeout=0.2):
                    break
            except OSError:
                time.sleep(0.1)
        else:
            cls.process.kill()
            raise RuntimeError("temniond failed to start within 3 seconds")

    @classmethod
    def tearDownClass(cls):
        if hasattr(cls, "process"):
            cls.process.terminate()
            try:
                cls.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                cls.process.kill()
        if hasattr(cls, "test_dir") and os.path.exists(cls.test_dir):
            shutil.rmtree(cls.test_dir, ignore_errors=True)

    def test_live_ping_and_describe(self):
        with TemnionClient(host="127.0.0.1", port=self.port, timeout=5.0) as client:
            latency = client.ping()
            self.assertGreater(latency, 0.0)
            self.assertLess(latency, 500.0)  # sub-half-second on loopback

            desc = client.describe()
            self.assertTrue(desc.server_id.startswith("temniond"))
            self.assertEqual(desc.version, 1)
            self.assertIn("tnp", desc.capabilities)
            self.assertIn("live-subscription", desc.capabilities)

    def test_live_query_execution(self):
        with TemnionClient(host="127.0.0.1", port=self.port, timeout=5.0) as client:
            # Query empty database via SQL
            res = client.query("SELECT * FROM events", format_=QueryFormat.SQL)
            self.assertEqual(len(res), 0)
            self.assertFalse(res.truncated)

            # Query via TemQL
            res_temql = client.query("FROM events SELECT *", format_=QueryFormat.TEMQL)
            self.assertEqual(len(res_temql), 0)


if __name__ == "__main__":
    unittest.main()
