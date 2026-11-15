import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class ScoreServerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary_directory = tempfile.TemporaryDirectory()
        cls.port = 18765
        cls.scores_path = Path(cls.temporary_directory.name) / "scores.json"
        environment = os.environ.copy()
        environment.update(PORT=str(cls.port), SCORES_FILE=str(cls.scores_path))
        cls.process = subprocess.Popen(
            [sys.executable, str(ROOT / "server.py")],
            cwd=ROOT,
            env=environment,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        cls.base_url = f"http://127.0.0.1:{cls.port}"
        for _ in range(50):
            if cls.process.poll() is not None:
                raise RuntimeError("score server exited during startup")
            try:
                with urllib.request.urlopen(cls.base_url + "/api/scores", timeout=1):
                    break
            except (OSError, urllib.error.URLError):
                time.sleep(0.1)
        else:
            cls.process.terminate()
            raise RuntimeError("score server did not start")

    @classmethod
    def tearDownClass(cls):
        cls.process.terminate()
        cls.process.wait(timeout=5)
        cls.temporary_directory.cleanup()

    def request(self, path, method="GET", body=None, content_type="application/json"):
        data = None if body is None else json.dumps(body).encode("utf-8")
        request = urllib.request.Request(self.base_url + path, data=data, method=method)
        if data is not None and content_type:
            request.add_header("Content-Type", content_type)
        try:
            response = urllib.request.urlopen(request, timeout=2)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.code, json.loads(response.read()), response.headers

    def test_empty_then_create_and_list_score(self):
        status, entries, headers = self.request("/api/scores")
        self.assertEqual(status, 200)
        self.assertEqual(entries, [])
        self.assertEqual(headers["Access-Control-Allow-Origin"], "*")

        status, entry, _ = self.request(
            "/api/scores", "POST", {"name": "  Ada  ", "score": 42}
        )
        self.assertEqual(status, 201)
        self.assertEqual(entry["name"], "Ada")
        self.assertEqual(entry["score"], 42)
        self.assertRegex(entry["created_at"], r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$")

        status, entries, _ = self.request("/api/scores")
        self.assertEqual(status, 200)
        self.assertEqual(entries, [entry])

    def test_invalid_name_is_bad_request(self):
        status, error, headers = self.request(
            "/api/scores", "POST", {"name": "   ", "score": 10}
        )
        self.assertEqual(status, 400)
        self.assertIn("error", error)
        self.assertEqual(headers["Access-Control-Allow-Origin"], "*")


if __name__ == "__main__":
    unittest.main()
