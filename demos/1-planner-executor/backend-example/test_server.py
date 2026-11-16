import http.client
import json
import os
import tempfile
import threading
import unittest
from http.server import ThreadingHTTPServer

import server


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.scores_path = os.path.join(self.temp_dir.name, "scores.json")
        self.start_server()

    def start_server(self):
        self.httpd = ThreadingHTTPServer(
            ("127.0.0.1", 0), server.create_handler(self.scores_path)
        )
        self.thread = threading.Thread(target=self.httpd.serve_forever)
        self.thread.start()
        self.port = self.httpd.server_address[1]

    def stop_server(self):
        self.httpd.shutdown()
        self.thread.join()
        self.httpd.server_close()

    def tearDown(self):
        self.stop_server()
        self.temp_dir.cleanup()

    def request(self, method, path="/api/scores", body=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.port)
        data = None if body is None else json.dumps(body)
        headers = {"Content-Type": "application/json"} if data is not None else {}
        connection.request(method, path, body=data, headers=headers)
        response = connection.getresponse()
        raw = response.read()
        result = (response.status, response.headers, json.loads(raw) if raw else None)
        connection.close()
        return result

    def test_post_and_get(self):
        status, _, record = self.request("POST", body={"name": " ann ", "score": 42})
        self.assertEqual(status, 201)
        self.assertEqual(record["name"], "ann")
        self.assertEqual(record["score"], 42)
        self.assertRegex(record["created_at"], r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$")
        status, _, records = self.request("GET")
        self.assertEqual(status, 200)
        self.assertEqual(records, [record])

    def test_invalid_posts(self):
        cases = [
            {"name": "", "score": 1},
            {"name": "x" * 21, "score": 1},
            {"name": 4, "score": 1},
            {"name": "a", "score": -1},
            {"name": "a", "score": 1.5},
            {"name": "a", "score": True},
            {"name": "a"},
        ]
        for body in cases:
            with self.subTest(body=body):
                status, _, result = self.request("POST", body=body)
                self.assertEqual(status, 400)
                self.assertIn("error", result)
        connection = http.client.HTTPConnection("127.0.0.1", self.port)
        connection.request("POST", "/api/scores", body="{")
        response = connection.getresponse()
        result = json.loads(response.read())
        self.assertEqual(response.status, 400)
        self.assertIn("error", result)
        connection.close()

    def test_top_ten_and_sorting(self):
        records = [
            {"name": "low", "score": 1, "created_at": "2020-01-01T00:00:00Z"},
            {"name": "later", "score": 50, "created_at": "2020-01-02T00:00:00Z"},
            {"name": "earlier", "score": 50, "created_at": "2020-01-01T00:00:00Z"},
        ]
        records.extend(
            {"name": str(i), "score": i, "created_at": "2021-01-01T00:00:00Z"}
            for i in range(2, 12)
        )
        server.save_scores(records, self.scores_path)
        _, _, result = self.request("GET")
        self.assertEqual(len(result), 10)
        self.assertEqual(result[0]["name"], "earlier")
        self.assertEqual(result[1]["name"], "later")
        self.assertEqual(result, sorted(records, key=lambda r: (-r["score"], r["created_at"]))[:10])

    def test_options_cors_and_not_found(self):
        status, headers, body = self.request("OPTIONS")
        self.assertEqual(status, 204)
        self.assertIsNone(body)
        self.assertEqual(headers["Access-Control-Allow-Origin"], "*")
        self.assertEqual(headers["Access-Control-Allow-Methods"], "GET, POST, OPTIONS")
        self.assertEqual(headers["Access-Control-Allow-Headers"], "Content-Type")
        status, _, result = self.request("GET", "/missing")
        self.assertEqual((status, result), (404, {"error": "not found"}))

    def test_persistence_after_restart(self):
        self.request("POST", body={"name": "persist", "score": 9})
        self.stop_server()
        self.start_server()
        status, _, records = self.request("GET")
        self.assertEqual(status, 200)
        self.assertEqual(len(records), 1)
        self.assertEqual(records[0]["name"], "persist")


if __name__ == "__main__":
    unittest.main()
