# Snake score backend

A small HTTP server for storing and listing snake game scores. It uses only the
Python 3 standard library, is a single file (`server.py`), and stores scores in
`scores.txt` next to `server.py`.

## Run

```sh
PORT=8765 python3 server.py
```

It listens on `127.0.0.1` only. If `PORT` is not set, it uses `8765`.

## Endpoints

- `POST /scores`: plain-text body `<name> <score>`, where the name has no spaces and
  the score is a whole number, e.g. `smoke 4242`. Appends the line to `scores.txt`
  and answers `204`. A malformed body answers `400`.
- `GET /scores`: answers `200` with `text/plain; charset=utf-8`, containing the ten
  best `<name> <score>` lines, highest score first. Empty body when there are no scores.

Every response carries `Access-Control-Allow-Origin: *`. `OPTIONS` requests answer
`204` with CORS headers.
