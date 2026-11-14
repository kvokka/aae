I want to play snake in my browser and keep a list of the best scores.

The game is one page, `index.html`, in plain JavaScript: no libraries, no build step. I draw the board on a
`<canvas>` and steer the snake with the arrow keys. When the game is over, it asks for my name, sends the score to
the backend and shows the ten best scores.

The backend is one file, `server.py`, for Python 3 with the standard library only. It listens on 127.0.0.1, on the
port in the `PORT` environment variable, 8765 when it is not set. It keeps the scores in a plain-text file,
`scores.txt`, next to `server.py`, one line per score:

- `POST /scores` takes a plain-text body `<name> <score>` (a name without spaces and a whole number), adds it to the
  file and answers 204; anything else gets 400;
- `GET /scores` answers in plain text with the ten best lines, the highest score first, and nothing when there are
  none.

I open the page straight from the file, so the backend must allow that: every answer says
`Access-Control-Allow-Origin: *`, and `OPTIONS` answers 204 allowing `GET`, `POST` and the `Content-Type` header.

The game and the backend live in separate repositories: the game in the frontend one, the backend in its own.

I will check it like this: start `server.py` with some `PORT`, post `smoke 4242` to `/scores`, find that line in
`scores.txt` and in `GET /scores`, and look into `index.html` for the canvas, the arrow keys and the `/scores` calls.
