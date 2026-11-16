# Snake high scores

Run the server with `PORT=8000 python3 server.py`. It listens on `127.0.0.1`; scores are stored in `./scores.json` by default. Set `SCORES_FILE` to choose another file.

Check the API with `curl -s localhost:8000/api/scores`, then submit a score with `curl -s -X POST -H 'Content-Type: application/json' -d '{"name":"ann","score":42}' localhost:8000/api/scores`. Run the tests with `python3 -m unittest -q`.
