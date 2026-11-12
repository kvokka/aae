# aae

## The br tracker and its license

A project with a `<project>/.beads` directory uses [br](https://github.com/Dicklesworthstone/beads_rust) as its
tracker. br is a separate binary you install yourself (this repository pins 0.7.6 in `mise.toml`); aae runs it as a
process and never links, vendors or ships it.

**br is not plain MIT.** Its license is MIT with a rider that grants no rights to OpenAI, Anthropic, their affiliates
or anyone acting on their behalf. Check whether that rider applies to you before you install br. Whether it applies to
Claude or Codex agents running `br` inside an aae session has not been assessed.
