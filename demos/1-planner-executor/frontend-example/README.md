# Snake frontend

A browser Snake game that stores high scores in the backend. Plain HTML, CSS and JavaScript: no build step, no
packages.

Files:

- `index.html` — the page: canvas, score, high-score list, Start/Restart button.
- `style.css` — styling.
- `game.js` — game logic and API calls. The backend URL is the `API_BASE_URL` constant at its top.

## Run

The game needs the backend, which listens on `http://localhost:8000`. Start it from the project root:

```sh
python3 backend/server.py
```

Then serve the frontend from its directory and open the page:

```sh
cd frontend && python3 -m http.server 8080
```

Open http://localhost:8080.

## Play

- Arrow keys or WASD change direction; reversing into the body is ignored.
- Space or Enter starts a game, and restarts it after game over.
- After a game with score above 0, enter a name; it is saved to the high-score list. An empty name becomes
  `anonymous`.
- If the backend is down, the game still plays and the high-score area shows "High scores unavailable".
