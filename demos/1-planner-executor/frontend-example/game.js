"use strict";

// Backend base URL. Must match the score API contract.
const API_BASE_URL = "http://localhost:8080";
const SCORES_ENDPOINT = `${API_BASE_URL}/api/scores`;

const NAME_STORAGE_KEY = "snake.playerName";
const NAME_MAX_LENGTH = 20;
const LEADERBOARD_LIMIT = 10;
const UNAVAILABLE_MESSAGE = "Scores unavailable (backend is not running)";

const GRID_SIZE = 20;
const CELL_SIZE = 20;
const STEP_INTERVAL_MS = 120;

const COLORS = {
  board: "#111827",
  grid: "#1f2937",
  snake: "#22c55e",
  snakeHead: "#15803d",
  food: "#ef4444",
  overlay: "rgba(0, 0, 0, 0.65)",
  text: "#f9fafb",
};

const DIRECTIONS = {
  up: { x: 0, y: -1 },
  down: { x: 0, y: 1 },
  left: { x: -1, y: 0 },
  right: { x: 1, y: 0 },
};

// Keys are lower-cased event.key values.
const KEY_TO_DIRECTION = {
  arrowup: DIRECTIONS.up,
  w: DIRECTIONS.up,
  arrowdown: DIRECTIONS.down,
  s: DIRECTIONS.down,
  arrowleft: DIRECTIONS.left,
  a: DIRECTIONS.left,
  arrowright: DIRECTIONS.right,
  d: DIRECTIONS.right,
};

const canvas = document.getElementById("board");
const context = canvas.getContext("2d");
const scoreLabel = document.getElementById("score");
const saveForm = document.getElementById("save-form");
const nameInput = document.getElementById("name");
const saveButton = document.getElementById("save-button");
const saveStatus = document.getElementById("save-status");
const leaderboardStatus = document.getElementById("leaderboard-status");
const leaderboardList = document.getElementById("leaderboard");

const game = {
  status: "idle", // "idle" (waiting for first key) | "running" | "over"
  snake: [],
  direction: DIRECTIONS.right, // direction applied on the last step
  nextDirection: DIRECTIONS.right, // direction applied on the next step
  food: null,
  score: 0,
  timerId: null,
  saving: false,
  saved: false,
};

class ScoreApiError extends Error {}

function sameCell(a, b) {
  return a.x === b.x && a.y === b.y;
}

function isInsideGrid(cell) {
  return cell.x >= 0 && cell.x < GRID_SIZE && cell.y >= 0 && cell.y < GRID_SIZE;
}

// Returns a random cell not occupied by the snake, or null if the board is full.
function placeFood() {
  const occupied = new Set(game.snake.map((cell) => cell.y * GRID_SIZE + cell.x));
  const free = [];
  for (let index = 0; index < GRID_SIZE * GRID_SIZE; index += 1) {
    if (!occupied.has(index)) {
      free.push(index);
    }
  }
  if (free.length === 0) {
    return null;
  }
  const index = free[Math.floor(Math.random() * free.length)];
  return { x: index % GRID_SIZE, y: Math.floor(index / GRID_SIZE) };
}

function stopTimer() {
  if (game.timerId !== null) {
    clearInterval(game.timerId);
    game.timerId = null;
  }
}

function startTimer() {
  stopTimer();
  game.timerId = setInterval(step, STEP_INTERVAL_MS);
}

function resetBoard(status) {
  const middle = Math.floor(GRID_SIZE / 2);
  game.snake = [
    { x: middle, y: middle },
    { x: middle - 1, y: middle },
    { x: middle - 2, y: middle },
  ];
  game.direction = DIRECTIONS.right;
  game.nextDirection = DIRECTIONS.right;
  game.score = 0;
  game.food = placeFood();
  game.status = status;
  game.saved = false;
  saveForm.hidden = true;
}

function startNewGame() {
  resetBoard("running");
  startTimer();
  render();
}

// Queues a direction for the next step. A 180-degree reverse of the current direction is ignored.
function steer(direction) {
  const isReverse =
    direction.x === -game.direction.x && direction.y === -game.direction.y;
  if (!isReverse) {
    game.nextDirection = direction;
  }
}

function step() {
  game.direction = game.nextDirection;
  const head = game.snake[0];
  const next = { x: head.x + game.direction.x, y: head.y + game.direction.y };
  const eats = sameCell(next, game.food);
  // The tail leaves the board on this step unless the snake grows, so it is not a blocker.
  const blockers = eats ? game.snake : game.snake.slice(0, -1);

  if (!isInsideGrid(next) || blockers.some((cell) => sameCell(cell, next))) {
    endGame();
    return;
  }

  game.snake.unshift(next);
  if (eats) {
    game.score += 1;
    game.food = placeFood();
    if (game.food === null) {
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
  game.status = "over";
  render();
  if (game.score > 0) {
    showSavePanel();
  }
}

function showSavePanel() {
  nameInput.value = loadSavedName();
  nameInput.disabled = false;
  saveButton.disabled = false;
  saveStatus.textContent = "";
  saveForm.hidden = false;
}

function setSaveStatus(message) {
  saveStatus.textContent = message;
}

function handleKeyDown(event) {
  const target = event.target;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    return; // Typing in the name field must not steer or restart the game.
  }

  const key = event.key.toLowerCase();
  const direction = KEY_TO_DIRECTION[key];

  if (direction) {
    if (game.status === "over") {
      return;
    }
    event.preventDefault();
    if (game.status === "idle") {
      game.status = "running";
      startTimer();
    }
    steer(direction);
    return;
  }

  if (key === " " || key === "enter") {
    if (game.status === "running" || event.repeat) {
      return;
    }
    if (target instanceof HTMLButtonElement) {
      if (key === "enter") {
        return; // Enter on a focused button (Save) activates that button.
      }
      // Space on a focused button would click it on release; drop focus so the key starts the game instead.
      target.blur();
    }
    event.preventDefault();
    startNewGame();
  }
}

async function fetchScores() {
  const response = await fetch(SCORES_ENDPOINT);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  const scores = await response.json();
  if (!Array.isArray(scores)) {
    throw new Error("Unexpected response shape");
  }
  return scores;
}

async function postScore(name, score) {
  const response = await fetch(SCORES_ENDPOINT, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name, score }),
  });
  const data = await response.json().catch(() => null);
  if (response.status !== 201) {
    const message =
      data && typeof data.error === "string"
        ? data.error
        : `Could not save the score (HTTP ${response.status}).`;
    throw new ScoreApiError(message);
  }
  return data;
}

function isSameEntry(a, b) {
  return a.name === b.name && a.score === b.score && a.date === b.date;
}

function renderLeaderboard(scores, highlight) {
  const items = scores.slice(0, LEADERBOARD_LIMIT).map((entry, index) => {
    const item = document.createElement("li");
    item.textContent = `${index + 1}. ${entry.name} - ${entry.score}`;
    if (highlight && isSameEntry(entry, highlight)) {
      item.classList.add("highlight");
    }
    return item;
  });
  leaderboardList.replaceChildren(...items);
  leaderboardStatus.textContent = scores.length === 0 ? "No scores yet." : "";
}

// Never throws: a failed load only changes the leaderboard message.
async function refreshLeaderboard(highlight = null) {
  leaderboardStatus.textContent = "Loading scores...";
  try {
    const scores = await fetchScores();
    renderLeaderboard(scores, highlight);
  } catch (error) {
    leaderboardList.replaceChildren();
    leaderboardStatus.textContent = UNAVAILABLE_MESSAGE;
  }
}

async function handleSave(event) {
  event.preventDefault();
  if (game.status !== "over" || game.saved || game.saving || game.score <= 0) {
    return;
  }

  const name = nameInput.value.trim();
  if (name === "") {
    setSaveStatus("Please enter a name.");
    nameInput.focus();
    return;
  }

  const score = game.score;
  game.saving = true;
  saveButton.disabled = true;
  setSaveStatus("Saving...");
  saveName(name);

  try {
    const entry = await postScore(name, score);
    game.saved = true;
    nameInput.disabled = true;
    setSaveStatus("Score saved.");
    await refreshLeaderboard(entry);
  } catch (error) {
    const message =
      error instanceof ScoreApiError ? error.message : UNAVAILABLE_MESSAGE;
    setSaveStatus(message);
    saveButton.disabled = false;
  } finally {
    game.saving = false;
  }
}

function loadSavedName() {
  try {
    return (localStorage.getItem(NAME_STORAGE_KEY) || "").slice(0, NAME_MAX_LENGTH);
  } catch (error) {
    return "";
  }
}

function saveName(name) {
  try {
    localStorage.setItem(NAME_STORAGE_KEY, name);
  } catch (error) {
    // Storage may be unavailable (for example, in private mode); the name is simply not remembered.
  }
}

function drawOverlay(lines) {
  context.fillStyle = COLORS.overlay;
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.fillStyle = COLORS.text;
  context.textAlign = "center";
  context.textBaseline = "middle";

  const lineHeight = 36;
  const top = canvas.height / 2 - ((lines.length - 1) * lineHeight) / 2;
  lines.forEach((line, index) => {
    context.font = `${line.weight} ${line.size}px system-ui, sans-serif`;
    context.fillText(line.text, canvas.width / 2, top + index * lineHeight);
  });
}

function drawBoard() {
  context.fillStyle = COLORS.board;
  context.fillRect(0, 0, canvas.width, canvas.height);

  context.strokeStyle = COLORS.grid;
  context.lineWidth = 1;
  context.beginPath();
  for (let line = 1; line < GRID_SIZE; line += 1) {
    const position = line * CELL_SIZE + 0.5;
    context.moveTo(position, 0);
    context.lineTo(position, canvas.height);
    context.moveTo(0, position);
    context.lineTo(canvas.width, position);
  }
  context.stroke();

  if (game.food) {
    context.fillStyle = COLORS.food;
    context.fillRect(game.food.x * CELL_SIZE + 2, game.food.y * CELL_SIZE + 2, CELL_SIZE - 4, CELL_SIZE - 4);
  }

  game.snake.forEach((cell, index) => {
    context.fillStyle = index === 0 ? COLORS.snakeHead : COLORS.snake;
    context.fillRect(cell.x * CELL_SIZE + 1, cell.y * CELL_SIZE + 1, CELL_SIZE - 2, CELL_SIZE - 2);
  });

  if (game.status === "idle") {
    drawOverlay([
      { text: "Snake", size: 32, weight: "bold" },
      { text: "Press an arrow key or W A S D to start", size: 15, weight: "normal" },
    ]);
  } else if (game.status === "over") {
    drawOverlay([
      { text: "Game over", size: 32, weight: "bold" },
      { text: `Score: ${game.score}`, size: 20, weight: "normal" },
      { text: "Press Space or Enter to play again", size: 15, weight: "normal" },
    ]);
  }
}

function render() {
  drawBoard();
  scoreLabel.textContent = `Score: ${game.score}`;
}

saveForm.addEventListener("submit", handleSave);
document.addEventListener("keydown", handleKeyDown);

resetBoard("idle");
render();
refreshLeaderboard();
