# Models (WP-H research, 2026-11-14)
## claude 2.1.325
--model help: "Provide an alias for the latest model (e.g. 'fable', 'opus', or 'sonnet') or a model's full name."
So the CLI resolves aliases itself. 'haiku' not named in help (cheat sheet 01 line 35: `--model <alias|id>`); not verified live.
## codex-cli 0.168.0, app-server model/list (includeHidden:true), no model call
Handshake: initialize{clientInfo,capabilities.experimentalApi} -> initialized notification -> model/list. Fields: id, model, displayName, hidden, isDefault, upgrade.
Visible: gpt-6.1-sol (default), gpt-6-astra, gpt-6-sol, gpt-6-luna, gpt-5.6-sol(upgrade gpt-6-sol), gpt-6.1-terra, gpt-5.6-terra(upgrade gpt-6-sol), gpt-5.6-luna(upgrade gpt-6-luna), gpt-5.5(upgrade gpt-6-sol)
Hidden: gpt-daybreak-blue-latest, gpt-daybreak-red-latest, codex-auto-review
No alias/family field; only `upgrade` (successor id).
Owner names: gpt-6-astra YES; gpt-6-luna YES; gpt-6-1-terra NO (real id gpt-6.1-terra, dot); gpt-6-1-sol NO (real gpt-6.1-sol); gpt-5.6-luna YES but deprecated -> gpt-6-luna.
Live calls: 0.
