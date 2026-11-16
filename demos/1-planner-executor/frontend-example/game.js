const API_BASE = 'http://localhost:8000';

const BOARD_CELLS = 20;
const CELL_PX = 20;
const TICK_MS = 120;
const POINTS_PER_FOOD = 10;
const MAX_NAME_LENGTH = 20;
const LEADERBOARD_LIMIT = 10;
const START_LENGTH = 3;
const SERVER_UNREACHABLE = 'Could not reach the server';

const DIR_UP = { x: 0, y: -1 };
const DIR_DOWN = { x: 0, y: 1 };
const DIR_LEFT = { x: -1, y: 0 };
const DIR_RIGHT = { x: 1, y: 0 };

const KEY_DIRS = {
  ArrowUp: DIR_UP,
  ArrowDown: DIR_DOWN,
  ArrowLeft: DIR_LEFT,
  ArrowRight: DIR_RIGHT,
  w: DIR_UP,
  s: DIR_DOWN,
  a: DIR_LEFT,
  d: DIR_RIGHT,
};

const canvas = document.getElementById('board');
const ctx = canvas.getContext('2d');
const scoreEl = document.getElementById('score');
const statusEl = document.getElementById('status');
const gameOverPanel = document.getElementById('game-over');
const gameOverText = document.getElementById('game-over-text');
const nameInput = document.getElementById('player-name');
const saveBtn = document.getElementById('save-btn');
const playAgainBtn = document.getElementById('play-again-btn');
const saveMessage = document.getElementById('save-message');
const leaderboardEl = document.getElementById('leaderboard');

canvas.width = BOARD_CELLS * CELL_PX;
canvas.height = BOARD_CELLS * CELL_PX;

// Game state: snake[0] is the head. `pending` holds the next direction
// chosen by a key press; it is applied on the next tick.
let state = null;
let timerId = null;

function sameCell(a, b) {
  return a.x === b.x && a.y === b.y;
}

function placeFood(snake) {
  const occupied = new Set(snake.map((c) => c.y * BOARD_CELLS + c.x));
  const free = [];
  for (let i = 0; i < BOARD_CELLS * BOARD_CELLS; i++) {
    if (!occupied.has(i)) free.push({ x: i % BOARD_CELLS, y: Math.floor(i / BOARD_CELLS) });
  }
  if (free.length === 0) return null;
  return free[Math.floor(Math.random() * free.length)];
}

function newGame() {
  stopTimer();
  const mid = Math.floor(BOARD_CELLS / 2);
  const snake = [];
  for (let i = 0; i < START_LENGTH; i++) {
    snake.push({ x: mid - i, y: mid });
  }
  state = {
    snake,
    dir: DIR_RIGHT,
    pending: null,
    food: placeFood(snake),
    score: 0,
    started: false,
    over: false,
    saved: false,
  };
  gameOverPanel.hidden = true;
  nameInput.value = '';
  saveMessage.textContent = '';
  saveBtn.disabled = false;
  scoreEl.textContent = String(state.score);
  statusEl.textContent = 'Press an arrow key to start';
  draw();
}

function startTimer() {
  timerId = setInterval(tick, TICK_MS);
}

function stopTimer() {
  if (timerId !== null) {
    clearInterval(timerId);
    timerId = null;
  }
}

function tick() {
  if (state.pending) {
    state.dir = state.pending;
    state.pending = null;
  }
  const head = state.snake[0];
  const next = { x: head.x + state.dir.x, y: head.y + state.dir.y };

  const outside =
    next.x < 0 || next.y < 0 || next.x >= BOARD_CELLS || next.y >= BOARD_CELLS;
  const eats = state.food !== null && sameCell(next, state.food);
  // The tail cell moves away on this tick unless the snake grows.
  const body = eats ? state.snake : state.snake.slice(0, -1);
  if (outside || body.some((c) => sameCell(c, next))) {
    endGame();
    return;
  }

  state.snake.unshift(next);
  if (eats) {
    state.score += POINTS_PER_FOOD;
    scoreEl.textContent = String(state.score);
    state.food = placeFood(state.snake);
    if (state.food === null) {
      endGame();
      return;
    }
  } else {
    state.snake.pop();
  }
  draw();
}

function endGame() {
  stopTimer();
  state.over = true;
  statusEl.textContent = '';
  gameOverText.textContent = `Game over, score ${state.score}`;
  gameOverPanel.hidden = false;
  draw();
}

function draw() {
  ctx.fillStyle = '#1b1b1b';
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  if (state.food) {
    ctx.fillStyle = '#e53935';
    ctx.fillRect(state.food.x * CELL_PX, state.food.y * CELL_PX, CELL_PX, CELL_PX);
  }

  ctx.fillStyle = '#43a047';
  for (const c of state.snake) {
    ctx.fillRect(c.x * CELL_PX + 1, c.y * CELL_PX + 1, CELL_PX - 2, CELL_PX - 2);
  }
}

function onKeyDown(event) {
  // Typing in the name field must not steer the snake.
  if (event.target instanceof HTMLInputElement) return;
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  const dir = KEY_DIRS[key];
  if (!dir || state.over) return;
  event.preventDefault();

  if (!state.started) {
    state.started = true;
    statusEl.textContent = '';
    startTimer();
  }
  // Reversing the current direction is ignored.
  const reverses = dir.x + state.dir.x === 0 && dir.y + state.dir.y === 0;
  if (!reverses) {
    state.pending = dir;
  }
}

async function loadLeaderboard() {
  try {
    const res = await fetch(`${API_BASE}/api/scores`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const rows = await res.json();
    if (!Array.isArray(rows)) throw new Error('unexpected payload');
    renderLeaderboard(rows.slice(0, LEADERBOARD_LIMIT));
  } catch (err) {
    leaderboardEl.textContent = 'Leaderboard unavailable';
  }
}

function renderLeaderboard(rows) {
  if (rows.length === 0) {
    leaderboardEl.textContent = 'No scores yet.';
    return;
  }
  const table = document.createElement('table');
  const thead = document.createElement('thead');
  const headRow = document.createElement('tr');
  for (const [label, cls] of [['Rank', ''], ['Name', ''], ['Score', 'num']]) {
    const th = document.createElement('th');
    th.textContent = label;
    if (cls) th.className = cls;
    headRow.appendChild(th);
  }
  thead.appendChild(headRow);
  table.appendChild(thead);

  const tbody = document.createElement('tbody');
  rows.forEach((row, index) => {
    const tr = document.createElement('tr');
    const rank = document.createElement('td');
    rank.textContent = String(index + 1);
    const name = document.createElement('td');
    name.textContent = String(row.name);
    const score = document.createElement('td');
    score.className = 'num';
    score.textContent = String(row.score);
    tr.append(rank, name, score);
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);
  leaderboardEl.replaceChildren(table);
}

async function saveScore() {
  const name = nameInput.value.trim().slice(0, MAX_NAME_LENGTH) || 'Anonymous';
  saveMessage.textContent = '';
  saveBtn.disabled = true;
  let message = null;
  try {
    const res = await fetch(`${API_BASE}/api/scores`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, score: state.score }),
    });
    if (res.status === 201) {
      state.saved = true;
      saveMessage.textContent = 'Score saved.';
      await loadLeaderboard();
      return;
    }
    message = SERVER_UNREACHABLE;
    try {
      const data = await res.json();
      if (data && typeof data.error === 'string') message = data.error;
    } catch (err) {
      // Non-JSON body: keep the generic message.
    }
  } catch (err) {
    message = SERVER_UNREACHABLE;
  }
  saveMessage.textContent = message;
  saveBtn.disabled = state.saved;
}

saveBtn.addEventListener('click', saveScore);
playAgainBtn.addEventListener('click', newGame);
document.addEventListener('keydown', onKeyDown);

newGame();
loadLeaderboard();
