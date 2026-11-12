#!/usr/bin/env python3
"""Хуки харнесса автономного цикла aae (одна точка входа на все события).

Что делает:
  * контекст субагентов: >= AAE_CTX_SOFT (300k) — просьба сворачиваться (повтор каждые +30k),
    >= AAE_CTX_HARD (420k) — принудительная остановка субагента и автоматический дамп
    в .loop/handoffs/ для родителя;
  * PreCompact: при AAE_LOOP=1 сжатие разрешено только главному потоку (overseer),
    у рабочих агентов оно блокируется — их режет порог выше;
  * политика Agent при AAE_LOOP=1: кто кого может запускать, без переопределения model,
    не больше AAE_MAX_CHILDREN одновременно живых субагентов у кодера;
  * защита файлов харнесса и опасных git-команд;
  * Stop/SessionStart главного потока при AAE_LOOP=1: не даёт мастеру остановиться,
    пока цикл не завершён, и после сжатия напоминает, где состояние;
  * SubagentStop coder/reviewer: не отпускает без отчёта нужного формата.

Проверено на claude 2.1.322 (2026-11-12): continue:false из хука субагента останавливает
только субагента, родитель получает "(Subagent completed but returned no output.)",
SubagentStop при этом не приходит; transcript_path в хуке субагента указывает на транскрипт
родителя; PreCompact внутри субагента приходит с agent_type родителя и без agent_id.
"""
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

SOFT = int(os.environ.get("AAE_CTX_SOFT", "300000"))
HARD = int(os.environ.get("AAE_CTX_HARD", "420000"))
WARN_STEP = int(os.environ.get("AAE_CTX_WARN_STEP", "30000"))
MAX_CHILDREN = int(os.environ.get("AAE_MAX_CHILDREN", "3"))
# Мастер (Sonnet) сжимается при окне modelSettings.claude-sonnet-5-5.autoCompactWindow=300000 (порог ~267k).
MASTER_COMPACT_AT = int(os.environ.get("AAE_MASTER_COMPACT_AT", "240000"))
LOOP = os.environ.get("AAE_LOOP") == "1"

ROOT = Path(os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd())
LOOPDIR = ROOT / ".loop"
RUN = LOOPDIR / "run"

MAIN = "<main>"
SPAWN_POLICY = {
    MAIN: {"coder", "reviewer"},
    "coder": {"helper", "subcoder", "expert"},
}
CHILD_TYPES = {"helper", "subcoder", "expert"}
_PROT = r"(\.claude/|scripts/loop/|CLAUDE\.md)"
BASH_WRITE = re.compile(r"(>>?\s*['\"]?\S*" + _PROT + r"|\b(tee|sed\s+-i\S*|perl\s+-i\S*|mv|cp|rm|truncate|chmod|ln|install)\b[^|;&]*" + _PROT + r")")
BASH_DENY = [
    (re.compile(r"\bgit\s+push\b.*(\s--force\b|\s-f\b|\s--force-with-lease|\s\+\S)"), "force-push запрещён"),
    (re.compile(r"\bgit\s+push\b.*\s--delete\b"), "удаление веток на remote запрещено"),
    (re.compile(r"\bgit\s+(filter-branch|filter-repo)\b"), "переписывание истории запрещено"),
    (re.compile(r"\bgit\s+commit\b.*--no-verify"), "--no-verify запрещён"),
    (re.compile(r"\bgit\s+config\s+--global\b"), "глобальный git config не трогаем"),
    (re.compile(r"\brm\s+-[a-zA-Z]*r[a-zA-Z]*f?\s+(/|~|\$HOME)(\s|$)"), "rm -r по корню/дому запрещён"),
]


# ---------------------------------------------------------------- utils
def out(obj):
    print(json.dumps(obj, ensure_ascii=False))
    sys.exit(0)


def deny(reason):
    out({"hookSpecificOutput": {"hookEventName": "PreToolUse",
                                "permissionDecision": "deny",
                                "permissionDecisionReason": reason}})


def context(event, text):
    out({"hookSpecificOutput": {"hookEventName": event, "additionalContext": text}})


def now_iso():
    return time.strftime("%Y-%m-%dT%H:%M:%S%z")


def read_state():
    try:
        return json.loads((LOOPDIR / "state.json").read_text())
    except Exception:
        return {}


def transcript_for(d):
    """Транскрипт агента, в котором сработал хук."""
    tp = Path(d.get("transcript_path") or "")
    aid = d.get("agent_id")
    if not aid:
        return tp
    sid = d.get("session_id", "")
    cands = [tp.parent / sid / "subagents" / f"agent-{aid}.jsonl"]
    if tp.parent.exists():
        cands += list(tp.parent.glob(f"{sid}/subagents/**/agent-{aid}.jsonl"))
    cands += list((Path.home() / ".claude" / "projects").glob(f"*/{sid}/subagents/**/agent-{aid}.jsonl"))
    for c in cands:
        if c.exists():
            return c
    return None


def tail_lines(path, max_bytes=8 << 20):
    """Строки с конца файла (читает окно с конца, растит его до max_bytes)."""
    try:
        size = path.stat().st_size
    except Exception:
        return []
    win = 1 << 20
    while True:
        start = max(0, size - win)
        with open(path, "rb") as f:
            f.seek(start)
            data = f.read()
        lines = data.split(b"\n")
        if start > 0:
            lines = lines[1:]
        yield_lines = [l for l in lines if l.strip()]
        if start == 0 or win >= max_bytes or any(b'"usage"' in l for l in yield_lines):
            return list(reversed(yield_lines))
        win *= 4


def ctx_tokens(path):
    """Размер контекста = вход последнего ответа модели (+ его выход)."""
    if not path:
        return 0
    for raw in tail_lines(path):
        try:
            e = json.loads(raw)
        except Exception:
            continue
        if e.get("type") == "system" and e.get("subtype") == "compact_boundary":
            return 0
        msg = e.get("message") or {}
        if e.get("type") == "assistant" and msg.get("usage") and msg.get("model") != "<synthetic>":
            u = msg["usage"]
            return sum(int(u.get(k) or 0) for k in (
                "input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens", "output_tokens"))
    return 0


def last_activity(path, n_text=3, n_tools=12, limit=3000):
    """Последние тексты агента и его последние вызовы инструментов (от старых к новым)."""
    texts, tools = [], []
    for raw in tail_lines(path):
        try:
            e = json.loads(raw)
        except Exception:
            continue
        if e.get("type") != "assistant":
            continue
        for block in reversed((e.get("message") or {}).get("content") or []):
            if not isinstance(block, dict):
                continue
            if block.get("type") == "text" and block.get("text", "").strip() and len(texts) < n_text:
                texts.append(block["text"].strip()[:limit])
            elif block.get("type") == "tool_use" and len(tools) < n_tools:
                tools.append(f"{block.get('name')}: {json.dumps(block.get('input'), ensure_ascii=False)[:240]}")
        if len(texts) >= n_text and len(tools) >= n_tools:
            break
    return list(reversed(texts)), list(reversed(tools))


def sh(cmd, limit=60):
    try:
        r = subprocess.run(cmd, shell=True, cwd=ROOT, capture_output=True, text=True, timeout=20)
        lines = (r.stdout + r.stderr).splitlines()
        more = f"\n… (+{len(lines) - limit} строк)" if len(lines) > limit else ""
        return "\n".join(lines[:limit]) + more
    except Exception as ex:
        return f"(ошибка: {ex})"


# ---------------------------------------------------------------- markers
def marker_dir(name):
    p = RUN / name
    p.mkdir(parents=True, exist_ok=True)
    return p


def alive(info):
    """Агент жив: есть маркер и его транскрипт недавно писался (SubagentStop может не прийти)."""
    tr = Path(info.get("transcript") or "")
    if tr.is_file():
        return time.time() - tr.stat().st_mtime < 1200
    return time.time() - info.get("t", 0) < 600


def live_agents(types):
    infos = []
    for m in marker_dir("agents").glob("*.json"):
        try:
            infos.append(json.loads(m.read_text()))
        except Exception:
            continue
    # кодер, который ждёт своих субагентов, транскрипт не пишет — он жив, пока жив кто-то из них
    kids = any(i.get("type") in CHILD_TYPES and alive(i) for i in infos)
    return [i for i in infos if i.get("type") in types
            and (alive(i) or (i.get("type") == "coder" and kids))]


def running_children():
    return len(live_agents(CHILD_TYPES))


# ---------------------------------------------------------------- hard cut
def hard_cut(d, path, ctx):
    aid = d.get("agent_id") or "main"
    atype = d.get("agent_type") or "?"
    cutmark = marker_dir("cut") / aid
    (marker_dir("agents") / f"{aid}.json").unlink(missing_ok=True)
    if cutmark.exists():
        return json.loads(cutmark.read_text()).get("handoff")
    st = read_state()
    cycle = int(st.get("cycle") or 0)
    base = st.get("base") or "HEAD~5"
    hdir = LOOPDIR / "handoffs"
    hdir.mkdir(parents=True, exist_ok=True)
    name = f"{time.strftime('%Y%m%dT%H%M%S')}-c{cycle:02d}-{atype}-{aid[:8]}.md"
    hp = hdir / name
    texts, tools = last_activity(path) if path else ([], [])
    body = [
        f"# Автоматический дамп: {atype} обрезан по контексту",
        "",
        f"- время: {now_iso()}",
        f"- агент: `{atype}` id `{aid}`; цикл {cycle}",
        f"- контекст: {ctx // 1000}k токенов при жёстком пороге {HARD // 1000}k",
        f"- транскрипт: `{path}`",
        f"- отчёт агента (если вёл): `.loop/cycles/{cycle:02d}/{atype}.md` или файлы, которые он назвал ниже",
        "",
        "Агент остановлен хуком без возможности дописать итог. Ниже — состояние как есть.",
        "",
        "## git status --short",
        "```", sh("git status --short", 80), "```",
        f"## git log {base}..HEAD",
        "```", sh(f"git log --oneline {base}..HEAD", 30), "```",
        "## git diff --stat (рабочее дерево к HEAD)",
        "```", sh("git diff --stat HEAD", 40), "```",
        "## Последние вызовы инструментов",
        "```", "\n".join(tools) or "(нет)", "```",
        "## Последние сообщения агента",
    ]
    for i, t in enumerate(texts, 1):
        body += ["", f"### {i}", "", t]
    hp.write_text("\n".join(body) + "\n")
    rel = str(hp.relative_to(ROOT))
    cutmark.write_text(json.dumps({"handoff": rel, "t": time.time()}))
    with open(marker_dir(".") / "cuts.jsonl", "a") as f:
        f.write(json.dumps({"ts": now_iso(), "agent_type": atype, "agent_id": aid, "cycle": cycle,
                            "ctx": ctx, "handoff": rel}, ensure_ascii=False) + "\n")
    return rel


def cut_reason(rel):
    return (f"ЖЁСТКИЙ ПОРОГ КОНТЕКСТА {HARD // 1000}k: агент остановлен, инструменты отключены. "
            f"Дамп состояния для родителя: {rel}")


# ---------------------------------------------------------------- events
def on_pre_tool(d):
    tool = d.get("tool_name", "")
    ti = d.get("tool_input") or {}
    aid = d.get("agent_id")
    is_sub = bool(aid)

    if tool == "Bash":
        cmd = ti.get("command", "")
        for rx, why in BASH_DENY:
            if rx.search(cmd):
                deny(f"Запрещено правилами харнесса: {why}.")
        if LOOP and BASH_WRITE.search(cmd):
            deny("Файлы харнесса (.claude/, scripts/loop/, CLAUDE.md) в цикле не меняются. "
                 "Предложение — в .loop/JOURNAL.md.")
    if LOOP and tool in ("Edit", "Write", "MultiEdit", "NotebookEdit"):
        fp = str(ti.get("file_path") or ti.get("notebook_path") or "")
        rel = os.path.relpath(fp, ROOT) if os.path.isabs(fp) else fp
        if rel.startswith((".claude/", "scripts/loop/")) or rel == "CLAUDE.md":
            deny("Файлы харнесса (.claude/, scripts/loop/, CLAUDE.md) в цикле не меняются. "
                 "Предложение — в .loop/JOURNAL.md.")

    ctx = 0
    if is_sub:
        cutmark = RUN / "cut" / aid
        if cutmark.exists():
            deny(cut_reason(json.loads(cutmark.read_text()).get("handoff")))
        path = transcript_for(d)
        ctx = ctx_tokens(path)
        if ctx >= HARD:
            rel = hard_cut(d, path, ctx)
            out({"continue": False, "stopReason": cut_reason(rel),
                 "hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
                                        "permissionDecisionReason": cut_reason(rel)}})

    if tool == "Agent" and LOOP:
        caller = d.get("agent_type") if is_sub else MAIN
        target = ti.get("subagent_type") or "general-purpose"
        allowed = SPAWN_POLICY.get(caller, set())
        if target not in allowed:
            deny(f"Политика цикла: `{caller}` может запускать только {sorted(allowed) or 'никого'}; "
                 f"`{target}` запрещён.")
        if ti.get("model"):
            deny("Политика цикла: не переопределяй model — модель и effort заданы в определении агента.")
        if is_sub and ctx >= SOFT:
            deny(f"Контекст {ctx // 1000}k ≥ {SOFT // 1000}k: новых субагентов не запускай, сворачивайся.")
        if caller == "coder" and running_children() >= MAX_CHILDREN:
            deny(f"Уже работают {MAX_CHILDREN} субагента (лимит). Дождись завершения одного из них.")
        if caller == MAIN:
            busy = live_agents({"coder", "reviewer"})
            if busy:
                deny(f"`{busy[0]['type']}` ещё работает. Закончи ход и дождись уведомления о его завершении, "
                     "затем `scripts/loop/loopctl next`.")


def on_post_tool(d):
    aid = d.get("agent_id")
    if not aid:
        return
    path = transcript_for(d)
    ctx = ctx_tokens(path)
    if ctx >= HARD:
        rel = hard_cut(d, path, ctx)
        out({"continue": False, "stopReason": cut_reason(rel)})
    if ctx >= SOFT:
        wfile = marker_dir("ctx") / aid
        last = int(wfile.read_text() or 0) if wfile.exists() else 0
        if last and ctx < last + WARN_STEP:
            return
        wfile.write_text(str(ctx))
        context("PostToolUse",
                f"⚠ КОНТЕКСТ {ctx // 1000}k ≥ {SOFT // 1000}k. Сворачивайся сейчас: "
                "(1) не начинай новых подзадач и не запускай субагентов; "
                "(2) доведи текущий атомарный шаг до зелёного состояния или откати его; "
                "(3) закоммить готовое (если твоя роль коммитит); "
                "(4) допиши свой отчёт: что сделано, что не получилось и почему, что начато и не "
                "закончено (файлы, точка остановки, следующий шаг); находки и вопросы — в .loop/JOURNAL.md; "
                f"(5) верни родителю краткий итог. На {HARD // 1000}k агент будет остановлен "
                "принудительно, и родитель получит только автоматический дамп.")


def on_subagent_start(d):
    aid = d.get("agent_id")
    if aid:
        tp = Path(d.get("transcript_path") or "")
        tr = tp.parent / d.get("session_id", "") / "subagents" / f"agent-{aid}.jsonl"
        (marker_dir("agents") / f"{aid}.json").write_text(
            json.dumps({"type": d.get("agent_type"), "t": time.time(), "transcript": str(tr)}))


def report_ok(role, st):
    cycle = int(st.get("cycle") or 0)
    p = LOOPDIR / "cycles" / f"{cycle:02d}" / f"{'coder' if role == 'coder' else 'review'}.md"
    if not p.exists():
        return False, str(p.relative_to(ROOT))
    txt = p.read_text()
    need = [r"^STATUS:\s*\S+"] if role == "coder" else [r"^SCORE:\s*\d+(\.\d+)?\s*/\s*10", r"^VERDICT:\s*(continue|complete)"]
    return all(re.search(rx, txt, re.M) for rx in need), str(p.relative_to(ROOT))


def on_subagent_stop(d):
    aid = d.get("agent_id")
    if aid:
        (RUN / "agents" / f"{aid}.json").unlink(missing_ok=True)
    role = d.get("agent_type")
    if not LOOP or role not in ("coder", "reviewer") or d.get("stop_hook_active"):
        return
    if aid and (RUN / "cut" / aid).exists():
        return
    st = read_state()
    if st.get("phase") != role:
        return
    ok, rel = report_ok(role, st)
    if not ok:
        fmt = "строкой `STATUS: done|partial|blocked`" if role == "coder" else \
            "строками `SCORE: <0-10>/10` и `VERDICT: continue|complete`"
        out({"decision": "block",
             "reason": f"Отчёт {rel} отсутствует или не начинается {fmt}. Допиши его по формату и заверши."})


def on_pre_compact(d):
    if not LOOP or d.get("trigger") == "manual":
        return
    main_ctx = ctx_tokens(Path(d.get("transcript_path") or ""))
    sid = d.get("session_id", "")
    sub_dir = Path(d.get("transcript_path") or "").parent / sid / "subagents"
    busy_sub = False
    if sub_dir.exists():
        for f in sub_dir.glob("**/agent-*.jsonl"):
            if time.time() - f.stat().st_mtime < 900 and ctx_tokens(f) >= MASTER_COMPACT_AT:
                busy_sub = True
                break
    if main_ctx >= MASTER_COMPACT_AT and not busy_sub:
        return  # сжимается мастер — разрешаем
    out({"decision": "block",
         "reason": "Сжатие рабочих агентов отключено: их контекст ограничивают пороги 300k/420k."})


def loop_status_line(st):
    return (f"cycle={st.get('cycle')}/{st.get('max_cycles')} phase={st.get('phase')} "
            f"status={st.get('status')} last_scores={[h.get('score') for h in st.get('history', [])][-5:]}")


def on_stop(d):
    if not LOOP or d.get("agent_id"):
        return
    st = read_state()
    if st.get("status") in ("done", "halted") or not st:
        return
    if d.get("background_tasks") or live_agents({"coder", "reviewer"}):
        return  # мастера разбудит уведомление о завершении фонового агента
    out({"decision": "block",
         "reason": "Цикл не завершён (" + loop_status_line(st) + "). Выполни `scripts/loop/loopctl next` "
                   "и следуй инструкции. Остановиться можно только после `LOOP FINISHED` "
                   "или `scripts/loop/loopctl halt \"<причина>\"`."})


def on_session_start(d):
    if not LOOP or d.get("agent_id"):
        return
    st = read_state()
    context("SessionStart",
            f"Ты — overseer автономного цикла (источник старта: {d.get('source')}). Состояние: "
            f"{loop_status_line(st)}. Вся память цикла на диске (.loop/state.json, .loop/cycles/). "
            "Продолжай с `scripts/loop/loopctl next`.")


def main():
    try:
        d = json.load(sys.stdin)
    except Exception:
        return
    ev = d.get("hook_event_name")
    handler = {
        "PreToolUse": on_pre_tool,
        "PostToolUse": on_post_tool,
        "SubagentStart": on_subagent_start,
        "SubagentStop": on_subagent_stop,
        "PreCompact": on_pre_compact,
        "Stop": on_stop,
        "SessionStart": on_session_start,
    }.get(ev)
    if handler:
        handler(d)


if __name__ == "__main__":
    main()
