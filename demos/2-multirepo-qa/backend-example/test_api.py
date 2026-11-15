"""Repeatable end-to-end checks; runs isolated servers and a temporary score file."""

import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parent
BASE = "http://127.0.0.1:8000"


def request(method="GET", body=None, headers=None):
    data = None if body is None else json.dumps(body).encode()
    req = Request(BASE + "/api/scores", data=data, headers=headers or {}, method=method)
    try:
        with urlopen(req, timeout=2) as response:
            return response.status, response.headers, response.read()
    except HTTPError as response:
        with response:
            return response.code, response.headers, response.read()


class ScoresAPITest(unittest.TestCase):
    def test_api_and_restart_persistence(self):
        with tempfile.TemporaryDirectory() as directory:
            env = dict(os.environ, SCORE_STORE_PATH=str(Path(directory) / "scores.json"))

            def start():
                process = subprocess.Popen([sys.executable, str(ROOT / "server.py")],
                                           cwd=ROOT, env=env,
                                           stdout=subprocess.DEVNULL,
                                           stderr=subprocess.DEVNULL)
                for _ in range(50):
                    if process.poll() is not None:
                        self.fail("server exited during startup")
                    try:
                        request()
                        return process
                    except Exception:
                        time.sleep(0.1)
                process.terminate()
                self.fail("server did not start")

            process = start()
            try:
                status, _, body = request()
                self.assertEqual((status, json.loads(body)), (200, {"scores": []}))
                status, _, body = request("POST", {"name": " Ada ", "score": 12},
                                          {"Content-Type": "application/json"})
                self.assertEqual(status, 201)
                self.assertEqual(json.loads(body)["scores"], [{"name": "Ada", "score": 12}])

                for score in range(15):
                    request("POST", {"name": "P%d" % score, "score": score},
                            {"Content-Type": "application/json"})
                status, _, body = request()
                scores = json.loads(body)["scores"]
                self.assertEqual(status, 200)
                self.assertEqual(len(scores), 10)
                self.assertEqual([row["score"] for row in scores],
                                 [14, 13, 12, 12, 11, 10, 9, 8, 7, 6])
                tied_names = [row["name"] for row in scores if row["score"] == 12]
                self.assertEqual(tied_names, ["Ada", "P12"])

                for invalid in ({"name": " ", "score": 0},
                                {"name": "X" * 21, "score": 0},
                                {"name": "Ada", "score": True},
                                {"name": "Ada", "score": -1},
                                {"name": "Ada", "score": 1.5}):
                    status, _, body = request("POST", invalid, {"Content-Type": "application/json"})
                    self.assertEqual(status, 400, body)
                    self.assertIn("error", json.loads(body))
                status, _, body = request("POST", headers={"Content-Type": "application/json"})
                self.assertEqual(status, 400)
                self.assertIn("error", json.loads(body))
                status, _, body = request("POST", {"name": "Ada", "score": 1},
                                          {"Content-Type": "text/plain"})
                self.assertEqual(status, 400)

                req = Request(BASE + "/api/scores", method="OPTIONS",
                              headers={"Origin": "http://127.0.0.1:5173",
                                       "Access-Control-Request-Method": "POST",
                                       "Access-Control-Request-Headers": "content-type"})
                with urlopen(req, timeout=2) as response:
                    self.assertEqual(response.status, 204)
                    self.assertEqual(response.headers["Access-Control-Allow-Origin"],
                                     "http://127.0.0.1:5173")
                    self.assertIn("POST", response.headers["Access-Control-Allow-Methods"])
                    self.assertIn("Content-Type", response.headers["Access-Control-Allow-Headers"])
            finally:
                process.terminate()
                process.wait(timeout=5)

            process = start()
            try:
                status, _, body = request()
                self.assertEqual(status, 200)
                self.assertEqual(json.loads(body)["scores"][0], {"name": "P14", "score": 14})
            finally:
                process.terminate()
                process.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
