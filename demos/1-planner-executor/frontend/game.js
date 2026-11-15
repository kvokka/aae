// Snake: board logic, rendering, keyboard input and the high-score calls.

const API_BASE = "http://127.0.0.1:8000";

const GRID = 20; // cells per side
const CELL = 20; // canvas pixels per cell
const TICK_MS = 120;
const START_LENGTH = 3;
const FOOD_POINTS = 10;
const MAX_NAME_LENGTH = 20;
const DEFAULT_NAME = "anonymous";
const MAX_QUEUED_TURNS = 2;
const UNAVAILABLE_TEXT = "High scores unavailable";

const KEY_DIRECTIONS = {
  ArrowUp: { x: 0, y: -1 },
  w: { x: 0, y: -1 },
  ArrowDown: { x: 0, y: 1 },
  s: { x: 0, y: 1 },
  ArrowLeft: { x: -1, y: 0 },
  a: { x: -1, y: 0 },
  ArrowRight: { x: 1, y: 0 },
  d: { x: 1, y: 0 },
};

const canvas = document.getElementById("board");
const ctx = canvas.getContext("2d");
const scoreEl = document.getElementById("score");
const startButton = document.getElementById("start");
const statusEl = document.getElementById("status");
const scoresStatusEl = document.getElementById("scores-status");
const scoresList = document.getElementById("scores");

canvas.width = GRID * CELL;
canvas.height = GRID * CELL;

// snake[0] is the head. dir is the direction of the last move; queue holds
// turns accepted from the keyboard but not yet applied.
const state = {
  snake: [],
  dir: { x: 1, y: 0 },
  queue: [],
  food: null,
  score: 0,
  running: false,
  timer: null,
};

function sameCell(a, b) {
  return a !== null && b !== null && a.x === b.x && a.y === b.y;
}

function resetBoard() {
  const mid = Math.floor(GRID / 2);
  state.snake = [];
  for (let i = 0; i < START_LENGTH; i++) {
    state.snake.push({ x: mid - i, y: mid });
  }
  state.dir = { x: 1, y: 0 };
  state.queue = [];
  state.score = 0;
  state.food = placeFood();
  scoreEl.textContent = "0";
}

// A random free cell, or null when the snake fills the board.
function placeFood() {
  const taken = new Set(state.snake.map((s) => s.y * GRID + s.x));
  const free = [];
  for (let i = 0; i < GRID * GRID; i++) {
    if (!taken.has(i)) {
      free.push(i);
    }
  }
  if (free.length === 0) {
    return null;
  }
  const index = free[Math.floor(Math.random() * free.length)];
  return { x: index % GRID, y: Math.floor(index / GRID) };
}

function startGame() {
  clearTimeout(state.timer);
  resetBoard();
  state.running = true;
  setStatus("Game on. Use the arrow keys or WASD.");
  render();
  state.timer = setTimeout(tick, TICK_MS);
}

function tick() {
  if (!state.running) {
    return;
  }
  if (state.queue.length > 0) {
    state.dir = state.queue.shift();
  }
  const head = state.snake[0];
  const next = { x: head.x + state.dir.x, y: head.y + state.dir.y };
  const eating = sameCell(next, state.food);

  // The tail moves away this tick unless the snake grows, so its cell is free.
  const body = eating ? state.snake : state.snake.slice(0, -1);
  const outside = next.x < 0 || next.y < 0 || next.x >= GRID || next.y >= GRID;
  if (outside || body.some((s) => sameCell(s, next))) {
    endGame();
    return;
  }

  state.snake.unshift(next);
  if (eating) {
    state.score += FOOD_POINTS;
    scoreEl.textContent = String(state.score);
    state.food = placeFood();
    if (state.food === null) {
      // The board is full: nothing is left to eat.
      render();
      endGame();
      return;
    }
  } else {
    state.snake.pop();
  }
  render();
  state.timer = setTimeout(tick, TICK_MS);
}

function endGame() {
  state.running = false;
  clearTimeout(state.timer);
  render();
  setStatus(`Game over. Score: ${state.score}.`);
  if (state.score > 0) {
    askNameAndSubmit();
  }
}

function queueTurn(dir) {
  const last = state.queue.length > 0 ? state.queue[state.queue.length - 1] : state.dir;
  // Ignore a repeat of the current heading and a 180-degree reversal.
  if (dir.x === last.x && dir.y === last.y) {
    return;
  }
  if (dir.x === -last.x && dir.y === -last.y) {
    return;
  }
  if (state.queue.length < MAX_QUEUED_TURNS) {
    state.queue.push(dir);
  }
}

function normalizeName(raw) {
  const trimmed = (raw || "").trim();
  const limited = Array.from(trimmed).slice(0, MAX_NAME_LENGTH).join("").trim();
  return limited || DEFAULT_NAME;
}

async function askNameAndSubmit() {
  const answer = window.prompt(`Game over! Your score is ${state.score}. Enter your name:`, "");
  await submitScore(normalizeName(answer), state.score);
}

async function submitScore(name, score) {
  setStatus("Saving score...");
  let response;
  try {
    response = await fetch(`${API_BASE}/api/scores`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, score }),
    });
  } catch (err) {
    showUnavailable();
    setStatus("Score not saved: high scores are unavailable.");
    return;
  }
  if (!response.ok) {
    setStatus(`Score not saved: ${await errorMessage(response)}`);
    return;
  }
  setStatus("Score saved.");
  await loadScores();
}

async function loadScores() {
  let response;
  try {
    response = await fetch(`${API_BASE}/api/scores`);
  } catch (err) {
    showUnavailable();
    return;
  }
  if (!response.ok) {
    scoresList.replaceChildren();
    setScoresStatus(await errorMessage(response));
    return;
  }
  renderScores(await response.json());
}

async function errorMessage(response) {
  let message = `request failed with status ${response.status}.`;
  try {
    const data = await response.json();
    if (data && typeof data.error === "string") {
      message = data.error;
    }
  } catch (err) {
    // The body is not JSON; keep the generic message.
  }
  return message;
}

function renderScores(entries) {
  scoresList.replaceChildren();
  for (const entry of entries) {
    const item = document.createElement("li");
    const name = document.createElement("span");
    name.textContent = entry.name;
    const score = document.createElement("span");
    score.textContent = String(entry.score);
    item.append(name, score);
    scoresList.append(item);
  }
  setScoresStatus(entries.length === 0 ? "No scores yet." : "");
}

function showUnavailable() {
  scoresList.replaceChildren();
  setScoresStatus(UNAVAILABLE_TEXT);
}

function setScoresStatus(text) {
  scoresStatusEl.textContent = text;
}

function setStatus(text) {
  statusEl.textContent = text;
}

function render() {
  ctx.fillStyle = "#1b1f24";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  if (state.food !== null) {
    ctx.fillStyle = "#e5534b";
    ctx.fillRect(state.food.x * CELL + 2, state.food.y * CELL + 2, CELL - 4, CELL - 4);
  }
  state.snake.forEach((cell, i) => {
    ctx.fillStyle = i === 0 ? "#7ee787" : "#3fb950";
    ctx.fillRect(cell.x * CELL + 1, cell.y * CELL + 1, CELL - 2, CELL - 2);
  });
}

document.addEventListener("keydown", (event) => {
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  if (key === " ") {
    event.preventDefault();
    if (!event.repeat) {
      startGame();
    }
    return;
  }
  const dir = KEY_DIRECTIONS[key];
  if (dir === undefined) {
    return;
  }
  event.preventDefault();
  if (state.running) {
    queueTurn(dir);
  }
});

startButton.addEventListener("click", startGame);

resetBoard();
render();
setStatus("Press Start or Space to play.");
loadScores();
