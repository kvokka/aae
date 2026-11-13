# Acceptance checks done by hand

Most criteria of the first release are scenarios of `crates/aae-acceptance` (see the README). A few cannot be: they
need the owner's real projects, a real exhausted quota or the passage of time. This page says how to check each one
and what to record. The platform of acceptance is the owner's host, Linux arm64.

Record every check as a line in the table at the end, with the date and the versions (`aae --version`,
`claude --version`, `codex --version`, `br --version`).

## Criterion 2, the codex half (when codex has a login)

The claude half is the request scenario `c02_prompt`. The codex half needs `codex login` for the user who runs the
spec. Run the codex contract suite once against the real binary (`cargo nextest run -p aae-codex`), then a one-step
prompt formula on the cheapest codex model through the daemon, as `c02_prompt` does for claude: an agent with
`harness = "codex"` and that model, a prompt asking to run `aae finish pass --summary done` once. Pass: the run ends
`passed` with one attempt (`aae run show <run>`).

## Criterion 10, the claude half: a quota with a reset time

Claude reports the reset time of a quota only to a subscription login, so this waits for the first real exhaustion.

1. Keep `default-policy` on (it is by default). When a claude run fails with a usage limit, `aae run show <run>`
   gives the failed attempt `failure_kind = quota_exhausted`, and its `run.attempt_failed` event (`aae events`) has
   `failure.retry_at`, the reset time.
2. `aae gate list` shows the gate `account:claude/<account>` closed by the policy until that time plus a minute
   (`quota_margin_s` of `[builtin.default-policy]`).
3. Start a second run of an agent on the same account: it stays `queued` (or `parked`) with that gate as the reason
   and its harness is not launched.
4. After the reset time both runs start and pass with no command from you.

If step 1 shows no `retry_at`, claude did not report a reset time: record it as such, with the claude version; the
gate is then closed for `quota_wait_s` (30 minutes by default).

## Criterion 20: ten real runs in a row

Pick a real pipeline of one of your projects on a real rig: the formulas, triggers and plugins you would use anyway.

1. Run `aae doctor`; fix what it reports as an error.
2. Start the pipeline ten times in a row (by hand, by a trigger or by a schedule) and do not intervene: no
   `aae run cancel`, no `aae run answer`, no edits in the rig, no restart of the daemon by hand.
3. Pass: all ten reach `passed` (`aae run list --project <project>`), and nothing needed you on the way. A run that
   waits on a policy gate and goes on by itself counts as no intervention; any command you had to type does not.

Record the project, the formula and the ten run ids. A failure is worth a note of where it happened (`aae events`
from the run's first `seq` shows the whole story).

## Criterion 21: a harness update costs at most two hours

At the next release of `claude` or `codex`, follow `docs/harness-updates.md` and time it from `mise upgrade` to the
committed `mise.lock` with the suite green. Pass: at most two hours. The log of that page records the date and the
versions; add the time spent to the line.

## Record

| Date | Criterion | Versions | Result | Notes |
|---|---|---|---|---|
