# CLAUDE.md

## What this is

bertbox is a multicall binary (busybox-style) of personal command-line tools.
A tool runs as `bertbox <tool>`, `bertbox-<tool>`, or `<tool>`. 
Code that runs bertbox executes `dispatch::program()` with `BERTBOX_AS_BERTBOX=1` in the environment and the tool name as the first argument.

## Commands

Use or create `just` (`justfile`) recipes for _any_ repeatable development task.
