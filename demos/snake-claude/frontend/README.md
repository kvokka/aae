# Snake (frontend)

A single-file browser snake game: `index.html`. Plain HTML, CSS and JavaScript, no libraries and no build step.

## Open the game

Open `index.html` directly in a browser (double-click it, or File > Open). No server is needed for the page itself.

## Start the backend (optional, for the high-score list)

The score list talks to the backend in the separate backend repository:

```sh
cd <backend-repo>
PORT=8765 python3 server.py
```

The page calls `http://127.0.0.1:8765` (fixed). The game is playable without the backend; the score list then shows an error message instead.

## Controls

- Arrow keys: steer. Reversing directly into the snake is ignored. Arrow keys do not scroll the page.
- Enter or Space: start a game (on the start screen) and start a new game after game over.
- "New game" button: same as Enter/Space after game over.

## Scores

- When the game ends, you are asked for a name. Spaces are replaced with `_`, and an empty name asks again. Cancelling the prompt skips submission.
- The score is sent as `POST http://127.0.0.1:8765/scores` with `Content-Type: text/plain` and the body `<name> <score>`.
- The top ten scores are fetched with `GET http://127.0.0.1:8765/scores` (plain text, one `<name> <score>` per line, highest first) and shown on the page.

The page is opened from `file://`, so the backend must allow cross-origin requests (`Access-Control-Allow-Origin`). The SnakeScores backend sends `Access-Control-Allow-Origin: *`.
