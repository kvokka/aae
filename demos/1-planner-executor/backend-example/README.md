# Snake high-score backend

Run with `python3 backend/server.py` from the project directory, or with
`cd backend && python3 server.py`. The server listens on `127.0.0.1:8000`;
set `PORT` to use another port.

`GET /api/scores` returns up to 10 scores in descending order:

```sh
curl -i http://localhost:8000/api/scores
```

`POST /api/scores` adds a score. Names are trimmed and limited to 20 characters;
scores must be non-negative integers.

```sh
curl -i -X POST http://localhost:8000/api/scores \
  -H 'Content-Type: application/json' \
  -d '{"name":"ann","score":12}'
```

`OPTIONS /api/scores` is supported for CORS preflight requests:

```sh
curl -i -X OPTIONS http://localhost:8000/api/scores
```
