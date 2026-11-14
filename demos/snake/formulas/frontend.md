You are building the frontend of a browser snake game in {{ rig.path }}, an empty directory.

Write one file, `index.html`, with plain JavaScript in it: no libraries, no build step, nothing loaded from the
network except the backend below.

- The game: a snake on a `<canvas>` grid, steered with the arrow keys (`ArrowUp` and so on), grows by eating food,
  ends when it hits a wall or itself; the score is the food eaten. Space starts a new game.
- The backend keeps the best scores at `http://127.0.0.1:{{ inputs.port }}`: `GET {{ inputs.get }}` returns them as
  plain text, one `<name> <score>` per line, best first; `POST {{ inputs.post }}` with a plain-text body
  `<name> <score>` stores one (the name has no spaces).
- At game over, ask for a name, post the score, then show the best scores beside the board; show them on load too.
  When the backend is down, the game still plays.

When `index.html` is written, run `aae finish pass --summary "index.html plays snake"`.
If you cannot, run `aae finish fail --summary "<why>"`.
