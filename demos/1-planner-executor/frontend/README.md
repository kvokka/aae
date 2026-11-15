# Snake frontend

A browser Snake game with a high-score table. It is plain HTML, CSS and JavaScript with no build step, no npm
packages and no external CDN.

Files:

- `index.html` - the page: canvas, score, Start / restart button and the high-score list.
- `style.css` - styling.
- `game.js` - game logic, rendering and the calls to the backend. The backend address is the `API_BASE` constant at
  the top of the file (`http://127.0.0.1:8000`).

## Run

The high-score table needs the backend (see the `backend` rig). Start it first:

```sh
cd ../backend
python3 server.py
```

Then serve this directory and open the page:

```sh
cd /path/to/frontend
python3 -m http.server 8080
```

Open <http://127.0.0.1:8080/index.html> in a browser.

If the backend is not running, the game still works and the table shows "High scores unavailable".

## Play

- Arrow keys or WASD change direction. A reversal into the body is ignored.
- Eating food gives 10 points and makes the snake longer by one cell.
- Hitting a wall or the snake's own body ends the game. If the score is above 0, you are asked for a name (at most
  20 characters; an empty name is saved as `anonymous`). The score is then sent to the backend and the table is
  refreshed.
- Space or the Start / restart button starts a new game.
