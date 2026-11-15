// Snake: game logic and high-score API calls. Plain browser JavaScript, no build step.

// Backend base URL; change it here if the API runs elsewhere.
const API_BASE_URL = "http://localhost:8000";
const SCORES_URL = `${API_BASE_URL}/api/scores`;

const GRID_SIZE = 20; // cells per side
const CELL_PX = 20; // 20 * 20 = 400 px canvas
const STEP_MS = 120; // time between snake steps
const POINTS_PER_FOOD = 10;
const MAX_NAME_LENGTH = 20;
const DEFAULT_NAME = "anonymous";
const UNAVAILABLE_TEXT = "High scores unavailable";

const UP = { x: 0, y: -1 };
const DOWN = { x: 0, y: 1 };
const LEFT = { x: -1, y: 0 };
const RIGHT = { x: 1, y: 0 };

// Keys are normalized to lower case for single characters, so "W" and "w" both work.
const KEY_DIRECTIONS = {
  ArrowUp: UP,
  w: UP,
  ArrowDown: DOWN,
  s: DOWN,
  ArrowLeft: LEFT,
  a: LEFT,
  ArrowRight: RIGHT,
  d: RIGHT,
};

const canvas = document.getElementById("board");
const ctx = canvas.getContext("2d");
const scoreElement = document.getElementById("score");
const messageElement = document.getElementById("message");
const startButton = document.getElementById("start-button");
const scoresMessageElement = document.getElementById("scores-message");
const scoresListElement = document.getElementById("scores-list");

// phase: "idle" (before the first game), "playing", or "over".
let phase = "idle";
let state = createInitialState();
let timerId = null;

function createInitialState() {
  const middle = Math.floor(GRID_SIZE / 2);
  // Head first; the snake starts with length 3 and moves right.
  const snake = [
    { x: middle, y: middle },
    { x: middle - 1, y: middle },
    { x: middle - 2, y: middle },
  ];
  return {
    snake,
    dir: RIGHT,
    nextDir: RIGHT,
    food: randomFreeCell(snake),
    score: 0,
  };
}

function randomFreeCell(snake) {
  const occupied = new Set(snake.map((cell) => cell.y * GRID_SIZE + cell.x));
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

function startGame() {
  if (phase === "playing") {
    return;
  }
  stopTimer();
  state = createInitialState();
  phase = "playing";
  setMessage("");
  startButton.disabled = true;
  updateScore();
  draw();
  timerId = setInterval(tick, STEP_MS);
}

function stopTimer() {
  if (timerId !== null) {
    clearInterval(timerId);
    timerId = null;
  }
}

function tick() {
  const current = state;
  current.dir = current.nextDir;
  const head = current.snake[0];
  const next = { x: head.x + current.dir.x, y: head.y + current.dir.y };

  const hitWall =
    next.x < 0 || next.y < 0 || next.x >= GRID_SIZE || next.y >= GRID_SIZE;
  const ateFood =
    current.food !== null && next.x === current.food.x && next.y === current.food.y;
  // The tail moves away this step unless the snake grows, so its cell is free to enter.
  const body = ateFood ? current.snake : current.snake.slice(0, -1);
  const hitSelf = body.some((cell) => cell.x === next.x && cell.y === next.y);

  if (hitWall || hitSelf) {
    endGame();
    return;
  }

  current.snake.unshift(next);
  if (ateFood) {
    current.score += POINTS_PER_FOOD;
    current.food = randomFreeCell(current.snake);
    updateScore();
    if (current.food === null) {
      // The board is full; there is nowhere left to go.
      endGame();
      return;
    }
  } else {
    current.snake.pop();
  }
  draw();
}

function endGame() {
  stopTimer();
  phase = "over";
  const finalScore = state.score;
  startButton.textContent = "Restart";
  startButton.disabled = false;
  setMessage(`Game over. Final score: ${finalScore}.`);
  draw();
  if (finalScore > 0) {
    // Let the browser paint the game-over frame before the blocking prompt opens.
    setTimeout(() => saveScore(finalScore), 0);
  }
}

function queueDirection(dir) {
  // Compare with the direction the snake is actually moving, so a reversal into the body is ignored.
  if (dir.x + state.dir.x === 0 && dir.y + state.dir.y === 0) {
    return;
  }
  state.nextDir = dir;
}

function updateScore() {
  scoreElement.textContent = String(state.score);
}

function setMessage(text) {
  messageElement.textContent = text;
}

function draw() {
  ctx.fillStyle = "#101820";
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  if (state.food !== null) {
    ctx.fillStyle = "#e94560";
    ctx.fillRect(
      state.food.x * CELL_PX + 2,
      state.food.y * CELL_PX + 2,
      CELL_PX - 4,
      CELL_PX - 4,
    );
  }

  state.snake.forEach((cell, index) => {
    ctx.fillStyle = index === 0 ? "#7bd389" : "#4caf50";
    ctx.fillRect(cell.x * CELL_PX + 1, cell.y * CELL_PX + 1, CELL_PX - 2, CELL_PX - 2);
  });

  if (phase !== "playing") {
    ctx.fillStyle = "rgba(0, 0, 0, 0.6)";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = "#ffffff";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.font = "bold 36px system-ui, sans-serif";
    if (phase === "over") {
      ctx.fillText("Game over", canvas.width / 2, canvas.height / 2 - 20);
      ctx.font = "20px system-ui, sans-serif";
      ctx.fillText(`Score: ${state.score}`, canvas.width / 2, canvas.height / 2 + 20);
    } else {
      ctx.font = "24px system-ui, sans-serif";
      ctx.fillText("Press Space or Enter", canvas.width / 2, canvas.height / 2);
    }
  }
}

// Turns the answer of window.prompt into a name: trimmed, at most 20 characters, never empty.
// Characters are counted as code points so that the limit matches the backend.
function normalizeName(rawName) {
  const trimmed = (rawName ?? "").trim();
  if (trimmed === "") {
    return DEFAULT_NAME;
  }
  return Array.from(trimmed).slice(0, MAX_NAME_LENGTH).join("").trim();
}

async function saveScore(score) {
  const name = normalizeName(
    window.prompt("Game over! Enter your name for the high-score list:", ""),
  );
  try {
    const response = await fetch(SCORES_URL, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, score }),
    });
    if (response.status !== 201) {
      throw new Error(`Unexpected status ${response.status}`);
    }
  } catch (error) {
    console.error("Could not save the score:", error);
    setScoresMessage(UNAVAILABLE_TEXT);
    return;
  }
  await loadScores();
}

async function loadScores() {
  try {
    const response = await fetch(SCORES_URL);
    if (!response.ok) {
      throw new Error(`Unexpected status ${response.status}`);
    }
    const scores = await response.json();
    if (!Array.isArray(scores)) {
      throw new Error("Scores response is not an array");
    }
    renderScores(scores);
  } catch (error) {
    console.error("Could not load high scores:", error);
    renderScores([]);
    setScoresMessage(UNAVAILABLE_TEXT);
  }
}

function renderScores(scores) {
  scoresListElement.replaceChildren();
  for (const entry of scores) {
    const item = document.createElement("li");
    item.textContent = `${entry.name}: ${entry.score}`;
    scoresListElement.appendChild(item);
  }
  setScoresMessage(scores.length === 0 ? "No scores yet." : "");
}

function setScoresMessage(text) {
  scoresMessageElement.textContent = text;
}

window.addEventListener("keydown", (event) => {
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;

  const dir = KEY_DIRECTIONS[key];
  if (dir !== undefined) {
    event.preventDefault();
    if (phase === "playing") {
      queueDirection(dir);
    }
    return;
  }

  if (key === " " || key === "Enter") {
    event.preventDefault();
    if (phase !== "playing") {
      startGame();
    }
  }
});

startButton.addEventListener("click", () => {
  startGame();
  startButton.blur();
});

updateScore();
draw();
loadScores();
