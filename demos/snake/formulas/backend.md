You are building the backend of a browser snake game in {{ rig.path }}, an empty directory.

Write one file, `server.py`, for Python 3 with the standard library only:

- it listens on 127.0.0.1, on the port in the `PORT` environment variable, 8765 when `PORT` is unset;
- `POST /scores` takes a plain-text body `<name> <score>` (a name without spaces, a non-negative integer), appends
  it as one line to `scores.txt` next to `server.py` and answers 204; a malformed body gets 400;
- `GET /scores` answers `text/plain`: the 10 best lines of `scores.txt`, highest score first, nothing when there are
  none;
- every answer carries `Access-Control-Allow-Origin: *`, and `OPTIONS` answers 204 with
  `Access-Control-Allow-Methods: GET, POST` and `Access-Control-Allow-Headers: Content-Type`, so a page opened from
  a file can call it.

Try it: start it on another port, post a score, read the scores, then stop the server. Leave no server running and
no `scores.txt` behind.

When it works, run exactly:

    aae finish pass --summary "server.py keeps the best scores" --data '{"port": 8765, "get": "/scores", "post": "/scores"}'

If you cannot make it work, run `aae finish fail --summary "<why>"`.
