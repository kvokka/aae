'use strict';

const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs/promises');
const path = require('node:path');

const baseUrl = 'http://localhost:3000';
const dataFile = path.join(__dirname, 'scores.json');
let server;

async function start() {
  server = spawn('npm', ['start'], { cwd: __dirname, stdio: 'ignore' });
  for (let attempt = 0; attempt < 50; attempt += 1) {
    if (server.exitCode !== null) throw new Error(`Server exited with ${server.exitCode}`);
    try {
      await fetch(`${baseUrl}/api/high-scores`);
      return;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
  throw new Error('Server did not start');
}

async function stop() {
  if (!server || server.exitCode !== null) return;
  server.kill('SIGTERM');
  await new Promise((resolve) => server.once('exit', resolve));
  server = null;
}

async function jsonResponse(response) {
  return { status: response.status, body: await response.json() };
}

async function post(name, score) {
  return jsonResponse(await fetch(`${baseUrl}/api/high-scores`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, score }),
  }));
}

async function main() {
  await fs.rm(dataFile, { force: true });
  await start();
  assert.deepEqual(await jsonResponse(await fetch(`${baseUrl}/api/high-scores`)), { status: 200, body: { scores: [] } });

  const preflight = await fetch(`${baseUrl}/api/high-scores`, {
    method: 'OPTIONS',
    headers: { Origin: 'http://localhost:5173', 'Access-Control-Request-Method': 'POST', 'Access-Control-Request-Headers': 'content-type' },
  });
  assert.equal(preflight.status, 204);
  assert.equal(preflight.headers.get('access-control-allow-origin'), '*');
  assert.match(preflight.headers.get('access-control-allow-headers'), /content-type/i);

  assert.deepEqual(await post('  Ada  ', 12), { status: 201, body: { scores: [{ name: 'Ada', score: 12 }] } });
  assert.equal((await post('   ', 1)).status, 400);
  assert.equal((await post('x'.repeat(21), 1)).status, 400);
  assert.equal((await post('Ada', -1)).status, 400);
  assert.equal((await post('Ada', Number.MAX_SAFE_INTEGER + 1)).status, 400);
  assert.equal((await fetch(`${baseUrl}/api/high-scores`, { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: '{}' })).status, 400);
  assert.equal((await fetch(`${baseUrl}/api/high-scores`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{' })).status, 400);

  await Promise.all(Array.from({ length: 14 }, (_, index) => post(`Player${index}`, index)));
  const sorted = await jsonResponse(await fetch(`${baseUrl}/api/high-scores`));
  assert.equal(sorted.status, 200);
  assert.equal(sorted.body.scores.length, 10);
  assert.deepEqual(sorted.body.scores.map((entry) => entry.score), [13, 12, 12, 11, 10, 9, 8, 7, 6, 5]);
  assert.deepEqual(sorted.body.scores.filter((entry) => entry.score === 12).map((entry) => entry.name), ['Ada', 'Player12']);
  await stop();
  await start();
  const persisted = await jsonResponse(await fetch(`${baseUrl}/api/high-scores`));
  assert.deepEqual(persisted, sorted);
  await stop();
  await fs.rm(dataFile, { force: true });
  console.log('API checks passed: empty GET, POST, validation, CORS, top 10, and restart persistence.');
}

main().catch(async (error) => {
  await stop();
  await fs.rm(dataFile, { force: true }).catch(() => {});
  console.error(error);
  process.exitCode = 1;
});
