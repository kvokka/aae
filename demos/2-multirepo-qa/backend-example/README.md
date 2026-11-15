# Snake high-score API

## Prerequisites

Node.js 18 or newer and npm. The service uses only Node.js built-in modules; no install step is needed.

## Start

From this directory, run:

```sh
npm start
```

The API listens on `0.0.0.0:3000`, available locally at `http://localhost:3000`. Its JSON data file is `scores.json` in this backend directory, independent of the shell's current directory. The file is created on first access and persists across restarts. It is ignored by git.

## API

`GET /api/high-scores` returns HTTP 200 with the top ten scores, in descending score order:

```sh
curl http://localhost:3000/api/high-scores
```

```json
{"scores":[{"name":"Ada","score":12}]}
```

An empty store returns `{"scores":[]}`. Submit a score with JSON:

```sh
curl -X POST http://localhost:3000/api/high-scores \
  -H 'Content-Type: application/json' \
  -d '{"name":"Ada","score":12}'
```

Successful persistence returns HTTP 201 in the same response format, with the updated top ten. Names are trimmed and must contain 1 to 20 characters; scores must be nonnegative safe integers. Invalid requests return HTTP 400 with `{"error":"human-readable message"}`. Cross-origin GET, POST, and OPTIONS requests are allowed for the frontend at `http://localhost:5173`.

## Checks

Run `npm run check` to exercise empty GET, valid and invalid submissions, CORS preflight, sorting and top-ten limits, and persistence across a server restart. The check starts and stops its own server and removes its generated score file.
