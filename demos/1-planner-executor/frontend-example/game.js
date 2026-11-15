// Snake: canvas board, keyboard controls, and a high-score table served by the backend API.
"use strict";

const API_BASE = "http://localhost:8000";

const COLS = 20;
const ROWS = 20;
const CELL = 24;
const TICK_MS = 120;
const NAME_KEY = "snake.name";
const NAME_MAX = 20;
const SCORE_LIMIT = 10;
const REQUEST_TIMEOUT_MS = 5000;

const UP = { x: 0, y: -1 };
const DOWN = { x: 0, y: 1 };
const LEFT = { x: -1, y: 0 };
const RIGHT = { x: 1, y: 0 };

const KEY_DIRECTIONS = {
  arrowup: UP,
  w: UP,
  arrowdown: DOWN,
  s: DOWN,
  arrowleft: LEFT,
  a: LEFT,
  arrowright: RIGHT,
  d: RIGHT,
};

const canvas = document.getElementById("board");
const ctx = canvas.getContext("2d");
const scoreEl = document.getElementById("score");
const gameOverPanel = document.getElementById("game-over");
const finalScoreEl = document.getElementById("final-score");
const nameInput = document.getElementById("name");
const saveBtn = document.getElementById("save");
const skipBtn = document.getElementById("skip");
const saveStatusEl = document.getElementById("save-status");
const scoresStatusEl = document.getElementById("scores-status");
const scoresTable = document.getElementById("scores-table");
const scoresBody = document.getElementById("scores-body");

// state: "ready" (waiting to start) | "running" | "paused" | "over"
let state = "ready";
let snake = [];
let dir = RIGHT;
let nextDir = RIGHT;
let food = null;
// One record per game. Lets a late save response know whether its game is still on screen.
let runCounter = 0;
let run = makeRun();

function makeRun() {
  runCounter += 1;
  return { id: runCounter, score: 0, saved: false, saving: false };
}

function resetSnake() {
  const midX = Math.floor(COLS / 2);
  const midY = Math.floor(ROWS / 2);
  snake = [
    { x: midX, y: midY },
    { x: midX - 1, y: midY },
    { x: midX - 2, y: midY },
  ];
  dir = RIGHT;
  nextDir = RIGHT;
  food = randomFreeCell();
}

function newGame() {
  run = makeRun();
  resetSnake();
  state = "running";
  hideGameOver();
  updateScore();
  draw();
}

function endGame() {
  state = "over";
  draw();
  showGameOver();
}

function updateScore() {
  scoreEl.textContent = `Score: ${run.score}`;
}

function randomFreeCell() {
  const occupied = new Set(snake.map((s) => s.y * COLS + s.x));
  const free = [];
  for (let i = 0; i < COLS * ROWS; i += 1) {
    if (!occupied.has(i)) {
      free.push({ x: i % COLS, y: Math.floor(i / COLS) });
    }
  }
  if (free.length === 0) {
    return null;
  }
  return free[Math.floor(Math.random() * free.length)];
}

function tick() {
  if (state !== "running") {
    return;
  }
  dir = nextDir;
  const head = snake[0];
  const next = { x: head.x + dir.x, y: head.y + dir.y };

  if (next.x < 0 || next.x >= COLS || next.y < 0 || next.y >= ROWS) {
    endGame();
    return;
  }

  const eats = food !== null && next.x === food.x && next.y === food.y;
  // The tail moves away this tick unless the snake grows, so it is not a collision.
  const body = eats ? snake : snake.slice(0, -1);
  if (body.some((s) => s.x === next.x && s.y === next.y)) {
    endGame();
    return;
  }

  snake.unshift(next);
  if (eats) {
    run.score += 1;
    updateScore();
    food = randomFreeCell();
    if (food === null) {
      // The snake fills the whole board: nothing left to eat or move into.
      endGame();
      return;
    }
  } else {
    snake.pop();
  }
  draw();
}

function setDirection(d) {
  // A direct reversal would run the head into the neck, so it is ignored.
  // nextDir is always perpendicular to dir, so checking against dir is enough.
  if (d.x === -dir.x && d.y === -dir.y) {
    return;
  }
  nextDir = d;
}

function togglePause() {
  if (state === "running") {
    state = "paused";
  } else if (state === "paused") {
    state = "running";
  }
  draw();
}

function draw() {
  ctx.fillStyle = "#101418";
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  if (food !== null) {
    ctx.fillStyle = "#ff5c5c";
    ctx.fillRect(food.x * CELL + 4, food.y * CELL + 4, CELL - 8, CELL - 8);
  }

  snake.forEach((s, i) => {
    ctx.fillStyle = i === 0 ? "#b6f2a1" : "#4caf50";
    ctx.fillRect(s.x * CELL + 1, s.y * CELL + 1, CELL - 2, CELL - 2);
  });

  if (state === "ready") {
    drawOverlay("Press Space or Enter to start");
  } else if (state === "paused") {
    drawOverlay("Paused: press P to resume");
  } else if (state === "over") {
    drawOverlay("Game over: Space or Enter to play again");
  }
}

function drawOverlay(text) {
  const width = canvas.width;
  const height = canvas.height;
  ctx.fillStyle = "rgba(16, 20, 24, 0.7)";
  ctx.fillRect(0, height / 2 - 28, width, 56);
  ctx.fillStyle = "#e6edf3";
  ctx.font = "bold 18px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(text, width / 2, height / 2);
}

function showGameOver() {
  finalScoreEl.textContent = String(run.score);
  nameInput.value = loadName();
  setSaveStatus("");
  saveBtn.disabled = false;
  skipBtn.disabled = false;
  gameOverPanel.hidden = false;
  nameInput.focus();
}

function hideGameOver() {
  gameOverPanel.hidden = true;
}

function setSaveStatus(text) {
  saveStatusEl.textContent = text;
}

function loadName() {
  try {
    return (localStorage.getItem(NAME_KEY) || "").slice(0, NAME_MAX);
  } catch {
    return "";
  }
}

function storeName(name) {
  try {
    localStorage.setItem(NAME_KEY, name);
  } catch {
    // Storage may be disabled (private mode); the name is simply not remembered.
  }
}

async function saveScore() {
  const game = run;
  if (state !== "over" || game.saved || game.saving) {
    return;
  }
  const name = nameInput.value.trim();
  if (name.length < 1 || name.length > NAME_MAX) {
    setSaveStatus(`Enter a name of 1 to ${NAME_MAX} characters.`);
    nameInput.focus();
    return;
  }

  game.saving = true;
  saveBtn.disabled = true;
  skipBtn.disabled = true;
  setSaveStatus("Saving...");

  let saved = false;
  let errorText = "";
  try {
    const res = await fetch(`${API_BASE}/api/scores`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, score: game.score }),
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
    let data = null;
    try {
      data = await res.json();
    } catch {
      data = null;
    }
    if (res.status === 201) {
      saved = true;
    } else {
      errorText = data && typeof data.error === "string" ? data.error : `server responded ${res.status}`;
    }
  } catch {
    errorText = "backend unreachable";
  }

  game.saving = false;
  if (saved) {
    game.saved = true;
    storeName(name);
    if (game.id === run.id) {
      hideGameOver();
    }
    loadScores();
  } else if (game.id === run.id) {
    setSaveStatus(`Could not save score (${errorText}). Your score is kept: try again.`);
    saveBtn.disabled = false;
    skipBtn.disabled = false;
  }
}

function skipScore() {
  if (state === "over" && !run.saving) {
    newGame();
  }
}

async function loadScores() {
  try {
    const res = await fetch(`${API_BASE}/api/scores?limit=${SCORE_LIMIT}`, {
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
    if (!res.ok) {
      throw new Error(`HTTP ${res.status}`);
    }
    const rows = await res.json();
    if (!Array.isArray(rows)) {
      throw new Error("unexpected payload");
    }
    renderScores(rows);
  } catch {
    scoresBody.replaceChildren();
    scoresTable.hidden = true;
    scoresStatusEl.textContent = "High scores unavailable (backend offline)";
  }
}

function renderScores(rows) {
  scoresBody.replaceChildren();
  rows.slice(0, SCORE_LIMIT).forEach((row, i) => {
    const tr = document.createElement("tr");
    const cells = [String(i + 1), String(row.name ?? ""), String(row.score ?? "")];
    for (const text of cells) {
      const td = document.createElement("td");
      // textContent, never innerHTML: names come from users.
      td.textContent = text;
      tr.append(td);
    }
    scoresBody.append(tr);
  });
  scoresTable.hidden = rows.length === 0;
  scoresStatusEl.textContent = rows.length === 0 ? "No scores yet. Be the first!" : "";
}

document.addEventListener("keydown", (e) => {
  // The name input handles its own keys (typing, Enter = save).
  if (e.target === nameInput) {
    return;
  }
  const isButton = e.target instanceof HTMLButtonElement;

  if (e.key === " " || e.key === "Enter") {
    // A focused button handles Space/Enter as a click; do not also start a game.
    if (isButton) {
      return;
    }
    e.preventDefault();
    if (state === "ready" || state === "over") {
      newGame();
    }
    return;
  }

  const key = e.key.toLowerCase();
  if (key === "p") {
    togglePause();
    return;
  }

  const d = KEY_DIRECTIONS[key];
  if (d) {
    e.preventDefault();
    if (state === "ready") {
      newGame();
    }
    if (state === "running") {
      setDirection(d);
    }
  }
});

nameInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    saveScore();
  }
});

saveBtn.addEventListener("click", saveScore);
skipBtn.addEventListener("click", skipScore);

resetSnake();
draw();
setInterval(tick, TICK_MS);
loadScores();
