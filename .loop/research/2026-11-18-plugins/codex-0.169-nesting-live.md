codex 0.169.0 (not 0.168.0), model gpt-6-luna via myproxy, effort low. 11 live runs. Logs *.log, per-run thread depth tables *.threads.
Clean CODEX_HOME = ./ch (provider+model only). Host config has agents.max_depth=2, max_threads=15, features.multi_agent=true.
- Model selects multi_agent_v2 on its own: all runs used namespace `collaboration` (spawn_agent task_name/message, wait_agent). multi_agent_v1 never seen; features.multi_agent_v2=false did not switch to v1.
- Clean home default: depth 3 reached (l1,l2,l3); l3's spawn of l4 refused: "collab spawn failed: agent thread limit reached".
- agents.max_depth = 0, 1, 2, 3 had NO effect on depth (clean: d0 -> depth 3 reached; v2=true+d1 -> depth 3).
- agents.max_threads=15 (clean home) -> depth 4 reached (l4 ran). Host config (max_threads=15) -> depth 4 regardless of max_depth.
- Roles defined by -c agents.lN resolved at every depth (l3, l4 spawned from depth 2/3 children).
- Parent used wait_agent and replies embedded child results (synchronous via wait).
- One run (v2=true + max_depth=2 on host config) gave main no spawn tool at all (model said none); not reproduced with clean home.
