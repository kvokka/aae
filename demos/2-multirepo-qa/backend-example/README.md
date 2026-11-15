# Snake high-score backend

Requires Python 3. Start the backend from this rig with:

```sh
python3 server.py
```

The API listens at `http://127.0.0.1:8000`.

## API

Fetch the current top ten:

```sh
curl http://127.0.0.1:8000/api/scores
```

Submit a score (names are trimmed; scores must be non-negative integers):

```sh
curl -X POST http://127.0.0.1:8000/api/scores \
  -H 'Content-Type: application/json' \
  -d '{"name":"Ada","score":12}'
```

Both successful responses contain `{"scores":[{"name":"Ada","score":12}]}`. Invalid submissions return HTTP 400 with an `error` message.

Scores are stored in `scores.json` beside `server.py`. To reset the leaderboard, stop the server and remove that file:

```sh
rm scores.json
```

## Frontend and verification

The separately served frontend connects to `http://127.0.0.1:8000`. From the frontend rig, start it with:

```sh
python3 -m http.server 5173 --bind 127.0.0.1
```

Then open `http://127.0.0.1:5173` in a browser. The API permits browser requests from `127.0.0.1:5173` and `localhost:5173`.

Run the repeatable API, validation, CORS, ordering, top-ten, and restart persistence checks with:

```sh
python3 -m unittest test_api.py
```

The check uses a temporary score file and removes it when finished.
