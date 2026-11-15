"use strict";

// Base URL of the score backend. Change it here only.
const API_BASE_URL = "http://127.0.0.1:8000";

const GRID_SIZE = 20;
const CELL_PX = 20; // GRID_SIZE * CELL_PX matches the 400px canvas
const TICK_MS = 120;
const INITIAL_LENGTH = 3;
const POINTS_PER_FOOD = 10;
const NAME_MAX_LENGTH = 20;
const DEFAULT_NAME = "Player";
const LEADERBOARD_LIMIT = 10;
const REQUEST_TIMEOUT_MS = 5000;

const UP = { x: 0, y: -1 };
const DOWN = { x: 0, y: 1 };
const LEFT = { x: -1, y: 0 };
const RIGHT = { x: 1, y: 0 };

const DIRECTION_BY_KEY = {
  ArrowUp: UP,
  ArrowDown: DOWN,
  ArrowLeft: LEFT,
  ArrowRight: RIGHT,
  w: UP,
  s: DOWN,
  a: LEFT,
  d: RIGHT,
};

const COLORS = {
  board: "#14161b",
  grid: "#1d2028",
  snakeHead: "#8af0a8",
  snakeBody: "#4fd17a",
  food: "#ff7b72",
  overlay: "rgba(13, 15, 20, 0.72)",
  overlayText: "#e8eaf0",
};

const canvas = document.getElementById("board");
const ctx = canvas.getContext("2d");
const scoreEl = document.getElementById("score");
const statusEl = document.getElementById("status");
const saveForm = document.getElementById("save-form");
const saveButton = document.getElementById("save-button");
const nameInput = document.getElementById("player-name");
const saveMessageEl = document.getElementById("save-message");
const leaderboardMessageEl = document.getElementById("leaderboard-message");
const leaderboardTable = document.getElementById("leaderboard-table");
const leaderboardBody = document.getElementById("leaderboard-body");

// Game state. state: "ready" | "running" | "over".
// snake[0] is the head. dir is the direction applied by the last tick;
// queuedDir is the single pending direction applied by the next tick.
let game = null;
let timerId = null;
// The finished game whose score the name form is currently saving.
let saveTarget = null;

function createGame() {
  const cx = Math.floor(GRID_SIZE / 2);
  const cy = Math.floor(GRID_SIZE / 2);
  const snake = [];
  for (let i = 0; i < INITIAL_LENGTH; i += 1) {
    snake.push({ x: cx - i, y: cy });
  }
  return {
    snake,
    dir: RIGHT,
    queuedDir: RIGHT,
    food: randomFreeCell(snake),
    score: 0,
    state: "ready",
  };
}

function randomFreeCell(snake) {
  const occupied = new Set(snake.map((cell) => cell.y * GRID_SIZE + cell.x));
  const free = [];
  for (let y = 0; y < GRID_SIZE; y += 1) {
    for (let x = 0; x < GRID_SIZE; x += 1) {
      if (!occupied.has(y * GRID_SIZE + x)) {
        free.push({ x, y });
      }
    }
  }
  if (free.length === 0) {
    return null;
  }
  return free[Math.floor(Math.random() * free.length)];
}

function stopTimer() {
  if (timerId !== null) {
    clearInterval(timerId);
    timerId = null;
  }
}

function startGame() {
  stopTimer();
  hideSaveForm();
  game = createGame();
  game.state = "running";
  setStatus("Game on! Steer with arrow keys or WASD.");
  timerId = setInterval(tick, TICK_MS);
  render();
}

function queueDirection(dir) {
  // A reversal into the neck is ignored. Checked against the applied direction,
  // so at most one change can take effect per tick: only queuedDir is applied.
  const current = game.dir;
  if (dir.x === -current.x && dir.y === -current.y) {
    return;
  }
  game.queuedDir = dir;
}

function tick() {
  if (!game || game.state !== "running") {
    return;
  }
  game.dir = game.queuedDir;
  const head = game.snake[0];
  const next = { x: head.x + game.dir.x, y: head.y + game.dir.y };

  if (next.x < 0 || next.x >= GRID_SIZE || next.y < 0 || next.y >= GRID_SIZE) {
    endGame();
    return;
  }

  const eats = next.x === game.food.x && next.y === game.food.y;
  // The tail moves away on a normal step, so its cell is free; when eating it stays.
  const blockers = eats ? game.snake : game.snake.slice(0, -1);
  if (blockers.some((cell) => cell.x === next.x && cell.y === next.y)) {
    endGame();
    return;
  }

  game.snake.unshift(next);
  if (eats) {
    game.score += POINTS_PER_FOOD;
    game.food = randomFreeCell(game.snake);
    if (game.food === null) {
      // The board is full: nothing left to eat.
      endGame();
      return;
    }
  } else {
    game.snake.pop();
  }
  render();
}

function endGame() {
  stopTimer();
  game.state = "over";
  render();
  if (game.score > 0) {
    setStatus(`Game over! Final score: ${game.score}. Enter your name to save it.`);
    showSaveForm(game);
  } else {
    setStatus("Game over. A score of 0 is not saved. Press Space or Enter to play again.");
  }
}

function render() {
  ctx.fillStyle = COLORS.board;
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  ctx.strokeStyle = COLORS.grid;
  ctx.lineWidth = 1;
  for (let i = 1; i < GRID_SIZE; i += 1) {
    const p = i * CELL_PX + 0.5;
    ctx.beginPath();
    ctx.moveTo(p, 0);
    ctx.lineTo(p, canvas.height);
    ctx.moveTo(0, p);
    ctx.lineTo(canvas.width, p);
    ctx.stroke();
  }

  if (game.food) {
    ctx.fillStyle = COLORS.food;
    ctx.beginPath();
    ctx.arc(
      game.food.x * CELL_PX + CELL_PX / 2,
      game.food.y * CELL_PX + CELL_PX / 2,
      CELL_PX / 2 - 3,
      0,
      Math.PI * 2
    );
    ctx.fill();
  }

  game.snake.forEach((cell, index) => {
    ctx.fillStyle = index === 0 ? COLORS.snakeHead : COLORS.snakeBody;
    ctx.fillRect(cell.x * CELL_PX + 1, cell.y * CELL_PX + 1, CELL_PX - 2, CELL_PX - 2);
  });

  if (game.state !== "running") {
    ctx.fillStyle = COLORS.overlay;
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = COLORS.overlayText;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.font = "600 28px system-ui, sans-serif";
    ctx.fillText(game.state === "over" ? "Game over" : "Snake", canvas.width / 2, canvas.height / 2 - 14);
    ctx.font = "16px system-ui, sans-serif";
    ctx.fillText("Press Space or Enter", canvas.width / 2, canvas.height / 2 + 18);
  }

  scoreEl.textContent = String(game.score);
}

function setStatus(text) {
  statusEl.textContent = text;
}

function isTypingTarget(element) {
  return (
    element instanceof HTMLElement &&
    (element.tagName === "INPUT" || element.tagName === "TEXTAREA" || element.tagName === "SELECT")
  );
}

document.addEventListener("keydown", (event) => {
  if (isTypingTarget(event.target) || event.ctrlKey || event.metaKey || event.altKey) {
    return;
  }

  const key = event.key;
  if (key === " " || key === "Enter") {
    // Buttons handle their own Space/Enter activation.
    if (event.target instanceof HTMLButtonElement) {
      return;
    }
    event.preventDefault();
    if (!event.repeat && (!game || game.state !== "running")) {
      startGame();
    }
    return;
  }

  const dir = DIRECTION_BY_KEY[key] || DIRECTION_BY_KEY[String(key).toLowerCase()];
  if (dir) {
    event.preventDefault();
    if (game && game.state === "running") {
      queueDirection(dir);
    }
  }
});

// ---- Name form and score submission ----

function showSaveForm(finishedGame) {
  saveTarget = finishedGame;
  nameInput.value = "";
  saveMessageEl.textContent = "";
  saveMessageEl.classList.remove("error");
  saveButton.disabled = false;
  saveForm.hidden = false;
  nameInput.focus();
}

function hideSaveForm() {
  saveTarget = null;
  saveForm.hidden = true;
}

function normalizeName(raw) {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return DEFAULT_NAME;
  }
  // Input maxlength already limits this; the slice is a safety net.
  return Array.from(trimmed).slice(0, NAME_MAX_LENGTH).join("");
}

function fetchWithTimeout(url, options = {}) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  return fetch(url, { ...options, signal: controller.signal }).finally(() => clearTimeout(timeout));
}

function unreachableMessage() {
  return `cannot reach the score server at ${API_BASE_URL}. Check that it is running and allows requests from this page.`;
}

async function submitScore(name, score) {
  let response;
  try {
    response = await fetchWithTimeout(`${API_BASE_URL}/api/scores`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, score }),
    });
  } catch (error) {
    throw new Error(`Score not saved: ${unreachableMessage()}`);
  }

  let data = null;
  try {
    data = await response.json();
  } catch (error) {
    data = null;
  }

  if (response.status === 201) {
    return data;
  }
  const detail = data && typeof data.error === "string" ? data.error : `HTTP ${response.status}`;
  throw new Error(`Score not saved: ${detail}.`);
}

saveForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const target = saveTarget;
  if (!target || saveButton.disabled) {
    return;
  }

  const name = normalizeName(nameInput.value);
  saveButton.disabled = true;
  saveMessageEl.classList.remove("error");
  saveMessageEl.textContent = "Saving…";

  try {
    await submitScore(name, target.score);
    // A new game may have started while the request was in flight.
    if (saveTarget === target) {
      hideSaveForm();
      setStatus(`Saved: ${name} with ${target.score} points. Press Space or Enter to play again.`);
    }
    await loadLeaderboard();
  } catch (error) {
    if (saveTarget === target) {
      saveMessageEl.classList.add("error");
      saveMessageEl.textContent = error.message;
      saveButton.disabled = false;
    }
  }
});

// ---- Leaderboard ----

function setLeaderboardMessage(text, isError = false) {
  leaderboardMessageEl.textContent = text;
  leaderboardMessageEl.classList.toggle("error", isError);
}

function formatDate(value) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? String(value) : date.toLocaleString();
}

function renderLeaderboard(entries) {
  leaderboardBody.replaceChildren();
  if (entries.length === 0) {
    leaderboardTable.hidden = true;
    setLeaderboardMessage("No scores yet. Be the first!");
    return;
  }

  entries.forEach((entry, index) => {
    const row = document.createElement("tr");
    const cells = [
      { text: String(index + 1), className: "num" },
      { text: String(entry && entry.name !== undefined ? entry.name : "") },
      { text: String(entry && entry.score !== undefined ? entry.score : ""), className: "num score" },
      { text: formatDate(entry && entry.created_at !== undefined ? entry.created_at : "") },
    ];
    for (const cell of cells) {
      const td = document.createElement("td");
      td.textContent = cell.text;
      if (cell.className) {
        td.className = cell.className;
      }
      row.appendChild(td);
    }
    leaderboardBody.appendChild(row);
  });

  leaderboardTable.hidden = false;
  setLeaderboardMessage("");
}

async function loadLeaderboard() {
  try {
    let response;
    try {
      response = await fetchWithTimeout(`${API_BASE_URL}/api/scores?limit=${LEADERBOARD_LIMIT}`);
    } catch (error) {
      leaderboardTable.hidden = true;
      setLeaderboardMessage(`High scores unavailable: ${unreachableMessage()} The game still works.`, true);
      return;
    }

    if (!response.ok) {
      leaderboardTable.hidden = true;
      setLeaderboardMessage(`High scores unavailable: the server returned HTTP ${response.status}.`, true);
      return;
    }

    let entries;
    try {
      entries = await response.json();
    } catch (error) {
      entries = null;
    }
    if (!Array.isArray(entries)) {
      leaderboardTable.hidden = true;
      setLeaderboardMessage("High scores unavailable: the server sent an unexpected response.", true);
      return;
    }
    renderLeaderboard(entries);
  } catch (error) {
    // Last-resort guard: the leaderboard must never break the game.
    leaderboardTable.hidden = true;
    setLeaderboardMessage("High scores unavailable: unexpected error while loading.", true);
  }
}

// ---- Start ----

game = createGame();
render();
setLeaderboardMessage("Loading high scores…");
loadLeaderboard();
