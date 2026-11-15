# Snake high-score API

Run the server with `python3 app.py` (default port 8000; set `PORT` to change it). Scores are stored in `scores.db` beside `app.py`.

Run tests with `python3 -m unittest -v`.

## API

- `GET /api/scores?limit=10` returns scores ordered by score descending, then creation time ascending. `limit` must be 1–100.
- `POST /api/scores` accepts JSON `{"name":"ann","score":12}`. Names are trimmed and must contain 1–20 characters; scores must be integers from 0 to 1,000,000. Returns the saved score.
- Unknown paths return 404. Invalid requests return 400. All responses include CORS headers; `OPTIONS` returns 204.
