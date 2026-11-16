# Snake high-score API

This is a small Node.js HTTP API for the browser snake game. It uses only Node built-in modules and stores scores in a local JSON file.

## Requirements and commands

Install Node.js 18 or newer. From this directory, start the API with:

```sh
npm start
```

It listens on `http://localhost:3000`. Run the built-in test suite with:

```sh
npm test
```

## API

Get the top ten scores:

```sh
curl http://localhost:3000/api/scores
```

Submit a score (the name is trimmed; names must be 1–20 characters and scores nonnegative safe integers):

```sh
curl -X POST http://localhost:3000/api/scores \
  -H 'Content-Type: application/json' \
  -d '{"name":"Ada","score":12}'
```

The POST response is the accepted `{ "name": "Ada", "score": 12 }`, even when it does not place in the top ten. Invalid requests return HTTP 400 and a JSON `error`. Browser requests from `http://localhost:5173` are allowed for GET, POST, and OPTIONS.

## Storage and frontend

Scores persist in `data/scores.json` under this backend directory. The `data/` directory is excluded from git. To reset scores, stop the server and remove `data/scores.json` (or remove the whole `data/` directory); the next request starts with an empty score list.

Run the frontend separately at `http://localhost:5173` and this API with `npm start` in the backend directory. The frontend should call `http://localhost:3000/api/scores`.
