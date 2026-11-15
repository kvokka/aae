import json
import tempfile
import threading
import unittest
from http.server import ThreadingHTTPServer
from urllib.error import HTTPError
from urllib.request import Request, urlopen

import server


class ScoresApiTests(unittest.TestCase):
    def setUp(self):
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.previous_scores_file = server.SCORES_FILE
        server.SCORES_FILE = self.temporary_directory.name + "/scores.json"
        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), server.ScoresHandler)
        self.thread = threading.Thread(target=self.httpd.serve_forever)
        self.thread.start()
        self.base_url = "http://127.0.0.1:{}".format(self.httpd.server_port)

    def tearDown(self):
        self.httpd.shutdown()
        self.thread.join()
        self.httpd.server_close()
        server.SCORES_FILE = self.previous_scores_file
        self.temporary_directory.cleanup()

    def request(self, path, method="GET", payload=None, headers=None):
        body = None if payload is None else json.dumps(payload).encode("utf-8")
        request = Request(self.base_url + path, data=body, method=method, headers=headers or {})
        try:
            return urlopen(request)
        except HTTPError as error:
            return error

    def test_empty_list_and_saved_entry(self):
        response = self.request("/api/scores")
        self.assertEqual(response.status, 200)
        self.assertEqual(json.loads(response.read()), [])
        response = self.request("/api/scores", "POST", {"name": " ann ", "score": 42})
        self.assertEqual(response.status, 201)
        entry = json.loads(response.read())
        self.assertEqual(entry["name"], "ann")
        self.assertEqual(entry["score"], 42)
        self.assertTrue(entry["created_at"].endswith("Z"))
        self.assertEqual(json.loads(self.request("/api/scores").read()), [entry])

    def test_validation_and_invalid_json(self):
        for payload in (
            {"name": "", "score": 1},
            {"name": "x" * 21, "score": 1},
            {"name": "ok", "score": True},
            {"name": "ok", "score": -1},
            {"name": "ok", "score": 1000001},
        ):
            response = self.request("/api/scores", "POST", payload)
            self.assertEqual(response.status, 400)
            self.assertIn("error", json.loads(response.read()))
        response = self.request("/api/scores", "POST", headers={"Content-Type": "application/json"})
        self.assertEqual(response.status, 400)

    def test_sort_top_ten_cors_options_and_routes(self):
        for score in range(12):
            self.request("/api/scores", "POST", {"name": "player", "score": score})
        response = self.request("/api/scores")
        scores = json.loads(response.read())
        self.assertEqual([item["score"] for item in scores], list(range(11, 1, -1)))
        self.assertEqual(response.headers["Access-Control-Allow-Origin"], "*")
        options = self.request("/anything", "OPTIONS")
        self.assertEqual(options.status, 204)
        self.assertEqual(options.headers["Content-Type"], "application/json")
        self.assertEqual(self.request("/missing").status, 404)
        self.assertEqual(self.request("/api/scores", "PUT").status, 405)


if __name__ == "__main__":
    unittest.main()
