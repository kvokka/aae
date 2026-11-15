# Snake backend

A small score server for the browser snake game. Standard library only
(Python 3, no packages, no build step).

## Run

```sh
python3 server.py                 # listens on 127.0.0.1:8765
PORT=8799 python3 server.py       # or pick another port
```

The server binds to `127.0.0.1` only.

## Storage

Scores live in `scores.txt`, next to `server.py`. One line per score:

```
<name> <score>
```

The file is created on the first write. A missing file means no scores.

## API

All responses carry `Access-Control-Allow-Origin: *`.

### `POST /scores`

Plain-text body: `<name> <score>`

- `name`: non-empty, contains no whitespace.
- `score`: whole number, digits only (`0`, `4242`, ...).

Responses:
- `204 No Content` — valid; the line is appended to `scores.txt`.
- `400 Bad Request` — anything else (missing or extra parts, empty name,
  non-integer or negative score, bad encoding).

### `GET /scores`

Returns `200 OK`, `Content-Type: text/plain`. The body is the ten best lines
from `scores.txt`, highest score first, one per line. Ties keep file order
(earlier-written first). With no scores the body is empty.

### `OPTIONS /scores`

Returns `204 No Content` with:
- `Access-Control-Allow-Methods: GET, POST, OPTIONS`
- `Access-Control-Allow-Headers: Content-Type`

### Other paths and methods

Unknown paths return `404`. `PUT`, `PATCH` and `DELETE` return `405`. Other
unsupported methods return `501` from the standard library. Error responses
carry the CORS header too.

## Example

```sh
curl -i -X POST --data 'alice 120' http://127.0.0.1:8765/scores
curl -i http://127.0.0.1:8765/scores
```
