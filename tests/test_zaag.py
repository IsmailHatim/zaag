"""Run with: python3 -m unittest discover -s tests"""
import importlib.machinery
import importlib.util
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

STATE = tempfile.mkdtemp(prefix="zaag-test-")
os.environ["ZAAG_STATE"] = STATE
loader = importlib.machinery.SourceFileLoader("zaag", str(Path(__file__).resolve().parent.parent / "bin" / "zaag"))
spec = importlib.util.spec_from_loader("zaag", loader)
zaag = importlib.util.module_from_spec(spec)
loader.exec_module(zaag)

PERMISSION = """
 Bash command

   rm -rf build

 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and don't ask again for rm commands
   3. No, and tell Claude what to do differently (esc)
"""

TRUST = """
 Quick safety check: Is this a project you created or one you trust?

 ❯ No, exit
   Yes, I trust this folder

 Enter to confirm · Esc to cancel
"""

RULE = "─" * 80
INTERRUPTED = f"  168\n  ⎿  Interrupted · What should Claude do instead?\n\n{RULE}\n❯ \n{RULE}\n  ⏵⏵ auto mode on"
RESUMED = f"  ⎿  Interrupted · What should Claude do instead?\n\n❯ go on\n\n⏺ Sure.\n\n{RULE}\n❯ \n{RULE}"


class Screen(unittest.TestCase):
    def test_numbered_dialog(self):
        self.assertEqual([o["id"] for o in zaag.dialog_options(PERMISSION)], ["1", "2", "3"])
        self.assertTrue(zaag.has_dialog(PERMISSION))

    def test_one_numbered_line_is_not_a_dialog(self):
        self.assertEqual(zaag.dialog_options("Steps:\n 1. only one\nthen text"), [])

    def test_unnumbered_dialog(self):
        self.assertEqual(zaag.dialog_options(TRUST), [])
        self.assertTrue(zaag.has_dialog(TRUST))

    def test_dialog_wrapped_by_a_narrow_pane(self):
        self.assertTrue(zaag.has_dialog("› 1. Trust and\n     continue\n  2. Back to Agent\n     Command\n\n  enter continue\n  · esc back"))

    def test_plain_screen_has_no_dialog(self):
        self.assertFalse(zaag.has_dialog(INTERRUPTED))

    def test_interrupted_only_while_it_is_the_last_thing_said(self):
        self.assertTrue(zaag.was_interrupted(INTERRUPTED))
        self.assertFalse(zaag.was_interrupted(RESUMED))
        self.assertFalse(zaag.was_interrupted("❯ \n" + RULE))

    def test_titles(self):
        self.assertEqual(zaag.clean_title("✳ Fix the login bug"), "Fix the login bug")
        self.assertEqual(zaag.clean_title("⠐ Notes"), "Notes")
        self.assertEqual(zaag.clean_title("~/code"), "~/code")

    def test_ids(self):
        self.assertEqual(zaag.split_id("webapp:4"), ("webapp", 4))
        self.assertEqual(zaag.split_id("a:b:12"), ("a:b", 12))
        for bad in ("webapp", "webapp:x", ":3"):
            with self.assertRaises(zaag.ZaagError):
                zaag.split_id(bad)


CODEX_START = """  Hooks need review
  1 hook is new or changed.

› 1. Review hooks
  2. Trust all and continue
  3. Continue without trusting (hooks won't run)

  enter confirm · esc skip
"""


class Settle(unittest.TestCase):
    def settle(self, st, text):
        with mock.patch.object(zaag, "screen", return_value=text), mock.patch.object(zaag, "save"):
            return zaag.settle("proj", 1, dict(st, updated=0))["status"]

    def test_a_start_up_menu_blocks_an_agent_that_has_not_spoken_yet(self):
        self.assertEqual(self.settle({"agent": "codex", "status": "idle"}, CODEX_START), "blocked")

    def test_and_it_is_idle_again_once_the_menu_is_answered(self):
        self.assertEqual(self.settle({"agent": "codex", "status": "blocked"}, "› \n"), "idle")

    def test_an_idle_agent_that_has_spoken_is_left_alone(self):
        self.assertEqual(self.settle({"agent": "codex", "status": "idle", "session_id": "s"}, CODEX_START), "idle")

    def test_a_dialog_answered_mid_turn_means_working(self):
        self.assertEqual(self.settle({"agent": "claude", "status": "blocked", "session_id": "s"}, "⏺ running\n"), "working")


class Reported(unittest.TestCase):
    """zellij 0.46 keeps what a program says about itself (OSC 7501) in `list-panes`."""

    def pane(self, state, seen=False, **more):
        record = dict({"state": state, "app": "claude-code", "updated_ms": 5000}, **more)
        flagged = "Idle" if seen else {"Error": "Failed", "Blocked": "NeedsYou"}.get(state, state)
        return {"id": 1, "title": "t", "program_status": [record], "attention": {"status": flagged}}

    def rows(self, pane, st=None):
        with mock.patch.object(zaag, "panes", return_value=[pane]), mock.patch.object(zaag, "load", return_value=st), \
                mock.patch.object(zaag, "screen", return_value=""), mock.patch.object(zaag, "branch_of", return_value=None):
            return zaag.session_rows("proj", False, {})

    def test_an_older_zellij_says_nothing(self):
        self.assertIsNone(zaag.reported({"id": 1, "title": "t"}))
        self.assertEqual(self.rows({"id": 1, "title": "t"}), [])

    def test_an_agent_without_hooks_is_listed_from_what_it_told_zellij(self):
        row = self.rows(self.pane("Blocked", kind="Question", msg="Tea or\ncoffee?"))[0]
        self.assertEqual((row["agent"], row["status"], row["message"]), ("claude", "blocked", "Tea or coffee?"))

    def test_done_is_idle_once_it_was_seen_and_a_failure_is_done(self):
        self.assertEqual(zaag.reported(self.pane("Done"))["status"], "done")
        self.assertEqual(zaag.reported(self.pane("Done", seen=True))["status"], "idle")
        self.assertEqual(zaag.reported(self.pane("Error"))["status"], "done")

    def test_the_latest_report_counts(self):
        pane = self.pane("Working")
        pane["program_status"].append({"state": "Done", "updated_ms": 9000})
        pane["attention"] = {"status": "Done"}
        self.assertEqual(zaag.reported(pane)["status"], "done")

    def test_hooks_come_first_and_the_message_only_goes_with_the_same_status(self):
        hooked = {"agent": "claude", "status": "blocked", "session_id": "s", "updated": 0}
        with mock.patch.object(zaag, "save"):
            row = self.rows(self.pane("Working", msg="Compiling"), st=hooked)[0]
        self.assertEqual(row["status"], "working")  # no dialog on the screen: settled by the hooks' rules
        self.assertEqual(row.get("message"), "Compiling")
        with mock.patch.object(zaag, "save"), mock.patch.object(zaag, "has_dialog", return_value=True):
            row = self.rows(self.pane("Working", msg="Compiling"), st=dict(hooked))[0]
        self.assertEqual((row["status"], row.get("message")), ("blocked", None))

    def test_a_name_given_by_zaag_new_is_kept_until_a_hook_speaks(self):
        row = self.rows(self.pane("Blocked", msg="Deploy?"), st={"agent": "sh", "name": "deploy", "status": "idle"})[0]
        self.assertEqual((row["name"], row["agent"], row["status"], row["message"]), ("deploy", "claude", "blocked", "Deploy?"))

    def test_it_is_never_written_down(self):
        zaag.save("proj", 77, zaag.reported(self.pane("Done")))
        self.assertIsNone(zaag.load("proj", 77))


class Zellij(unittest.TestCase):
    def test_a_pane_list_that_is_not_json_is_asked_again(self):
        answers = iter(["There is no active session!\n", "", '[{"id": 1, "is_plugin": false}, {"id": 0, "is_plugin": true}]'])
        with mock.patch.object(zaag, "zj", side_effect=lambda *a, **k: next(answers)), mock.patch.object(zaag.time, "sleep"):
            self.assertEqual([p["id"] for p in zaag.panes("proj")], [1])

    def test_and_is_a_plain_error_when_it_never_comes(self):
        with mock.patch.object(zaag, "zj", return_value="nope"), mock.patch.object(zaag.time, "sleep"):
            with self.assertRaises(zaag.ZaagError) as e:
                zaag.panes("proj")
        self.assertEqual(e.exception.code, "zellij-error")

    def test_a_remote_project_opens_in_a_tab_through_the_other_machines_zaag(self):
        calls = []
        args = zaag.parser().parse_args(["-m", "devbox", "attach", "api", "--pane", "4", "--in", "webapp"])
        with mock.patch.object(zaag, "all_panes", return_value=[{"tab_name": "Tab #1"}]), mock.patch.object(zaag, "zj", side_effect=lambda *a, **k: calls.append(a) or ""), \
                mock.patch.object(zaag, "config", return_value={"remote_zag": "~/.local/bin/zaag"}):
            out = zaag.attach_remote(args)
        self.assertEqual(out["tab"], "devbox:api")
        self.assertEqual(calls[0], ("webapp", "new-tab", "--name", "devbox:api", "--close-on-exit", "--", "ssh", "-t", "devbox", '"$HOME"/.local/bin/zaag attach api --pane 4'))
        with mock.patch.object(zaag, "all_panes", return_value=[{"tab_name": "devbox:api"}]), mock.patch.object(zaag, "zj", side_effect=lambda *a, **k: calls.append(a) or ""), \
                mock.patch.object(zaag, "config", return_value={"remote_zag": "~/.local/bin/zaag"}):
            zaag.attach_remote(args)
        self.assertEqual(calls[1], ("webapp", "go-to-tab-name", "devbox:api"))


class Background(unittest.TestCase):
    """Claude Code sessions that run in its background service and are only shown in a pane."""

    def setUp(self):
        for f in Path(STATE).glob("agents/*.json"):
            f.unlink()
        self.pane = {"id": 5, "title": "✳ Fix login", "pane_cwd": "/somewhere/else"}
        self.record = {"Fix login": {"kind": "bg", "name": "Fix login", "sessionId": "real", "cwd": "/w", "status": "busy", "home": STATE}}

    def test_a_pane_titled_like_a_background_session_is_adopted(self):
        st = zaag.adopt("proj", self.pane, None, self.record)
        self.assertEqual((st["agent"], st["session_id"], st["status"], st["cwd"]), ("claude", "real", "working", "/w"))

    def test_the_displaying_claude_is_remembered_and_then_ignored(self):
        zaag.save("proj", 5, {"agent": "claude", "status": "idle", "session_id": "viewer", "done_seq": 0})
        st = zaag.adopt("proj", self.pane, zaag.load("proj", 5), self.record)
        self.assertEqual((st["session_id"], st["viewer_session_id"]), ("real", "viewer"))

    def test_other_titles_and_other_agents_are_left_alone(self):
        self.assertIsNone(zaag.adopt("proj", dict(self.pane, title="zsh"), None, self.record))
        codex = {"agent": "codex", "status": "idle", "session_id": "c"}
        self.assertEqual(zaag.adopt("proj", self.pane, codex, self.record), codex)

    def test_a_renamed_pane_is_recognised_by_what_is_on_its_screen(self):
        said = [zaag.squeeze("The login form now has a **remember me** checkbox, and sessions last thirty days when it is ticked. " * 3)]
        wrapped = "  The login form now has a remember me\n  checkbox, and sessions last thirty days when\n  it is ticked. " * 3
        self.assertTrue(zaag.shows(zaag.squeeze(wrapped), said))
        self.assertFalse(zaag.shows(zaag.squeeze("something else entirely on this screen, nothing in common " * 5), said))

    def test_its_hooks_find_the_pane_by_the_session_name(self):
        with mock.patch.object(zaag, "claude_session_name", return_value="Fix login"), \
             mock.patch.object(zaag, "live_sessions", return_value=["proj"]), \
             mock.patch.object(zaag, "panes", return_value=[self.pane, {"id": 6, "title": "zsh"}]):
            self.assertEqual(zaag.claude_pane("real", "/w"), ("proj", "5"))
            self.assertEqual(zaag.claude_pane(None, "/w"), (None, None))

    def test_a_hook_from_outside_zellij_updates_that_pane(self):
        stdin = io.StringIO(json.dumps({"hook_event_name": "Stop", "session_id": "real", "cwd": "/w"}))
        stdin.isatty = lambda: False
        env = {k: v for k, v in os.environ.items() if not k.startswith("ZELLIJ")}
        with mock.patch.dict(os.environ, env, clear=True), mock.patch.object(sys, "stdin", stdin), \
             mock.patch.object(zaag, "claude_pane", return_value=("proj", "5")):
            zaag.main(["hook", "--agent", "claude"])
        st = zaag.load("proj", 5)
        self.assertEqual((st["status"], st["session_id"], st["done_seq"]), ("done", "real", 1))


class Hook(unittest.TestCase):
    def setUp(self):
        for f in Path(STATE).glob("agents/*.json"):
            f.unlink()
        self.env = mock.patch.dict(os.environ, {"ZELLIJ_SESSION_NAME": "proj", "ZELLIJ_PANE_ID": "7"})
        self.env.start()
        self.addCleanup(self.env.stop)
        self.addCleanup(mock.patch.stopall)
        mock.patch.object(zaag, "claude_ancestor", return_value=None).start()

    def fire(self, event, **data):
        data.setdefault("session_id", "s1")
        stdin = io.StringIO(json.dumps({"hook_event_name": event, **data}))
        stdin.isatty = lambda: False
        with mock.patch.object(sys, "stdin", stdin):
            zaag.main(["hook", "--agent", "claude"])
        return zaag.load("proj", 7)

    def test_a_turn(self):
        self.assertEqual(self.fire("SessionStart", cwd="/w", transcript_path="/t.jsonl")["status"], "idle")
        self.assertEqual(self.fire("UserPromptSubmit")["status"], "working")
        self.assertEqual(self.fire("Notification", notification_type="permission_prompt")["status"], "blocked")
        self.assertEqual(self.fire("PostToolUse", tool_name="Bash")["status"], "working")
        st = self.fire("Stop")
        self.assertEqual((st["status"], st["done_seq"], st["cwd"], st["transcript_path"]), ("done", 1, "/w", "/t.jsonl"))
        self.assertEqual(self.fire("Stop")["done_seq"], 2)

    def test_questions_block(self):
        self.fire("SessionStart")
        self.assertEqual(self.fire("PreToolUse", tool_name="AskUserQuestion")["status"], "blocked")

    def test_idle_notification_changes_nothing(self):
        self.fire("SessionStart")
        self.fire("Stop")
        self.assertEqual(self.fire("Notification", notification_type="idle_prompt")["status"], "done")

    def test_session_end_forgets(self):
        self.fire("SessionStart")
        self.assertIsNone(self.fire("SessionEnd"))

    def test_another_sessions_end_does_not_forget(self):
        self.fire("SessionStart")
        self.assertIsNotNone(self.fire("SessionEnd", session_id="other"))

    def test_name_given_by_zag_new_survives(self):
        zaag.save("proj", 7, {"agent": "claude", "status": "working", "name": "Reviewer", "done_seq": 0})
        st = self.fire("SessionStart")
        self.assertEqual((st["name"], st["status"]), ("Reviewer", "working"))

    def test_a_nested_agent_is_ignored(self):
        self.fire("SessionStart")
        st = zaag.load("proj", 7)
        st["pid"] = os.getpid()  # the owner is alive
        zaag.save("proj", 7, st)
        zaag.claude_ancestor.return_value = 999999
        with mock.patch.object(zaag, "is_under", return_value=True):
            self.assertEqual(self.fire("Stop", session_id="nested")["status"], "idle")

    def test_outside_zellij_nothing_is_written(self):
        with mock.patch.dict(os.environ, {"ZELLIJ_SESSION_NAME": ""}):
            self.fire("SessionStart")
        self.assertIsNone(zaag.load("proj", 7))

    def test_hermes_events(self):
        def fire(event, **extra):
            stdin = io.StringIO(json.dumps({"hook_event_name": event, "session_id": "h1", "cwd": "/w", "extra": extra}))
            stdin.isatty = lambda: False
            with mock.patch.object(sys, "stdin", stdin):
                zaag.main(["hook", "--agent", "hermes"])
            return zaag.load("proj", 7)

        self.assertEqual(fire("on_session_start")["status"], "idle")
        self.assertEqual(fire("pre_llm_call")["status"], "working")
        self.assertEqual(fire("post_tool_call")["status"], "working")
        st = fire("on_session_end", turn_id="t1", completed=True)
        self.assertEqual((st["agent"], st["status"], st["done_seq"]), ("hermes", "done", 1))
        self.assertEqual(fire("on_session_end", reason="exit")["done_seq"], 1)  # quitting is not a turn
        fire("pre_llm_call")
        self.assertEqual(fire("on_session_end", turn_id="t2", interrupted=True)["status"], "idle")
        self.assertIsNone(fire("on_session_finalize"))

    def test_seen(self):
        self.fire("SessionStart")
        self.fire("Stop")
        with mock.patch("builtins.print"):
            zaag.main(["seen", "proj:7"])
        self.assertEqual(zaag.load("proj", 7)["status"], "idle")


if __name__ == "__main__":
    unittest.main()
