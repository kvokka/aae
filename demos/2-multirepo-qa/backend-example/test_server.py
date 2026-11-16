import json
import tempfile
import threading
import unittest
from http.server import HTTPServer
from http.client import HTTPConnection
from pathlib import Path
from unittest.mock import patch

import server


class HighScoreAPITest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp_dir = tempfile.TemporaryDirectory()
        cls.db_path = Path(cls.temp_dir.name) / "scores.sqlite3"
        cls.db_patch = patch.object(server, "DATABASE_PATH", cls.db_path)
        cls.db_patch.start()
        cls.httpd = HTTPServer(("127.0.0.1", 0), server.HighScoreHandler)
        cls.thread = threading.Thread(target=cls.httpd.serve_forever, daemon=True)
        cls.thread.start()
        cls.port = cls.httpd.server_port

    @classmethod
    def tearDownClass(cls):
        cls.httpd.shutdown()
        cls.thread.join()
        cls.httpd.server_close()
        cls.db_patch.stop()
        cls.temp_dir.cleanup()

    def request(self, method, path="/api/highscores", payload=None, origin=None):
        connection = HTTPConnection("127.0.0.1", self.port)
        headers = {}
        if payload is not None:
            headers["Content-Type"] = "application/json"
        if origin:
            headers["Origin"] = origin
        body = json.dumps(payload) if payload is not None else None
        connection.request(method, path, body=body, headers=headers)
        response = connection.getresponse()
        data = response.read()
        result = (response.status, dict(response.getheaders()), json.loads(data) if data else None)
        connection.close()
        return result

    def test_empty_valid_order_ties_and_top_ten(self):
        self.assertEqual(self.request("GET")[2], {"scores": []})
        status, headers, saved = self.request("POST", payload={"name": " Ada ", "score": 30})
        self.assertEqual((status, saved), (201, {"name": "Ada", "score": 30}))
        for score in range(12):
            self.request("POST", payload={"name": "Player%d" % score, "score": score})
        self.request("POST", payload={"name": "Bea", "score": 30})
        rows = self.request("GET")[2]["scores"]
        self.assertEqual(len(rows), 10)
        self.assertEqual([r["score"] for r in rows], sorted([r["score"] for r in rows], reverse=True))
        self.assertEqual([r["name"] for r in rows[:2]], ["Ada", "Bea"])
        self.assertEqual(headers.get("Content-Type"), "application/json")

    def test_invalid_values_and_malformed_json(self):
        for payload in ({"name": "  ", "score": 0}, {"name": "x" * 25, "score": 0},
                        {"name": "A", "score": True}, {"name": "A", "score": 1.5},
                        {"name": "A", "score": -1}, {"name": "A", "score": 2147483648}):
            status, _, body = self.request("POST", payload=payload)
            self.assertEqual(status, 400, payload)
            self.assertIn("error", body)
        connection = HTTPConnection("127.0.0.1", self.port)
        connection.request("POST", "/api/highscores", body="{")
        response = connection.getresponse()
        self.assertEqual(response.status, 400)
        self.assertIn("error", json.loads(response.read()))
        connection.close()

    def test_persistence_after_reopening_database(self):
        self.request("POST", payload={"name": "Persist", "score": 99})
        with server.connect_database() as connection:
            rows = connection.execute("SELECT name, score FROM scores WHERE name = ?", ("Persist",)).fetchall()
        self.assertEqual(rows, [("Persist", 99)])

    def test_cors_preflight(self):
        status, headers, _ = self.request("OPTIONS", origin="http://localhost:5173")
        self.assertEqual(status, 204)
        self.assertEqual(headers.get("Access-Control-Allow-Origin"), "http://localhost:5173")
        self.assertEqual(headers.get("Access-Control-Allow-Headers"), "Content-Type")
        self.assertIn("POST", headers.get("Access-Control-Allow-Methods", ""))


if __name__ == "__main__":
    unittest.main()
