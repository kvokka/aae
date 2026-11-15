# Snake frontend

A browser Snake game with a top-10 high-score table. Plain HTML, CSS and JavaScript: no framework, no build step, no npm.

## Run

The frontend and the backend are two separate servers.

1. Start the backend in the backend repository (it serves the API on `http://localhost:8000`):

   ```sh
   python3 app.py
   ```

2. Start the frontend from this repository's root (port 8080):

   ```sh
   python3 -m http.server 8080
   ```

3. Open <http://localhost:8080/> in a desktop browser.

The game still works if the backend is not running. The high-score table then shows
"High scores unavailable (backend offline)", and saving a score keeps it on screen so you can retry once the backend is back.

The API address is the `API_BASE` constant at the top of `game.js`.

## Play

- Arrow keys or `W` `A` `S` `D` change direction. You cannot reverse straight into your own body.
- `P` pauses and resumes.
- Space or Enter starts a game. Eat the red food to grow and score 1 point per food.
- The game ends when the snake hits a wall or itself.
- On game over, type a name (max 20 characters) and press **Save score** or Enter. Press **Skip** to discard the score.
  The name is remembered in the browser for next time.

## Check

With the backend running, play a game to game over, save the score, and reload the page: the score should appear in the table.
Run `node --check game.js` for a quick syntax check.
