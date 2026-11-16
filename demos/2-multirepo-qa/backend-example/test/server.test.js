const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { ScoreStore, createServer, sortScores, validateSubmission } = require('../server');

async function temporaryStore(t) {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'snake-scores-'));
  t.after(() => fs.rm(directory, { recursive: true, force: true }));
  return { directory, file: path.join(directory, 'scores.json') };
}

test('scores sort descending, preserve tie submission order, and retain only ten', () => {
  const entries = Array.from({ length: 12 }, (_, i) => ({ name: `Player${i}`, score: i < 3 ? 20 : 20 - i }));
  const result = sortScores(entries);
  assert.equal(result.length, 10);
  assert.deepEqual(result.slice(0, 3).map((entry) => entry.name), ['Player0', 'Player1', 'Player2']);
  assert.ok(result.every((entry, i) => i === 0 || result[i - 1].score >= entry.score));
});

test('submission validation trims names and rejects invalid fields', () => {
  assert.deepEqual(validateSubmission({ name: ' Ada ', score: 12 }), { name: 'Ada', score: 12 });
  for (const body of [{ name: '', score: 1 }, { name: 'x'.repeat(21), score: 1 }, { name: 'Ada', score: -1 }, { name: 'Ada', score: 1.5 }, { name: 'Ada', score: Number.MAX_SAFE_INTEGER + 1 }]) {
    assert.throws(() => validateSubmission(body));
  }
});

test('scores persist across store instances', async (t) => {
  const { file } = await temporaryStore(t);
  const first = new ScoreStore(file);
  assert.deepEqual(await first.list(), []);
  await first.add({ name: 'Ada', score: 12 });
  const restarted = new ScoreStore(file);
  assert.deepEqual(await restarted.list(), [{ name: 'Ada', score: 12 }]);
});

test('HTTP API supports CORS, valid posts, malformed input, and GET', async (t) => {
  const { file } = await temporaryStore(t);
  const server = createServer(new ScoreStore(file));
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const base = `http://127.0.0.1:${server.address().port}`;
  const options = await fetch(`${base}/api/scores`, { method: 'OPTIONS', headers: { Origin: 'http://localhost:5173' } });
  assert.equal(options.status, 204);
  assert.equal(options.headers.get('access-control-allow-origin'), 'http://localhost:5173');
  const posted = await fetch(`${base}/api/scores`, { method: 'POST', headers: { Origin: 'http://localhost:5173', 'Content-Type': 'application/json' }, body: JSON.stringify({ name: ' Ada ', score: 12 }) });
  assert.equal(posted.status, 201);
  assert.deepEqual(await posted.json(), { name: 'Ada', score: 12 });
  assert.equal(posted.headers.get('access-control-allow-origin'), 'http://localhost:5173');
  const malformed = await fetch(`${base}/api/scores`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{' });
  assert.equal(malformed.status, 400);
  assert.equal((await malformed.json()).error.length > 0, true);
  const listed = await fetch(`${base}/api/scores`);
  assert.deepEqual(await listed.json(), { scores: [{ name: 'Ada', score: 12 }] });
});
