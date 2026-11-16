# Snake Club

A small browser Snake game with a persistent top ten leaderboard.

## Requirements and start

Install Node.js 18 or newer. In this directory, run:

```sh
npm start
```

Open <http://localhost:5173>. The frontend uses no install step or third party runtime dependencies. Start the API separately with `npm start` in the sibling `backend` rig; it listens on <http://localhost:3000> and stores scores in `backend/data/scores.json`.

## Controls and scoring

Press **Start game** or any arrow/WASD key to begin. Use arrow keys or WASD to steer. The snake moves at a steady pace, grows by one cell for every food dot, and earns one point per dot. The snake cannot reverse into itself. Hitting a wall or its body ends the run. Filling every cell wins. Press **Restart game** to reset the board and score. Enter a name (1–20 characters) and explicitly submit after game over to save a score.

## Verification checklist

With both servers running, confirm the board draws a snake and food; use arrows and WASD to steer; collect a dot and confirm the snake grows and the score increments; try a reverse direction and confirm it is ignored; collide with a wall or the snake to see game over; restart and confirm score and board reset without the game moving faster; submit a name and score; refresh and confirm the score remains in the leaderboard. Restart the backend and refresh again to check persistence. Stop the backend and reload the frontend: scores should show an understandable unavailable message while the game remains playable. Start the backend again when finished. The game uses a small fixed board, so filling every cell can also verify the win state.
