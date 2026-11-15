# Snake high-score backend

This backend uses only the Python 3 standard library. It listens on
`127.0.0.1:8000`. Set `PORT` to choose another port and `SCORES_FILE` to choose
the JSON file used to store scores. By default, scores are stored in
`scores.json` in the current working directory.

Start the server from this directory:

```sh
python3 server.py
```

List the top scores:

```sh
curl -i 'http://127.0.0.1:8000/api/scores'
```

Add a score:

```sh
curl -i -X POST http://127.0.0.1:8000/api/scores \
  -H 'Content-Type: application/json' \
  -d '{"name":"Ada","score":42}'
```

Run the standard-library test suite:

```sh
python3 -m unittest discover -s tests -v
```
