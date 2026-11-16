import json
import os
import tempfile
import threading
import unittest
from http.client import HTTPConnection
from http.server import HTTPServer

import server


class ScoreApiTests(unittest.TestCase):
    def setUp(self):
        self.original_directory = os.getcwd()
        self.temporary_directory = tempfile.TemporaryDirectory()
        os.chdir(self.temporary_directory.name)
        self.start_server()

    def start_server(self):
        self.httpd = HTTPServer(("127.0.0.1", 0), server.ScoreHandler)
        self.port = self.httpd.server_address[1]
        self.thread = threading.Thread(target=self.httpd.serve_forever)
        self.thread.start()

    def stop_server(self):
        self.httpd.shutdown()
        self.httpd.server_close()
        self.thread.join()

    def tearDown(self):
        self.stop_server()
        os.chdir(self.original_directory)
        self.temporary_directory.cleanup()

    def request(self, method, path="/api/scores", payload=None):
        connection = HTTPConnection("127.0.0.1", self.port)
        body = None if payload is None else json.dumps(payload)
        connection.request(method, path, body=body,
                           headers={"Content-Type": "application/json"} if body is not None else {})
        response = connection.getresponse()
        data = response.read()
        result = (response.status, response.getheaders(), json.loads(data) if data else None)
        connection.close()
        return result

    def test_empty_list(self):
        self.assertEqual(self.request("GET")[2], [])

    def test_post_then_get(self):
        status, headers, entry = self.request("POST", payload={"name": "  Ada  ", "score": 42})
        self.assertEqual(status, 201)
        self.assertEqual(entry["name"], "Ada")
        self.assertRegex(entry["date"], r"^\d{4}-\d\d-\d\dT.*Z$")
        self.assertEqual(self.request("GET")[2], [entry])

    def test_top_ten_and_ties_keep_order(self):
        for score in range(12):
            self.request("POST", payload={"name": str(score), "score": score})
        self.request("POST", payload={"name": "Tie", "score": 11})
        scores = self.request("GET")[2]
        self.assertEqual(len(scores), 10)
        self.assertEqual([item["score"] for item in scores], sorted(
            [item["score"] for item in scores], reverse=True))
        self.assertEqual(scores[0]["name"], "11")

    def test_validation_errors(self):
        for payload in [
            {"name": "   ", "score": 1}, {"name": "x" * 21, "score": 1},
            {"name": "x", "score": -1}, {"name": "x", "score": True},
            {"name": 3, "score": 1}, {"name": "x"},
        ]:
            status, _, body = self.request("POST", payload=payload)
            self.assertEqual(status, 400)
            self.assertEqual(set(body), {"error"})
        connection = HTTPConnection("127.0.0.1", self.port)
        connection.request("POST", "/api/scores", body="{bad", headers={"Content-Type": "application/json"})
        response = connection.getresponse()
        self.assertEqual(response.status, 400)
        self.assertIn("error", json.loads(response.read()))
        connection.close()

    def test_unknown_path_and_bad_method(self):
        self.assertEqual(self.request("GET", "/missing")[0], 404)
        self.assertEqual(self.request("POST", "/missing", {"name": "A", "score": 1})[0], 404)
        self.assertEqual(self.request("PUT")[0], 405)

    def test_cors_headers(self):
        status, headers, _ = self.request("GET")
        self.assertEqual(status, 200)
        self.assertEqual(dict(headers)["Access-Control-Allow-Origin"], "*")
        status, headers, _ = self.request("OPTIONS")
        self.assertEqual(status, 204)
        self.assertEqual(dict(headers)["Access-Control-Allow-Methods"], "GET, POST, OPTIONS")
        self.assertEqual(dict(headers)["Access-Control-Allow-Headers"], "Content-Type")

    def test_persistence_after_restart(self):
        entry = self.request("POST", payload={"name": "Persist", "score": 99})[2]
        self.stop_server()
        self.start_server()
        self.assertEqual(self.request("GET")[2], [entry])


if __name__ == "__main__":
    unittest.main()
