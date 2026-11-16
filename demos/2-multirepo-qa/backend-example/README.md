# Snake high-score API

A small persistent API using only Python 3's standard library and SQLite. No packages or external services are needed.

## Start

From this directory, run:

```sh
python3 server.py
```

The server binds to `127.0.0.1:8000`. Scores are stored in `highscores.sqlite3` beside `server.py` and remain after server restarts. The database is created on first use and is excluded from git. Set `SNAKE_HIGHSCORES_DB` to use a different database path.

## API

`GET /api/highscores` returns HTTP 200 and up to ten entries, ordered by score descending and then submission order ascending. An empty store returns `{"scores":[]}`.

`POST /api/highscores` accepts `Content-Type: application/json` and `{"name":"Ada","score":30}`. Names are trimmed and must contain 1–24 characters. Scores must be JSON integers from 0 through 2147483647 (booleans are not accepted). A successful submission returns HTTP 201 with the saved name and score. Invalid fields or malformed JSON return HTTP 400 with `{"error":"human-readable message"}`. JSON responses use `Content-Type: application/json`.

CORS allows `http://127.0.0.1:5173` and `http://localhost:5173`, including `OPTIONS` preflight for JSON POST requests. There is no authentication.

Examples:

```sh
curl http://127.0.0.1:8000/api/highscores
curl -i -X POST http://127.0.0.1:8000/api/highscores \
  -H 'Content-Type: application/json' \
  -d '{"name":"Ada","score":30}'
```

Unknown paths return 404 and unsupported methods return 405.

## Automated checks

Run the focused checks with:

```sh
python3 -m unittest -v
```

They cover empty results, valid submissions, top-ten ordering and ties, invalid input, database persistence after reopening storage, and CORS preflight. Tests use a temporary database and remove it when finished.

## Run with the browser frontend

In this backend rig, start the API with `python3 server.py`. In the sibling frontend rig, run `python3 -m http.server 5173 --bind 127.0.0.1`, then open <http://127.0.0.1:5173>.
