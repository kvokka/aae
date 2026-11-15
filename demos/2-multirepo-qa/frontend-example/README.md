# Snake

## Prerequisites

- Python 3 for the static frontend server.
- Python 3 for the score API. The API uses only the Python standard library.

## Start

From this directory, serve the frontend with:

```sh
python3 -m http.server 5173 --bind localhost
```

In a second terminal, start the backend from `../backend`:

```sh
cd ../backend
python3 server.py
```

Open the frontend at <http://localhost:5173>. The API listens at <http://localhost:3000>.
The game remains playable when the API is unavailable; leaderboard loading and score submission need it.

## Controls

Select **Start**, then steer with the arrow keys. Eat food to grow and score. Hitting a wall or the snake ends the round. Select **Restart** to reset.

## Manual verification

1. Start the backend and frontend with the commands above and open the frontend URL.
2. Confirm the leaderboard loads, select Start, and steer with the arrow keys. Confirm the snake moves, eating food increases the score, and a wall or self collision shows Game over.
3. Enter a name at game over and submit. Confirm the success message and that the new score appears in the visible leaderboard.
4. Refresh the page and confirm the score remains in the leaderboard. Select Restart and confirm a new round starts with score zero.
