# Snake score API

Requires Python 3 and uses only the standard library.

Run from this directory with:

```sh
python3 server.py
```

The server listens on `127.0.0.1:8080`. Run the tests with:

```sh
python3 -m unittest -v
```

## API

- `GET /api/scores` returns a JSON array of up to 10 entries, highest score first. Equal scores keep their earlier order.
- `POST /api/scores` accepts `{"name":"Ada","score":42}` and returns the saved entry with status 201. Names are trimmed and must contain 1–20 characters. Scores must be nonnegative integers (booleans are rejected).
- Invalid JSON or values return status 400 with `{"error":"..."}`. Unknown paths return 404; unsupported methods on the API path return 405.
- `OPTIONS /api/scores` returns 204 with CORS method and header permissions.

Each entry contains `name`, `score`, and an ISO 8601 UTC `date`. The server stores the top 10 entries in `scores.json` in the current working directory, replacing the file atomically.
