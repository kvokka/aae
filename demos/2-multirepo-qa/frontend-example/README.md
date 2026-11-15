# Snake

## Prerequisite

Python 3 is required. The game uses plain HTML, CSS and JavaScript and has no build step.

## Start

From this frontend rig, serve the page with the exact command:

```sh
python3 -m http.server 5173 --bind 127.0.0.1
```

Open <http://127.0.0.1:5173> in a desktop browser.

Start the high-score backend separately from its rig:

```sh
cd ../backend
python3 server.py
```

The game remains playable when the backend is unavailable; scores need the API at `http://127.0.0.1:8000`.

## Play

Select **Start** and use the arrow keys to steer. Arrow keys are prevented from scrolling the page while the game is running. Eat the food to grow and earn one point. Hitting a wall or yourself ends the game; filling every cell wins. **Restart** resets the board at any time.

After a game ends, enter a name of 1–20 characters and select **Submit** to save that final score.

## Shared API

- `GET http://127.0.0.1:8000/api/scores` returns `{"scores":[{"name":"Ada","score":12}]}` with up to ten scores, highest first. An empty leaderboard has an empty `scores` array.
- `POST http://127.0.0.1:8000/api/scores` accepts JSON `{"name":"Ada","score":12}` and returns HTTP 201 with the updated scores. Names are trimmed and must contain 1–20 characters; score must be a non-negative integer. Invalid submissions return HTTP 400 and `{"error":"human-readable message"}`.
- The backend supports browser requests from `http://127.0.0.1:5173` and `http://localhost:5173`.

## End-to-end verification

1. Start `python3 server.py` in the backend rig and the static server command above in this rig.
2. Open the browser URL and check the leaderboard's loading/empty state, then select **Start**. Steer with arrow keys, eat food to check growth and scoring, and collide with a wall or yourself to finish.
3. Select **Restart** to verify a fresh game. Finish another game, enter a name, submit, and confirm the updated leaderboard and success message.
4. Reload and confirm the saved score remains. Stop the backend and reload to confirm the leaderboard shows an availability error while the game still starts and plays.
