# Snake

A browser snake game with a high-score leaderboard. Plain HTML, CSS and vanilla JavaScript: no build step, no npm, no external libraries.

## Files

- `index.html` — page layout: canvas, score line, name form and leaderboard.
- `style.css` — styling.
- `game.js` — game logic and calls to the score API.

## Running

The game needs the score backend from the `backend` rig. Start both servers.

1. Start the backend (listens on `127.0.0.1:8080`):

   ```sh
   cd ../backend
   python3 server.py
   ```

2. In this folder, serve the static files:

   ```sh
   python3 -m http.server 8000
   ```

3. Open <http://localhost:8000> in a browser.

The game still plays if the backend is not running; the leaderboard then shows "Scores unavailable (backend is not running)".

The API base URL is the `API_BASE_URL` constant at the top of `game.js`.

## Controls

- Arrow keys or `W` `A` `S` `D` — change direction. Reversing directly (180 degrees) is ignored.
- The game starts on the first direction key press.
- `Space` or `Enter` — start a new game (also after game over).

## Rules

- The board is 20 x 20 cells. The snake starts with length 3, in the middle, moving right.
- The snake moves one cell every 120 ms.
- Eating food grows the snake by one cell and adds 1 to the score.
- The game is over when the snake hits a wall or its own body.

## Saving a score

After game over with a score above 0, enter a name (up to 20 characters) and press **Save** or `Enter`. The name is remembered in `localStorage` for next time. The leaderboard shows the top 10 scores and highlights the new entry.
