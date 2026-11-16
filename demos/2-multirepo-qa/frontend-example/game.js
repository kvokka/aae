(() => {
  "use strict";

  const API = "http://127.0.0.1:8000/api/highscores";
  const SIZE = 20;
  const STEP_MS = 115;
  const canvas = document.querySelector("#board");
  const ctx = canvas.getContext("2d");
  const scoreNode = document.querySelector("#score");
  const statusNode = document.querySelector("#game-status");
  const startButton = document.querySelector("#start-button");
  const scoresList = document.querySelector("#scores-list");
  const scoresMessage = document.querySelector("#scores-message");
  const refreshButton = document.querySelector("#refresh-button");
  const scoreForm = document.querySelector("#score-form");
  const nameInput = document.querySelector("#player-name");
  const submitButton = document.querySelector("#submit-button");
  const submitMessage = document.querySelector("#submit-message");

  let snake = [];
  let food = null;
  let direction = { x: 1, y: 0 };
  let queuedDirection = null;
  let score = 0;
  let state = "ready";
  let lastFrame = 0;
  let accumulator = 0;
  let submitted = false;
  let gameId = 0;

  function reset() {
    snake = [{ x: 9, y: 10 }, { x: 8, y: 10 }, { x: 7, y: 10 }];
    direction = { x: 1, y: 0 };
    queuedDirection = null;
    score = 0;
    food = null;
    state = "ready";
    accumulator = 0;
    submitted = false;
    gameId += 1;
    scoreNode.textContent = "0";
    statusNode.textContent = "Press Start or an arrow key to play.";
    startButton.textContent = "Start game";
    scoreForm.hidden = true;
    submitButton.disabled = false;
    submitMessage.textContent = "";
    nameInput.value = "";
    placeFood();
    draw();
  }

  function placeFood() {
    const free = [];
    for (let y = 0; y < SIZE; y += 1) {
      for (let x = 0; x < SIZE; x += 1) {
        if (!snake.some(cell => cell.x === x && cell.y === y)) free.push({ x, y });
      }
    }
    food = free.length ? free[Math.floor(Math.random() * free.length)] : null;
  }

  function start() {
    if (state === "playing") return;
    if (state === "over" || state === "won") reset();
    state = "playing";
    statusNode.textContent = "Go!";
    startButton.textContent = "Restart";
  }

  function finish(won) {
    state = won ? "won" : "over";
    statusNode.textContent = won ? "You filled the board. You win!" : "Game over. Score: " + score;
    startButton.textContent = "Restart";
    scoreForm.hidden = false;
    submitMessage.textContent = "Enter a name to save this score.";
  }

  function tick() {
    if (queuedDirection) {
      direction = queuedDirection;
      queuedDirection = null;
    }
    const head = { x: snake[0].x + direction.x, y: snake[0].y + direction.y };
    if (head.x < 0 || head.x >= SIZE || head.y < 0 || head.y >= SIZE) {
      finish(false);
      return;
    }
    const eating = food && head.x === food.x && head.y === food.y;
    const bodyToCheck = eating ? snake : snake.slice(0, -1);
    if (bodyToCheck.some(cell => cell.x === head.x && cell.y === head.y)) {
      finish(false);
      return;
    }
    snake.unshift(head);
    if (eating) {
      score += 10;
      scoreNode.textContent = String(score);
      placeFood();
      if (!food) {
        finish(true);
        return;
      }
    } else {
      snake.pop();
    }
  }

  function draw() {
    const cell = canvas.width / SIZE;
    ctx.fillStyle = "#e8f1e7";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.strokeStyle = "#dce8dc";
    ctx.lineWidth = 1;
    for (let i = 1; i < SIZE; i += 1) {
      ctx.beginPath(); ctx.moveTo(i * cell, 0); ctx.lineTo(i * cell, canvas.height); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(0, i * cell); ctx.lineTo(canvas.width, i * cell); ctx.stroke();
    }
    if (food) {
      ctx.fillStyle = "#e35e4e";
      ctx.beginPath(); ctx.arc((food.x + .5) * cell, (food.y + .5) * cell, cell * .34, 0, Math.PI * 2); ctx.fill();
    }
    snake.forEach((part, index) => {
      ctx.fillStyle = index === 0 ? "#145a45" : "#248768";
      const inset = index === 0 ? 2 : 3;
      ctx.beginPath();
      ctx.roundRect(part.x * cell + inset, part.y * cell + inset, cell - inset * 2, cell - inset * 2, 4);
      ctx.fill();
    });
  }

  function frame(timestamp) {
    if (!lastFrame) lastFrame = timestamp;
    accumulator += Math.min(timestamp - lastFrame, 250);
    lastFrame = timestamp;
    while (state === "playing" && accumulator >= STEP_MS) {
      tick();
      accumulator -= STEP_MS;
    }
    draw();
    requestAnimationFrame(frame);
  }

  const directions = {
    ArrowUp: { x: 0, y: -1 }, w: { x: 0, y: -1 }, W: { x: 0, y: -1 },
    ArrowDown: { x: 0, y: 1 }, s: { x: 0, y: 1 }, S: { x: 0, y: 1 },
    ArrowLeft: { x: -1, y: 0 }, a: { x: -1, y: 0 }, A: { x: -1, y: 0 },
    ArrowRight: { x: 1, y: 0 }, d: { x: 1, y: 0 }, D: { x: 1, y: 0 }
  };

  document.addEventListener("keydown", event => {
    const next = directions[event.key];
    if (!next) return;
    if (event.key.startsWith("Arrow")) event.preventDefault();
    if (state !== "playing") start();
    if (next.x === -direction.x && next.y === -direction.y) return;
    if (queuedDirection) return;
    queuedDirection = next;
  });

  startButton.addEventListener("click", () => {
    if (state === "playing" || state === "over" || state === "won") reset();
    start();
  });

  async function loadScores() {
    refreshButton.disabled = true;
    scoresMessage.textContent = "Loading scores…";
    try {
      const response = await fetch(API);
      if (!response.ok) throw new Error("The score service returned HTTP " + response.status + ".");
      const data = await response.json();
      if (!data || !Array.isArray(data.scores)) throw new Error("The score service returned an invalid response.");
      scoresList.replaceChildren();
      data.scores.slice(0, 10).forEach(item => {
        const row = document.createElement("li");
        const name = document.createElement("span");
        const value = document.createElement("span");
        name.className = "score-name";
        value.className = "score-value";
        name.textContent = String(item.name);
        value.textContent = String(item.score);
        row.append(name, value);
        scoresList.append(row);
      });
      scoresMessage.textContent = data.scores.length ? "Top scores" : "No scores yet. Be the first!";
    } catch (error) {
      scoresMessage.textContent = "Could not load scores. " + error.message + " Use Refresh to try again.";
    } finally {
      refreshButton.disabled = false;
    }
  }

  refreshButton.addEventListener("click", loadScores);

  scoreForm.addEventListener("submit", async event => {
    event.preventDefault();
    if (submitted || (state !== "over" && state !== "won")) return;
    const name = nameInput.value.trim();
    const nameLength = Array.from(name).length;
    if (nameLength < 1 || nameLength > 24) {
      submitMessage.textContent = "Name must be 1 to 24 characters after trimming.";
      nameInput.focus();
      return;
    }
    submitButton.disabled = true;
    submitMessage.textContent = "Saving score…";
    const thisGame = gameId;
    try {
      const response = await fetch(API, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name, score })
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || "The score could not be saved (HTTP " + response.status + ").");
      if (gameId === thisGame) {
        submitted = true;
        submitMessage.textContent = "Score saved for " + result.name + ".";
        submitButton.disabled = true;
      }
      await loadScores();
    } catch (error) {
      if (gameId === thisGame) {
        submitMessage.textContent = "Could not save score. " + error.message + " You can retry.";
        submitButton.disabled = false;
      }
    }
  });

  reset();
  requestAnimationFrame(frame);
  loadScores();
})();
