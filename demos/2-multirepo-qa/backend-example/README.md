# Snake high score API

Requires Python 3. No third-party packages are needed.

## Start

From this directory, run:

```sh
python3 server.py
```

The API listens at `http://localhost:3000`. Scores are stored in `scores.json` beside `server.py`, regardless of the directory from which the server is started. The file is created with an empty list on first use and atomically replaced on updates.

## API

`GET /api/scores` returns the top ten scores in descending order:

```sh
curl -i http://localhost:3000/api/scores
```

An empty store returns `{"scores":[]}`. Submit a score with a trimmed name of 1–24 characters and a nonnegative integer score:

```sh
curl -i -H 'Content-Type: application/json' \
  -d '{"name":"Ada","score":12}' \
  http://localhost:3000/api/scores
```

Successful submissions return HTTP 201 and the updated leaderboard. Invalid requests return HTTP 400 with a JSON `error`. Browser requests from `http://localhost:5173` are allowed, including POST preflight.

## Verification

From this directory, with the server started using the command above, run these steps in another terminal:

1. For a fresh store, remove `scores.json` before starting the server. Run `curl -i http://localhost:3000/api/scores`; expect HTTP 200 and `{"scores": []}`.
2. Submit `curl -i -H 'Content-Type: application/json' -d '{"name":"Ada","score":12}' http://localhost:3000/api/scores`, then submit `curl -i -H 'Content-Type: application/json' -d '{"name":"Lin","score":20}' http://localhost:3000/api/scores`. Both return HTTP 201; the second response lists Lin before Ada.
3. Run `curl -i -H 'Content-Type: application/json' -d '{"name":"   ","score":-1}' http://localhost:3000/api/scores`; expect HTTP 400 and a JSON error.
4. Run `curl -i -X OPTIONS -H 'Origin: http://localhost:5173' -H 'Access-Control-Request-Method: POST' -H 'Access-Control-Request-Headers: Content-Type' http://localhost:3000/api/scores`; expect HTTP 204, `Access-Control-Allow-Origin: http://localhost:5173`, and the allowed method/header response headers.
5. Stop the server with Ctrl-C, start it again with `python3 server.py`, then run `curl -i http://localhost:3000/api/scores`; both submitted scores should still be present in descending order.
