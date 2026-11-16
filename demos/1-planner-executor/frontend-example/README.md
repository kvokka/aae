# Snake

A browser Snake game with a high-score leaderboard. Plain HTML, CSS and vanilla JavaScript; no build step and no
npm packages.

## Run it

1. Start the backend first (in the backend rig):

   ```bash
   PORT=8000 python3 server.py
   ```

2. Serve this directory (in the frontend rig):

   ```bash
   python3 -m http.server 8080
   ```

3. Open <http://localhost:8080/index.html>.

The game talks to the backend at `API_BASE` (top of `game.js`, `http://localhost:8000`). If the backend is not
running, the game still works and the leaderboard shows "Leaderboard unavailable".

## Controls

- Arrow keys or W/A/S/D change direction. Reversing the current direction is ignored.
- The first arrow key press starts the game.
- After game over, enter a name (optional, max 20 characters) and press "Save score", or press "Play again".
