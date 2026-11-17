# codex 0.168.0 app-server: per-thread skills and subagent roles

Date 2026-11-17, codex-cli 0.168.0 (`/home/ubuntu/.local/share/mise/installs/codex/0.168.0/bin/codex`), linux-arm64 orbstack.
Method: fake Responses API (`bin/fake.py`, logs every request body to `runs/<scenario>/requests.jsonl`; a user
text `CALL <ns>/<tool> {json}` makes it answer with that `function_call`, a request ending in
`function_call_output` gets "Done.", anything else "OK"), one `codex app-server --listen stdio://` per scenario with a
fresh `HOME`/`CODEX_HOME`, env = PATH, HOME, CODEX_HOME, SHELL only. Client: `bin/rig.py`. Every thread uses its own
`model` name so the provider log tells threads apart. `approvalPolicy: never`, `sandbox: danger-full-access`.
Scenario scripts: `bin/q1.py q2.py q3.py q5.py q6.py q7.py`; app-server message logs: `runs/<scenario>/messages.jsonl`.

Skills reach the provider as a `<skills_instructions>` developer message, one line per skill:
`- sk-alpha: Marker skill sk-alpha. (file: r0/sk-alpha/SKILL.md)`.

## Q1. Disable a process-level skill for one thread: WORKS

Key: `thread/start.config.skills.config = [{path: <abs path to SKILL.md>, enabled: false}]` or
`[{name: "<skill name>", enabled: false}]` (same shape as `[[skills.config]]` in config.toml and as
`skills/config/write` params). A directory path (without `/SKILL.md`) does nothing. A nonexistent path is ignored
silently.

`runs/q1` (skills sk-alpha, sk-beta in `$CODEX_HOME/skills`), six threads in one process, one turn each:
```
1 m-base      {'sk-alpha': True,  'sk-beta': True}
2 m-off-path  {'sk-alpha': False, 'sk-beta': True}   # {"path": ".../ch/skills/sk-alpha/SKILL.md", "enabled": false}
3 m-off-name  {'sk-alpha': False, 'sk-beta': True}   # {"name": "sk-alpha", "enabled": false}
4 m-off-dir   {'sk-alpha': True,  'sk-beta': True}   # path = skill directory -> no effect
6 m-base2     {'sk-alpha': True,  'sk-beta': True}   # thread started after the disabling ones: unaffected
```
`skills/list` (process level) still reports both as enabled. Same result for skills from `skills/extraRoots/set`
(`runs/q2`: `m-off-e1` lost sk-e1 only) and for bundled system skills (`{"name": "skill-creator", "enabled": false}`
removed skill-creator from that thread only; `skills.bundled.enabled = false` per thread removed all bundled ones).

Allow-list form also works (`runs/q6`): process config.toml disables every skill
(`[[skills.config]] path=... enabled=false`), a thread re-enables its own:
```
m-base          {'sk-a': False, 'sk-b': False}
m-enable-a      {'sk-a': True,  'sk-b': False}   # config.skills.config=[{path: sk-a/SKILL.md, enabled: true}]
m-enable-b-name {'sk-a': False, 'sk-b': True}    # config.skills.config=[{name: "sk-b", enabled: true}]
```

## Q2. Add a skill (path outside any root) to one thread: DOES NOT WORK directly

- `config.skills.config = [{path: /outside/sk-extra/SKILL.md, enabled: true}]`: skill absent from the request.
- guessed keys `config.skills.{extra_roots, roots, paths}`: no effect, no warning. `SkillsConfig` in the binary has
  only `bundled`, `include_instructions`, `max_context_tokens`, `config`.
- `turn/start.input` item `{type: "skill", name, path}` with a path outside the roots: turn accepted, skill name and
  body ("Say sk-extra") not in the request.
Working per-thread routes: (a) superset root at process level (`skills/extraRoots/set` or `$CODEX_HOME/skills`) +
per-thread enable/disable (Q1, allow-list form above); (b) the thread's `cwd` `.agents/skills` (rig, known from
experiment 4).

## Q3. Subagent roles per thread: WORKS (both v1 and v2)

Features in 0.168: `multi_agent` (deprecated, **on** by default) gives namespace `multi_agent_v1`
{spawn_agent, send_input, resume_agent, wait_agent, close_agent}; `spawn_agent` params: message|items, agent_type,
model, reasoning_effort, fork_context. `multi_agent_v2` (stable, off) gives namespace `collaboration`
{spawn_agent, send_message, followup_task, wait_agent, list_agents, interrupt_agent}; spawn_agent requires
`task_name` (lowercase/digits/underscore only, else "agent_name must use only lowercase letters, digits, and
underscores") and `message`, plus agent_type, fork_turns ("all"|"none", default all), model, reasoning_effort.
v2 can be switched on per thread: `config.features.multi_agent_v2 = true` gave that thread the `collaboration`
tools in a v1 process (`runs/q5` t8). Roles behave the same in v1 and v2.

Role definition (`AgentRoleToml` = description, config_file, nickname_candidates):
- process: `[agents.<role>] description=..., config_file=...` in config.toml; also discovered files
  `$CODEX_HOME/agents/<role>.toml` (re-read at each thread start) — these need `description` and
  `developer_instructions` inside, else "Ignoring malformed agent role definition ... must define
  `developer_instructions`".
- per thread: `thread/start.config.agents.<role> = {description, config_file}` — **visible to that thread only**;
  also `<cwd>/.codex/agents/<role>.toml` (per thread by cwd, `runs/q5` t7).
- The role list goes into the `agent_type` parameter description of spawn_agent:
  `proc-role: {\nPROC-ROLE-DESC\n- This role's model is set to `m-child-proc-role` and cannot be changed.\n}` then
  built-ins default, explorer, worker.
- Role config file is strictly parsed as a config layer: an unknown key (e.g. `bypass_hook_trust`) makes the whole
  role "malformed" and dropped (`configWarning`), and spawn then fails with "unknown agent_type '<role>'".

`runs/q3v1` (A has per-thread role thread-role, B none; process role proc-role):
```
2  m-parent-b  agent_type lists proc-role only (THREAD-ROLE-DESC absent)
7  m-parent-b  function_call_output: "unknown agent_type 'thread-role'"
5  m-child-thread-role ['DI-MARKER-thread-role', 'skill-creator', 'tag_parentmcpa', 'child task']   # child of A
14 m-child-proc-role   ['DI-MARKER-proc-role', 'skill-creator', 'child task']                       # child of B
   tools of children: exec_command, write_stdin, request_user_input, view_image, [parent MCP], goals, web_search
   — no spawn tools (default max_depth 1)
```
Child request: model = role's `model` (else the parent's model, `runs/q7*`); developer instructions = role's
`developer_instructions`, replacing the parent's `developerInstructions` (PARENT-DI absent); with
`agent_type: "default"` the child keeps PARENT-DI. Skills: the role file's `[[skills.config]]` applies (sk-proc
disabled in every child of a role that disables it, still present in parents), and the parent's per-thread
`skills.config` is inherited too (`runs/q7v1`, `q7v2`: sk-p disabled in parent -> absent in both children).
v2 child input is an `agent_message` item ("Message Type: NEW_TASK ... Payload"), v1 child input is a user message.

## Q4. Inheritance of per-thread hooks / MCP; role-file hooks / MCP / skills / DI

| What | Child gets it? | Evidence |
|---|---|---|
| parent `config.mcp_servers` (per thread) | **yes** | `tag_parentmcpa` in children of A, not of B; only one MCP process started (`mcp.pids` has one line) — child shares the parent's server |
| parent `config.hooks` (+ `bypass_hook_trust`) | **yes** | `hook/completed` on the child threadId, `source: sessionFlags`: v1 `subagentStart, userPromptSubmit, preModelRequest, subagentStop`; v2 `subagentStart, preModelRequest, subagentStop`. `SessionStart`/`Stop` do not fire for the child (SubagentStart/SubagentStop instead). Hook stdin on the child: `session_id` = **parent** thread id, plus `agent_id` (child id) and `agent_type` |
| role file `[mcp_servers.x]` | **no** | server never started, tool absent in child request |
| role file `[hooks]` (UserPromptSubmit, Stop, SessionStart, PreModelRequest, SubagentStart/Stop) | **no** | no ROLEHOOK line, no hook/* with another source; `bypass_hook_trust` cannot be put in a role file (unknown field -> role dropped) |
| role file `[[skills.config]]` | **yes** | see Q3 |
| role file `developer_instructions`, `model` | **yes** | see Q3 |

v1 hook log (`runs/q3v1`, threadId suffix):
```
464b5e sessionStart / userPromptSubmit / preModelRequest / stop       # parent turn
6a3f4a subagentStart sessionFlags                                       # child (thread-role)
6a3f4a userPromptSubmit sessionFlags
6a3f4a preModelRequest sessionFlags
6a3f4a subagentStop sessionFlags
```

## Q5. Restrict which roles a thread may spawn: NO (only add/override, or turn spawning off)

`runs/q5`, roles listed in each thread's spawn_agent `agent_type`:
```
t1 base                                  home-role, proc-role, default, explorer, worker
t3 agents.proc-role={description:"OVERRIDE-DESC"}      proc-role now OVERRIDE-DESC (override works)
t4 agents.proc-role={}                   unchanged, no warning
t6 agents.explorer={description,config_file}  explorer overridden; built-ins cannot be removed
t9 agents.own-role={...}                 own-role added, all others still there
t2 features.multi_agent=false            no spawn tools at all
t5 agents.max_depth=0                    no spawn tools at all
q6 agents.proc-role={description, config_file:"/nonexistent.toml"}  warning "Ignoring malformed agent role
   definition", proc-role still listed
```
`AgentRoleToml` has no `enabled`/hide field. So a thread sees: process roles (config.toml `[agents.*]` +
`$CODEX_HOME/agents/*.toml`) + its cwd's `.codex/agents/*.toml` + its own `config.agents.*` + built-ins
default/explorer/worker. To give threads different role sets in one process: define no roles at process level and
pass each thread its roles in `config.agents`; the built-ins stay visible (they can be redefined per thread).

## Consequences for one shared app-server

- Different skill sets per thread: yes, as long as every skill lives in a process-level root (CODEX_HOME/skills or
  extra roots): per-thread `skills.config` enable/disable by path or name. Skills outside the roots cannot be added
  per thread except through the thread's cwd.
- Different subagent sets per thread: yes, via `config.agents.<role>` per thread (keep process-level roles empty);
  built-ins remain. Children inherit the parent thread's MCP servers, hooks (as subagent events, session_id of the
  parent) and skills.config; per-role MCP and hooks are not possible through the role file.

## Caveats
- n=1 per case. Fake provider only; nothing about how a real model chooses roles.
- `/tmp` CODEX_HOME prints "Refusing to create helper binaries under temporary dir" (harmless here).
- The bubblewrap `configWarning` is from the container (no user namespaces), unrelated.
