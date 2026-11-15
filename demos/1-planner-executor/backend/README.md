# Snake high-score backend

Run the API with Python 3:

```sh
python3 server.py
```

The server listens on `127.0.0.1:8000`. Set `PORT` to use another port and
`SCORES_FILE` to choose the JSON storage file. The default storage path is
`./scores.json`; it is created on the first successful score submission.

## API

All responses use `application/json` and include CORS headers.

- `GET /api/scores` returns the top 10 entries, sorted by score descending and
  then by creation time ascending. An empty store returns `[]`.
- `POST /api/scores` accepts `{"name":"ann","score":42}` and returns the
  saved entry with status `201`. Names are trimmed and must contain 1 to 20
  characters. Scores must be integers from 0 to 1,000,000 inclusive.
- Invalid submissions return status `400` and `{"error":"..."}`. Unknown
  paths return `404`; unsupported methods on `/api/scores` return `405`.
- `OPTIONS` is supported on every path and returns `204`.

Run the standard-library test suite with:

```sh
python3 -m unittest discover -s tests -v
```
