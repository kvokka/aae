# Browser Snake

## Prerequisites

- Python 3 for the static frontend server.
- Node.js 18 or newer and npm for the high-score backend.
- A modern browser with canvas and JavaScript support.

## Start the services

In one terminal, start the backend from the backend rig:

```sh
cd ../backend
npm start
```

The backend listens on `0.0.0.0:3000` at `http://localhost:3000`.

In another terminal, from this frontend rig, run the exact frontend command:

```sh
python3 -m http.server 5173 --bind 0.0.0.0
```

Open `http://localhost:5173`.

## Play and submit scores

Click **Start game**, then steer with the arrow keys (WASD also works). Eat food to grow and score. Hitting a wall or the snake ends the game; **Restart** begins a fresh game. The page prevents arrow keys from scrolling while playing. After a game ends, enter a name of 1–20 characters and explicitly submit the final score. The leaderboard displays the top ten scores from the server. If the backend is unavailable, the game stays playable and failed submissions can be retried.

## API contract

The frontend calls `http://localhost:3000/api/high-scores` cross-origin.

- `GET /api/high-scores` returns HTTP 200 and `{"scores":[{"name":"Ada","score":12}]}`; an empty store returns `{"scores":[]}`.
- `POST /api/high-scores` accepts `Content-Type: application/json` with `{"name":"Ada","score":12}`. Successful persistence returns HTTP 201 in the same response format.
- Names are trimmed strings of 1–20 characters. Scores are nonnegative safe integers. Invalid submissions return HTTP 400 and `{"error":"human-readable message"}`.
- The list is limited to ten entries in descending score order, preserving submission order for equal scores. Cross-origin GET, POST, and OPTIONS are supported; `Access-Control-Allow-Origin: *` is sufficient for this local app.

## Verification

1. Start the backend and frontend with the commands above and open the page. The ready overlay and **Start game** button should be visible; the high-score list should load, or show a clear unavailable status if the backend is stopped.
2. Click **Start game**. Confirm the overlay and its button are hidden, including in the computed styles, and take a screenshot of the unobstructed board. Use arrow keys to move for a few seconds, then take another screenshot while still playing and confirm the snake moved.
3. Restart a game and confirm the overlay stays hidden during play. Steer into a wall or the snake; confirm the game-over message and final score appear while **Start game** remains hidden.
4. Submit a name and confirm the finished score appears in the leaderboard. Restart the backend with its saved data intact, reload the page, and confirm that score remains visible.

The backend stores scores in `../backend/scores.json`; delete that file to clear the local leaderboard.
