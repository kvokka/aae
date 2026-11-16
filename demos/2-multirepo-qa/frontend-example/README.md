# Snake

A small browser Snake game with a persistent top-10 list supplied by the sibling backend rig. The frontend has no build step or package dependencies.

## Run

1. In this directory, start the frontend:

   ```sh
   python3 -m http.server 5173 --bind 127.0.0.1
   ```

2. In a second terminal, from the sibling `backend` rig, start the API:

   ```sh
   python3 server.py
   ```

3. Open <http://127.0.0.1:5173> (or <http://localhost:5173>).

No install or build command is needed. The frontend uses `http://127.0.0.1:8000` for the API. It remains playable if the backend is unavailable; use **Refresh** to retry loading scores and retry the save after a failed submission.

## Controls and scoring

- Press **Start game** or any arrow/WASD key to begin. Use the arrow keys or **W**, **A**, **S**, **D** to steer.
- Eat the red food to grow by one cell and earn **10 points**.
- Hitting a wall or the snake ends the game. Filling every cell wins.
- Choose **Restart** to reset the board and score.
- After a game ends, enter a name of 1–24 characters (surrounding spaces are trimmed) and submit the score.

## API contract

Base URL: `http://127.0.0.1:8000`. JSON responses use `Content-Type: application/json`.

- `GET /api/highscores` returns HTTP 200, for example `{"scores":[{"name":"Ada","score":30}]}`. It contains up to 10 entries, highest score first, with earlier submissions first for ties. An empty store returns `{"scores":[]}`.
- `POST /api/highscores` accepts `Content-Type: application/json` and a body such as `{"name":"Ada","score":30}`. It returns HTTP 201 and the stored entry, for example `{"name":"Ada","score":30}`.
- Names are trimmed and must contain 1–24 characters after trimming. Scores must be JSON integers (not booleans) from 0 through 2147483647. Invalid input or malformed JSON returns HTTP 400 with `{"error":"human-readable message"}`.
- CORS permits `http://127.0.0.1:5173` and `http://localhost:5173`, including the POST preflight.

## Manual verification checklist

- [ ] Start both commands and confirm the board, food, and top-10 list appear.
- [ ] Start with the button and with a direction key; verify arrows and WASD steer the snake.
- [ ] Eat food; verify the snake grows and the score increases by 10.
- [ ] Try a direct reverse and two quick turns before a movement step; verify the snake cannot reverse into itself.
- [ ] Hit a wall and the snake body; verify game-over state and score form.
- [ ] Restart after a game; verify a fresh snake and zero score with no accelerated/doubled movement.
- [ ] Submit a finished score; verify success feedback, refreshed ranking, and ranking order after another score.
- [ ] Restart the backend and reload the page; verify saved scores remain.
- [ ] Stop the backend; verify the game still plays and failed list/save requests show retry guidance. Restart it and use Refresh or resubmit.
