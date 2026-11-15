# Snake high-score backend

A single-file HTTP service for snake high scores. Python 3 standard library only.

## Run

```sh
python3 server.py                 # listens on http://127.0.0.1:8765
PORT=9000 python3 server.py       # or pick another port
```

The server binds to 127.0.0.1 only. Scores are stored in `scores.txt`, created
next to `server.py` on the first write (the file is git-ignored).

## API

Every response carries `Access-Control-Allow-Origin: *`.

### `POST /scores`

Plain-text body: `<name> <score>`, e.g. `alice 4200`.

- `name`: one or more non-whitespace characters.
- `score`: whole number, digits only (non-negative).
- Surrounding whitespace/newlines are ignored.

Responses: `204` (no body) when stored; `400` for any other body.

### `GET /scores`

Returns the ten best entries as plain text, highest score first, one
`<name> <score>` per line. Empty body (status `200`) when there are no scores.

### `OPTIONS <any path>`

`204` with `Access-Control-Allow-Origin: *`,
`Access-Control-Allow-Methods: GET, POST, OPTIONS` and
`Access-Control-Allow-Headers: Content-Type`.

### Other requests

Unknown paths answer `404`; wrong methods on `/scores` answer `405`.

## Example

```sh
curl -i -X POST --data 'alice 4200' http://127.0.0.1:8765/scores
curl http://127.0.0.1:8765/scores
```
