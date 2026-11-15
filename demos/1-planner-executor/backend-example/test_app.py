import http.client
import json
import tempfile
import threading
import unittest
from pathlib import Path

from app import create_server


class ScoreApiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp_dir = tempfile.mkdtemp()
        cls.server = create_server(Path(cls.temp_dir) / "test-scores.db", host="127.0.0.1", port=0)
        cls.port = cls.server.server_address[1]
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()
        import shutil

        shutil.rmtree(cls.temp_dir)

    def request(self, method, path, body=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.port)
        encoded = None if body is None else json.dumps(body)
        connection.request(method, path, body=encoded, headers={"Content-Type": "application/json"})
        response = connection.getresponse()
        payload = response.read()
        result = None if not payload else json.loads(payload)
        headers = {key.lower(): value for key, value in response.getheaders()}
        status = response.status
        connection.close()
        return status, result, headers

    def test_scores_post_ordering_limit_and_shape(self):
        status, first, headers = self.request("POST", "/api/scores", {"name": "  ann  ", "score": 12})
        self.assertEqual(status, 201)
        self.assertEqual(first["name"], "ann")
        self.assertRegex(first["created_at"], r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ")
        self.assertEqual(headers["content-type"], "application/json")
        self.assertEqual(headers["access-control-allow-origin"], "*")
        self.request("POST", "/api/scores", {"name": "bob", "score": 50})
        status, scores, _ = self.request("GET", "/api/scores?limit=1")
        self.assertEqual(status, 200)
        self.assertEqual(len(scores), 1)
        self.assertEqual(scores[0]["name"], "bob")
        self.assertEqual(set(scores[0]), {"id", "name", "score", "created_at"})

    def test_validation_errors(self):
        for body in (
            None,
            {"name": "", "score": 1},
            {"name": " " * 21, "score": 1},
            {"name": "x" * 21, "score": 1},
            {"name": 1, "score": 1},
            {"name": "x", "score": True},
            {"name": "x", "score": 1.5},
            {"name": "x", "score": -1},
            {"name": "x", "score": 1_000_001},
            {"name": "x"},
        ):
            status, result, headers = self.request("POST", "/api/scores", body)
            self.assertEqual(status, 400, body)
            self.assertIsInstance(result["error"], str)
            self.assertEqual(headers["access-control-allow-origin"], "*")
        connection = http.client.HTTPConnection("127.0.0.1", self.port)
        connection.request("POST", "/api/scores", body="{bad", headers={"Content-Type": "application/json"})
        response = connection.getresponse()
        self.assertEqual(response.status, 400)
        self.assertIn("error", json.loads(response.read()))
        connection.close()
        connection = http.client.HTTPConnection("127.0.0.1", self.port)
        connection.request("POST", "/api/scores", body=b"\xff", headers={"Content-Type": "application/json"})
        response = connection.getresponse()
        self.assertEqual(response.status, 400)
        connection.close()

    def test_invalid_limit_not_found_and_options(self):
        for limit in ("0", "101", "abc", "1&limit=2"):
            status, result, _ = self.request("GET", "/api/scores?limit=" + limit)
            self.assertEqual(status, 400)
            self.assertIn("error", result)
        status, result, headers = self.request("GET", "/unknown")
        self.assertEqual((status, result), (404, {"error": "not found"}))
        self.assertEqual(headers["access-control-allow-methods"], "GET, POST, OPTIONS")
        status, result, headers = self.request("OPTIONS", "/anything")
        self.assertEqual(status, 204)
        self.assertIsNone(result)
        self.assertEqual(headers["access-control-allow-headers"], "Content-Type")


if __name__ == "__main__":
    unittest.main()
