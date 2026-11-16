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
