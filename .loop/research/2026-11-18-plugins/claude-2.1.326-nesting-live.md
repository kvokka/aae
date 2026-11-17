# Nesting findings, claude 2.1.326, haiku everywhere (main and agents), 8 live runs
Flags: claude -p --output-format stream-json --verbose --setting-sources project,local --permission-mode bypassPermissions --model haiku --settings settings.json --agents agents.json
Files: *.hooks (hook stdin JSON), *.out (stream-json). s3_deny.* was a run invalidated by a settings.json race (parallel runs); s3_deny2 is the valid one.
S1 no env: main->l1->l2 ran; l2 had no Agent (NOAGENT-L2). 2 levels below main.
S2 DEPTH=1: only l1 (no Agent in l1). =2: l1,l2 (l2 no Agent). =3: l1..l3 (l3 NOAGENT). =5: l1..l4 all ran. Default == =2.
S3 permissions.deny ["Agent(l3)"]: l2's PreToolUse for l3 fired, but no SubagentStart l3; l2 reported denial by rule from flagSettings. disallowedTools ["Agent"] on l2 only: l2 NOAGENT, l1 fine.
S4 sync: SubagentStop of child always precedes parent's Stop; parents waited. PreToolUse run_in_background False/None at levels main, l1, l2 (model chose it; not forced).
