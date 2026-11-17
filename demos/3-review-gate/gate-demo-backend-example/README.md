# gate-demo-backend

A small HTTP backend for the greeting demo. It uses only the Python standard
library.

## Prerequisites

- Python 3.8 or newer
- `curl` (for the endpoint check)

## Start locally

From this directory, run:

```sh
python3 server.py
```

The API listens on `http://localhost:8000`. Leave that command running in its
terminal.

## Check the endpoint

In another terminal, run:

```sh
curl --get --data-urlencode 'name=Ada Lovelace' http://localhost:8000/greeting
```

Expected response:

```json
{"greeting": "Hello, Ada Lovelace!"}
```

The `name` query parameter is URL decoded as UTF-8. Requests from
`http://localhost:3000` are allowed by CORS.
