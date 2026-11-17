---
name: zaag
description: Start, list, read and prompt the other coding agents running in zellij (one zellij session per project, one pane per agent). Use when the user asks to start or create an agent, pane, tab or project session, to send a prompt to another agent, to check what the other agents are doing, or to wait for one. Requires zellij (ZELLIJ is set inside a session) and the zaag command.
---

# zaag: the other agents, in zellij

Each project is a zellij session; each agent (Claude Code, Codex, Hermes) runs in its own pane, usually
its own tab. `zaag` is how you see and drive them. Every command prints `{"result": ...}` or
`{"error": {"code", "message"}}`; only `read` prints plain text.

An agent's id is `<session>:<pane>`, for example `webapp:4`; `<session>:<name>` also works when
one pane has that name. Take ids from `zaag list` or from what
`zaag new` answers: never guess them. You are the pane `$ZELLIJ_SESSION_NAME:$ZELLIJ_PANE_ID`.

```bash
zaag list                       # every agent: id, name, status, cwd, branch
zaag list --session webapp        # one project
zaag list --all                 # plain terminals too
zaag list --machines            # this machine and the SSH ones (grouped by machine)
zaag get webapp:4                # one agent
```

Status: `idle`, `working`, `blocked` (a dialog is waiting for an answer), `done` (finished, not
looked at yet). A `blocked` agent may carry `message`: what it is asking (zellij 0.46
and later).

## Start an agent

```bash
zaag new --name Reviewer --prompt "Review the diff on this branch and list the bugs."
zaag new --session api --cwd ~/code/api --name Builder           # another project; created if missing
zaag new --split right --name Tests --agent codex                 # a pane beside yours instead of a tab (also: hermes)
zaag new --agent shell --name logs                                # a plain terminal
zaag new --name Builder -- --permission-mode auto                 # after --: options for the agent itself
```

Without `--session` it starts in your own session; without `--cwd`, in your current folder. It
opens in a new tab and does not take the user's focus unless you add `--focus`. Start it where
the user asked; do not create a new project session unless they asked for one.

## Talk to one

```bash
zaag prompt webapp:4 "Run the tests and tell me which fail." --wait --timeout 900
zaag read webapp:4 --lines 60         # its screen; --full for the scrollback
zaag wait webapp:4 --until done,blocked --timeout 900
zaag keys webapp:4 esc                # interrupt; also: enter, tab, up, down, ctrl-c, or characters
zaag keys webapp:4 1                  # pick option 1 of the dialog a blocked agent shows
zaag rename webapp:4 "Test runner"
zaag close webapp:4
```

- `prompt --wait` returns when the agent has finished its turn or is blocked on a dialog. Read
  its answer with `zaag read`, or ask it in the prompt to write the answer to a file when it is long.
- `prompt` refuses a `blocked` agent (`"code": "blocked"`): read its screen, then answer the
  dialog with `zaag keys` only if the user allowed that kind of decision; otherwise tell the user.
- Prompt an agent that is `idle` or `done`. One that is `working` receives your text in the
  middle of its turn.

## Another machine

`-m <ssh-host>` before the command runs it there: `zaag -m devbox list`,
`zaag -m devbox prompt webapp:2 "..."`. Ids are per machine.

## Rules

- Close or interrupt only the agents you started, unless the user asked.
- Do not rename or close the user's sessions.
- Do not run `zellij kill-session`, `zellij delete-session` or `zellij kill-all-sessions`.
