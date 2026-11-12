#!/usr/bin/env bash
# Запуск (или продолжение) автономного цикла aae в интерактивной TUI-сессии.
# Интерактивно, а не `claude -p`: headless расходует лимит подписки заметно быстрее
# (anthropics/claude-code#97074). Состояние цикла — на диске, повторный запуск продолжает с того же места.
set -euo pipefail
cd "$(dirname "$0")/../.."

branch=$(git branch --show-current)
[ "$branch" = "main" ] || { echo "нужна ветка main, сейчас: $branch" >&2; exit 1; }
command -v claude >/dev/null || { echo "claude не найден в PATH" >&2; exit 1; }
mise install -q >/dev/null

export AAE_LOOP=1
# Мастер сжимается при ~267k (окно 300k минус буфер 33k); рабочим агентам сжатие
# блокирует хук PreCompact, их контекст режут пороги 300k/420k (.claude/hooks/loop_guard.py).
exec claude --agent overseer --model claude-sonnet-5-5 --effort medium \
  --permission-mode bypassPermissions \
  --settings '{"autoCompactWindow": 300000}' \
  "Старт или продолжение автономного цикла. Выполни scripts/loop/loopctl next и следуй инструкции."
