# context-budget changelog

## 0.3.0

- Add Codex support for soft warnings and the hard stop using token counts from the session transcript.

## 0.2.0

- Replace compaction counting with soft warnings and a hard stop on Claude. Replace the
  `window` and `max_compactions` settings with `soft`, `hard`, `warn_step` and `cut`.

## 0.1.0

- Configure the compaction window and limit session compactions on Claude and Codex.
