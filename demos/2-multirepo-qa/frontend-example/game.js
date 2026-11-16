(() => {
  'use strict';

  const API = 'http://localhost:3000/api/scores';
  const SIZE = 20;
  const STEP_MS = 125;
  const canvas = document.querySelector('#board');
  const ctx = canvas.getContext('2d');
  const scoreNode = document.querySelector('#score');
  const bestNode = document.querySelector('#best');
  const statusNode = document.querySelector('#game-status');
  const overlay = document.querySelector('#overlay');
  const overlayIcon = document.querySelector('#overlay-icon');
  const overlayTitle = document.querySelector('#overlay-title');
  const overlayCopy = document.querySelector('#overlay-copy');
  const startButton = document.querySelector('#start-button');
  const leaderboard = document.querySelector('#leaderboard');
  const apiMessage = document.querySelector('#api-message');
  const form = document.querySelector('#score-form');
  const nameInput = document.querySelector('#player-name');
  const submitButton = document.querySelector('#submit-button');
  const submitMessage = document.querySelector('#submit-message');

  const vectors = { ArrowUp: { x: 0, y: -1 }, w: { x: 0, y: -1 }, ArrowDown: { x: 0, y: 1 }, s: { x: 0, y: 1 }, ArrowLeft: { x: -1, y: 0 }, a: { x: -1, y: 0 }, ArrowRight: { x: 1, y: 0 }, d: { x: 1, y: 0 } };
  let snake, food, direction, turns, score, best, timer, state, runId, submittedRun;
  const cell = canvas.width / SIZE;

  function randomFood() {
    const free = [];
    for (let y = 0; y < SIZE; y++) for (let x = 0; x < SIZE; x++) {
      if (!snake.some(part => part.x === x && part.y === y)) free.push({ x, y });
    }
    return free.length ? free[Math.floor(Math.random() * free.length)] : null;
  }

  function reset() {
    clearInterval(timer);
    timer = null;
    snake = [{ x: 9, y: 10 }, { x: 8, y: 10 }, { x: 7, y: 10 }];
    direction = { x: 1, y: 0 };
    turns = [];
    score = 0;
    best = 0;
    food = randomFood();
    state = 'ready';
    runId++;
    scoreNode.textContent = '00';
    bestNode.textContent = String(best).padStart(2, '0');
    statusNode.textContent = 'READY';
    form.hidden = true;
    submitMessage.textContent = '';
    nameInput.value = '';
    startButton.innerHTML = 'Start game <span>↗</span>';
    overlayIcon.textContent = '↗';
    overlayTitle.textContent = 'Ready?';
    overlayCopy.textContent = 'Press start or use an arrow key.';
    overlay.classList.remove('hidden');
    draw();
  }

  function draw() {
    const scale = canvas.width / SIZE;
    ctx.fillStyle = '#101a18';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.strokeStyle = 'rgba(185, 213, 186, .045)';
    ctx.lineWidth = 1;
    for (let i = 1; i < SIZE; i++) {
      ctx.beginPath(); ctx.moveTo(i * scale, 0); ctx.lineTo(i * scale, canvas.height); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(0, i * scale); ctx.lineTo(canvas.width, i * scale); ctx.stroke();
    }
    if (food) {
      ctx.fillStyle = 'rgba(182,239,101,.18)'; ctx.beginPath(); ctx.arc((food.x + .5) * scale, (food.y + .5) * scale, scale * .42, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = '#b6ef65'; ctx.beginPath(); ctx.arc((food.x + .5) * scale, (food.y + .5) * scale, scale * .22, 0, Math.PI * 2); ctx.fill();
    }
    snake.forEach((part, index) => {
      const gap = 2.2;
      ctx.fillStyle = index === 0 ? '#d0fa91' : '#89c855';
      const radius = index === 0 ? 5 : 4;
      ctx.beginPath(); ctx.roundRect(part.x * scale + gap, part.y * scale + gap, scale - gap * 2, scale - gap * 2, radius); ctx.fill();
    });
    if (snake.length) {
      const head = snake[0];
      const eye = scale * .11;
      const ex = direction.x ? (direction.x > 0 ? .69 : .31) : .5;
      const ey = direction.y ? (direction.y > 0 ? .69 : .31) : .5;
      ctx.fillStyle = '#17301b';
      for (const side of [-1, 1]) {
        const px = direction.x ? ex * scale : (.5 + side * .19) * scale;
        const py = direction.y ? ey * scale : (.5 + side * .19) * scale;
        ctx.beginPath(); ctx.arc((head.x + px / scale) * scale, (head.y + py / scale) * scale, eye, 0, Math.PI * 2); ctx.fill();
      }
    }
  }

  function start() {
    if (state === 'playing') return;
    if (state === 'over' || state === 'won') reset();
    state = 'playing';
    statusNode.textContent = 'IN PLAY';
    overlay.classList.add('hidden');
    startButton.innerHTML = 'Restart game <span>↗</span>';
    timer = setInterval(tick, STEP_MS);
  }

  function finish(won = false) {
    clearInterval(timer);
    timer = null;
    state = won ? 'won' : 'over';
    statusNode.textContent = won ? 'YOU WIN' : 'GAME OVER';
    overlayIcon.textContent = won ? '✳' : '↻';
    overlayTitle.textContent = won ? 'Board cleared!' : 'That’s a wrap.';
    overlayCopy.textContent = won ? `You claimed all ${SIZE * SIZE} cells.` : `You scored ${score}. Ready for another?`;
    overlay.classList.remove('hidden');
    startButton.innerHTML = 'Play again <span>↗</span>';
    form.hidden = false;
    nameInput.focus();
  }

  function tick() {
    if (turns.length) direction = turns.shift();
    const head = { x: snake[0].x + direction.x, y: snake[0].y + direction.y };
    const eating = food && head.x === food.x && head.y === food.y;
    const bodyToCheck = eating ? snake : snake.slice(0, -1);
    if (head.x < 0 || head.y < 0 || head.x >= SIZE || head.y >= SIZE || bodyToCheck.some(part => part.x === head.x && part.y === head.y)) {
      finish();
      return;
    }
    snake.unshift(head);
    if (eating) {
      score++;
      scoreNode.textContent = String(score).padStart(2, '0');
      best = Math.max(best, score);
      bestNode.textContent = String(best).padStart(2, '0');
      food = randomFood();
      if (!food) { draw(); finish(true); return; }
    } else snake.pop();
    draw();
  }

  function changeDirection(next) {
    const last = turns.length ? turns[turns.length - 1] : direction;
    if (next.x === -last.x && next.y === -last.y) return;
    if (next.x === last.x && next.y === last.y) return;
    if (turns.length < 2) turns.push(next);
  }

  async function loadScores() {
    leaderboard.replaceChildren(Object.assign(document.createElement('p'), { className: 'leader-message', textContent: 'Loading scores…' }));
    apiMessage.textContent = '';
    try {
      const response = await fetch(API);
      const data = await response.json();
      if (!response.ok) throw new Error(data.error || `Server returned ${response.status}`);
      if (!Array.isArray(data.scores)) throw new Error('The score server returned an unexpected response.');
      leaderboard.replaceChildren();
      if (!data.scores.length) {
        leaderboard.append(Object.assign(document.createElement('p'), { className: 'leader-message', textContent: 'No scores yet. Be the first to make the board.' }));
      } else data.scores.slice(0, 10).forEach((entry, index) => {
        const row = document.createElement('div'); row.className = 'score-row';
        const rank = document.createElement('span'); rank.className = 'rank'; rank.textContent = String(index + 1).padStart(2, '0');
        const name = document.createElement('span'); name.className = 'score-name'; name.textContent = entry.name;
        const value = document.createElement('span'); value.className = 'score-value'; value.textContent = String(entry.score).padStart(2, '0');
        row.append(rank, name, value); leaderboard.append(row);
      });
    } catch (error) {
      leaderboard.replaceChildren(Object.assign(document.createElement('p'), { className: 'leader-message', textContent: 'Scores are unavailable right now.' }));
      apiMessage.textContent = `${error.message || 'Could not reach the score server.'} You can still play; try refreshing scores later.`;
    }
  }

  async function submitScore(event) {
    event.preventDefault();
    if (state !== 'over' && state !== 'won') return;
    if (submittedRun === runId || submitButton.disabled) return;
    submitButton.disabled = true;
    submitMessage.textContent = 'Submitting…';
    try {
      const response = await fetch(API, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name: nameInput.value, score }) });
      const data = await response.json();
      if (!response.ok) throw new Error(data.error || `Server returned ${response.status}`);
      submittedRun = runId;
      submitMessage.textContent = `Score saved as ${data.name}.`;
      await loadScores();
    } catch (error) {
      submitMessage.textContent = `${error.message || 'Could not submit score.'} Please try again.`;
    } finally {
      submitButton.disabled = false;
    }
  }

  document.addEventListener('keydown', event => {
    const next = vectors[event.key] || vectors[event.key.toLowerCase()];
    if (!next) return;
    if (state === 'playing') event.preventDefault();
    if (state === 'ready') start();
    if (state === 'over' || state === 'won') return;
    changeDirection(next);
  });
  startButton.addEventListener('click', () => {
    if (state === 'playing') reset();
    start();
  });
  form.addEventListener('submit', submitScore);

  best = 0;
  runId = 0;
  submittedRun = -1;
  reset();
  loadScores();
})();
