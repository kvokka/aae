'use strict';

const API_URL = 'http://localhost:3000/api/high-scores';
const canvas = document.querySelector('#board');
const ctx = canvas.getContext('2d');
const scoreDisplay = document.querySelector('#score');
const overlay = document.querySelector('#overlay');
const overlayTitle = document.querySelector('#overlay-title');
const overlayCopy = document.querySelector('#overlay-copy');
const startButton = document.querySelector('#start-button');
const restartButton = document.querySelector('#restart-button');
const status = document.querySelector('#game-status');
const form = document.querySelector('#submit-form');
const nameInput = document.querySelector('#player-name');
const submitButton = document.querySelector('#submit-button');
const submitStatus = document.querySelector('#submit-status');
const highScores = document.querySelector('#high-scores');
const scoresStatus = document.querySelector('#scores-status');

const CELLS = 24;
const CELL = canvas.width / CELLS;
const STEP_MS = 115;
const DIRECTIONS = {
  ArrowUp: { x: 0, y: -1 }, ArrowDown: { x: 0, y: 1 },
  ArrowLeft: { x: -1, y: 0 }, ArrowRight: { x: 1, y: 0 },
  w: { x: 0, y: -1 }, s: { x: 0, y: 1 }, a: { x: -1, y: 0 }, d: { x: 1, y: 0 },
};
let snake, direction, queuedDirection, food, score, state = 'ready';
let lastStep = 0;
let submitted = false;
let submitting = false;

function randomFood() {
  const empty = [];
  for (let y = 0; y < CELLS; y += 1) for (let x = 0; x < CELLS; x += 1) {
    if (!snake.some((part) => part.x === x && part.y === y)) empty.push({ x, y });
  }
  return empty.length ? empty[Math.floor(Math.random() * empty.length)] : null;
}

function draw() {
  ctx.fillStyle = '#111814'; ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.strokeStyle = '#1b251e'; ctx.lineWidth = 1;
  for (let i = 0; i <= CELLS; i += 1) {
    ctx.beginPath(); ctx.moveTo(i * CELL, 0); ctx.lineTo(i * CELL, canvas.height); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(0, i * CELL); ctx.lineTo(canvas.width, i * CELL); ctx.stroke();
  }
  if (food) {
    ctx.fillStyle = '#ff8066'; ctx.shadowColor = '#ff8066'; ctx.shadowBlur = 14;
    ctx.beginPath(); ctx.arc((food.x + .5) * CELL, (food.y + .5) * CELL, CELL * .34, 0, Math.PI * 2); ctx.fill(); ctx.shadowBlur = 0;
  }
  snake.forEach((part, index) => {
    ctx.fillStyle = index === 0 ? '#d4ff8d' : '#91d94b';
    const inset = 2;
    ctx.beginPath(); ctx.roundRect(part.x * CELL + inset, part.y * CELL + inset, CELL - inset * 2, CELL - inset * 2, 5); ctx.fill();
  });
}

function reset() {
  snake = [{ x: 11, y: 12 }, { x: 10, y: 12 }, { x: 9, y: 12 }];
  direction = { x: 1, y: 0 }; queuedDirection = direction; score = 0;
  scoreDisplay.textContent = '0'; food = randomFood(); state = 'ready'; submitted = false; submitting = false;
  form.hidden = true; nameInput.value = ''; submitStatus.textContent = '';
  startButton.hidden = false; startButton.textContent = 'Start game';
  overlayTitle.textContent = 'Ready to play?'; overlayCopy.textContent = 'Use the arrow keys to move'; overlay.hidden = false;
  status.textContent = 'Press start when you’re ready.'; draw();
}

function startGame() {
  if (state === 'playing') return;
  reset(); state = 'playing'; overlay.hidden = true; status.textContent = 'Game in progress'; lastStep = performance.now();
  requestAnimationFrame(tick);
}

function endGame(won = false) {
  state = 'over'; overlay.hidden = false; startButton.hidden = true;
  overlayTitle.textContent = won ? 'Board cleared!' : 'Game over';
  overlayCopy.textContent = `Final score: ${score}`;
  status.textContent = won ? 'You filled the board!' : 'Run ended. Submit your score or restart.';
  form.hidden = false; nameInput.focus({ preventScroll: true });
}

function tick(now) {
  if (state !== 'playing') return;
  if (now - lastStep >= STEP_MS) {
    lastStep += STEP_MS;
    direction = queuedDirection;
    const head = { x: snake[0].x + direction.x, y: snake[0].y + direction.y };
    const eating = food && head.x === food.x && head.y === food.y;
    const body = eating ? snake : snake.slice(0, -1);
    if (head.x < 0 || head.y < 0 || head.x >= CELLS || head.y >= CELLS || body.some((part) => part.x === head.x && part.y === head.y)) {
      endGame(); draw(); return;
    }
    snake.unshift(head);
    if (eating) { score += 1; scoreDisplay.textContent = String(score); food = randomFood(); }
    else snake.pop();
    draw();
    if (!food) { endGame(true); return; }
  }
  requestAnimationFrame(tick);
}

document.addEventListener('keydown', (event) => {
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  const next = DIRECTIONS[key];
  if (next) {
    event.preventDefault();
    if (state !== 'playing') return;
    if (next.x + queuedDirection.x === 0 && next.y + queuedDirection.y === 0) return;
    queuedDirection = next;
  }
});
startButton.addEventListener('click', startGame);
restartButton.addEventListener('click', startGame);

function renderScores(scores) {
  highScores.replaceChildren();
  if (!scores.length) {
    const item = document.createElement('li'); item.className = 'muted'; item.textContent = 'No scores yet. Be the first!'; highScores.append(item); return;
  }
  scores.forEach(({ name, score: points }) => {
    const item = document.createElement('li');
    const player = document.createElement('span'); player.className = 'name'; player.textContent = name;
    const value = document.createElement('span'); value.className = 'points'; value.textContent = String(points);
    item.append(player, value); highScores.append(item);
  });
}

async function loadScores() {
  try {
    const response = await fetch(API_URL);
    if (!response.ok) throw new Error(`Server returned ${response.status}`);
    const data = await response.json();
    if (!Array.isArray(data.scores)) throw new Error('Unexpected score response');
    renderScores(data.scores); scoresStatus.textContent = '';
    return data.scores;
  } catch {
    scoresStatus.textContent = 'High scores are unavailable. The game still works; try refreshing later.';
    highScores.replaceChildren();
    const item = document.createElement('li'); item.className = 'muted'; item.textContent = 'Could not load scores'; highScores.append(item);
    return null;
  }
}

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  const name = nameInput.value.trim();
  if (state !== 'over' || submitted || submitting) return;
  if (name.length < 1 || name.length > 20) { submitStatus.textContent = 'Enter a name between 1 and 20 characters.'; nameInput.focus(); return; }
  submitting = true; submitButton.disabled = true; submitStatus.textContent = 'Saving score…';
  try {
    const response = await fetch(API_URL, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, score }) });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error || `Server returned ${response.status}`);
    submitted = true; submitStatus.textContent = 'Score saved!';
    renderScores(data.scores); scoresStatus.textContent = '';
  } catch (error) {
    submitStatus.textContent = `Could not save score: ${error.message}. You can retry.`;
  } finally { submitting = false; submitButton.disabled = false; }
});

reset();
loadScores();
