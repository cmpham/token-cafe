#!/usr/bin/env python3
"""Android UI E2E against the real bridge and controlled live Claude session files.

Requires an already booted emulator with the APK installed and Pillow.
Only the selected emulator is controlled. No model/provider calls are made.
"""
import argparse
import datetime
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET

from PIL import Image

APP = "cafe.token.android/.MainActivity"
TASK = ("VERIFY ANDROID BRIDGE FROM LOCAL SESSION FILES. READ THE ENTIRE TASK USING THE "
        "SCROLLBAR. CHECK RECONNECT AND ROTATION. FINISH. ANDROID QA.")
SID = "01234567-aaaa-bbbb-cccc-123456789abc"


class Suite:
    def __init__(self, args, directory):
        self.args = args
        self.root = Path(directory)
        self.claude = self.root / "claude"
        (self.claude / "sessions").mkdir(parents=True)
        project = self.claude / "projects" / "android-qa"
        project.mkdir(parents=True)
        self.transcript = project / f"{SID}.jsonl"
        now = datetime.datetime.now(datetime.timezone.utc).isoformat()
        self.transcript.write_text(json.dumps({"type": "user", "sessionId": SID,
            "timestamp": now, "message": {"content": TASK}}) + "\n")
        self.sleeper = subprocess.Popen(["sleep", "3600"])
        self.record = self.claude / "sessions" / f"{self.sleeper.pid}.json"
        self.started = time.time() * 1000
        self.bridge = None
        self.token = None
        self.results = []
        self.output = Path(args.output)
        self.output.mkdir(parents=True, exist_ok=True)
        assets = (Path(__file__).resolve().parents[1] / "crates/cafe-ui/src/assets.rs").read_text()
        font = assets.split("pub static FONT:", 1)[1].split("= [", 1)[1].split("];", 1)[0]
        rows = re.findall(r"\[([^\]]+)\]", font)
        self.font = {tuple(int(v, 16) for v in re.findall(r"0x[0-9a-f]+", row)): chr(i + 32)
                     for i, row in enumerate(rows[:64])}
        self.window_y = int(re.search(r"pub const WIN_Y: i32 = (\d+)", assets).group(1))
        self.state("busy")

    def adb(self, *args, binary=False):
        return subprocess.check_output([self.args.adb, "-s", self.args.serial, *args],
                                       text=not binary, timeout=30)

    def state(self, status, why=None, age=0):
        value = {"pid": self.sleeper.pid, "kind": "interactive", "sessionId": SID,
                 "startedAt": self.started, "name": "ANDROID-QA", "cwd": str(self.root),
                 "status": status, "statusUpdatedAt": (time.time() - age) * 1000}
        proc = Path(f"/proc/{self.sleeper.pid}/stat")
        if proc.exists():
            value["procStart"] = int(proc.read_text().rsplit(")", 1)[1].split()[19])
        if why:
            value["waitingFor"] = why
        temporary = self.record.with_suffix(".tmp")
        temporary.write_text(json.dumps(value))
        temporary.replace(self.record)

    def start_bridge(self):
        env = os.environ.copy()
        env["CLAUDE_CONFIG_DIR"] = str(self.claude)
        env["XDG_CACHE_HOME"] = str(self.root / "cache")
        self.bridge = subprocess.Popen([str(Path(self.args.bridge).resolve())], env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        for _ in range(6):
            line = self.bridge.stdout.readline()
            if "Pairing token:" in line:
                self.token = line.split(":", 1)[1].strip()
                return
            if not line and self.bridge.poll() is not None:
                raise RuntimeError("Desktop bridge did not start (port 38417 must be free)")
        raise RuntimeError("Bridge did not emit its pairing token")

    def stop_bridge(self):
        if self.bridge:
            self.bridge.terminate()
            self.bridge.wait(timeout=10)
            self.bridge.stdout.close()
            self.bridge.stderr.close()
            self.bridge = None

    def snapshot(self):
        request = urllib.request.Request("http://127.0.0.1:38417/v1/snapshot",
            headers={"Authorization": "Bearer " + self.token})
        with urllib.request.urlopen(request, timeout=3) as response:
            return json.load(response)

    def session(self):
        return next((s for s in self.snapshot()["sessions"] if s["id"] == "claude:" + SID), None)

    def wait(self, predicate, message, timeout=18):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                if predicate():
                    return
            except (OSError, subprocess.SubprocessError, ET.ParseError):
                pass
            time.sleep(.4)
        raise AssertionError(message)

    def nodes(self):
        self.adb("shell", "uiautomator", "dump", "/sdcard/token-cafe-e2e.xml")
        return list(ET.fromstring(self.adb("shell", "cat", "/sdcard/token-cafe-e2e.xml")).iter("node"))

    @staticmethod
    def bounds(node):
        return list(map(int, re.findall(r"\d+", node.get("bounds"))))

    def click(self, node):
        left, top, right, bottom = self.bounds(node)
        self.adb("shell", "input", "tap", str((left + right) // 2), str((top + bottom) // 2))

    def button(self, text):
        self.click(next(n for n in self.nodes() if n.get("text", "").upper() == text.upper()))

    def status(self, text):
        self.wait(lambda: any(text in n.get("text", "") for n in self.nodes()),
                  "App status did not contain " + text)

    def pair(self, token):
        self.button("CONNECT USB")
        self.click(next(n for n in self.nodes() if n.get("class") == "android.widget.EditText"))
        self.adb("shell", "input", "text", token)
        keyboard = self.adb("shell", "dumpsys", "input_method")
        if re.search(r"(?:mInputShown|isInputViewShown)=true", keyboard):
            self.adb("shell", "input", "keyevent", "4")
        self.click(next(n for n in self.nodes() if n.get("resource-id") == "android:id/button1"))

    def frame(self):
        view = next(n for n in self.nodes() if n.get("content-desc", "").startswith("Animated Token Café"))
        left, top, right, bottom = self.bounds(view)
        scale = min((right-left)/480, (bottom-top)/272)
        if scale >= 1:
            scale = int(scale)
        width, height = round(480*scale), round(272*scale)
        return left+(right-left-width)//2, top+(bottom-top-height)//2, scale

    def tap_scene(self, x, y):
        left, top, scale = self.frame()
        self.adb("shell", "input", "tap", str(round(left+x*scale)), str(round(top+y*scale)))

    def select_fixture(self):
        # Rotation policies can preserve selection. Establish a cleared state
        # through visible UI before testing the same patron tap in either layout.
        self.wait(lambda: "REQUEST RESOLVED" not in self.footer(), "Resolution notice did not finish")
        footer = self.footer()
        if any(prefix in footer for prefix in ("TASK ANDROID-QA", "LAST ANDROID-QA", "LAST REQUEST ANDROID-QA")):
            self.tap_scene(60, 100)
        self.tap_scene(60, 100)

    def image(self):
        return Image.open(io.BytesIO(self.adb("exec-out", "screencap", "-p", binary=True))).convert("RGB")

    def footer(self, save=None, frame=None):
        left, top, scale = frame or self.frame()
        image = self.image().crop((left, round(top+self.window_y*scale),
                                  round(left+480*scale), round(top+272*scale)))
        if save:
            image.save(self.output / save)
        # Read actual screen pixels against the shipped bitmap alphabet. General
        # OCR confuses this font's F/E, N/H and I/T; no scene state is inspected.
        native = image.resize((480, 272-self.window_y), Image.Resampling.NEAREST)
        decoded = []
        for y, size in [(5, 1), (6, 1), (13, 1), (22, 2), (19, 2), (38, 2)]:
            line = []
            for column in range((480-50)//(6*size)):
                rows = []
                for dy in range(7):
                    bits = 0
                    for dx in range(5):
                        pixel = native.getpixel((50+(column*6+dx)*size, y+dy*size))
                        bits = (bits << 1) | int(pixel[0] > 180 and pixel[1] > 130)
                    rows.append(bits)
                line.append(self.font.get(tuple(rows), "?"))
            decoded.append("".join(line))
        return re.sub(r"\s+", " ", " ".join(decoded))

    def expect_footer(self, text, save=None):
        frame = self.frame()
        try:
            self.wait(lambda: text in self.footer(frame=frame), "Rendered footer did not contain " + text)
        except AssertionError:
            observed = self.footer("failure-footer.png", frame)
            raise AssertionError(f"Expected rendered footer {text!r}; screen pixels decoded {observed!r}") from None
        if save:
            self.footer(save, frame)

    def passed(self, name):
        self.results.append({"check": name, "result": "pass"})
        print("PASS:", name, flush=True)

    def run(self):
        self.adb("shell", "am", "force-stop", "cafe.token.android")
        self.adb("logcat", "-c")
        self.adb("shell", "settings", "put", "system", "accelerometer_rotation", "0")
        self.adb("shell", "wm", "user-rotation", "lock", "0")
        self.wait(lambda: self.image().height > self.image().width, "Portrait rotation did not apply")
        self.adb("shell", "am", "start", "-W", "-n", APP)
        self.status("Offline demo")
        self.image().save(self.output / "demo-portrait.png")
        self.passed("Cold launch and offline demo in portrait")
        self.start_bridge()
        self.wait(lambda: self.session() and self.session()["task"] == TASK,
                  "Real collector did not discover the live fixture/transcript")
        self.passed("Live process discovery and transcript parsing through real Rust bridge")
        self.adb("reverse", "tcp:38417", "tcp:38417")
        self.pair(self.token)
        self.status("Live desktop")
        self.select_fixture()
        self.expect_footer("VERIFY ANDROID", "live-task-start.png")
        self.passed("Pairing, live transport, JNI rendering and patron selection")
        left, top, scale = self.frame()
        self.adb("shell", "input", "swipe", str(round(left+460*scale)), str(round(top+228*scale)),
                 str(round(left+460*scale)), str(round(top+270*scale)), "500")
        self.expect_footer("ANDROID QA", "live-task-scrolled.png")
        self.passed("Touch scrollbar reaches final task text")
        self.state("waiting", "question")
        self.wait(lambda: self.session()["state"] == "wait" and self.session()["why"] == "ask", "Wait state missing")
        self.expect_footer("ANSWER NEEDED", "waiting-question.png")
        self.passed("Session-file update becomes waiting-question alert on Android")
        self.state("waiting", "permission prompt")
        self.wait(lambda: self.session()["why"] == "perm", "Permission state missing")
        self.expect_footer("APPROVAL NEEDED", "waiting-permission.png")
        self.passed("Permission alert rendered without approval dispatch")
        self.state("idle", age=700)
        self.wait(lambda: self.session()["state"] == "idle", "Idle state missing")
        self.select_fixture()
        self.expect_footer("LAST", "idle-last-task.png")
        self.passed("Idle state labels the task as historical")
        self.adb("shell", "wm", "user-rotation", "lock", "1")
        self.wait(lambda: self.image().width > self.image().height, "Landscape rotation did not apply")
        self.status("Live desktop")
        self.select_fixture()
        self.expect_footer("LAST", "landscape-task.png")
        self.passed("Landscape recreation preserves pairing and correctly maps touch")
        self.adb("shell", "input", "keyevent", "3")
        self.state("busy")
        self.adb("shell", "am", "start", "-W", "-n", APP)
        self.status("Live desktop")
        self.expect_footer("VERIFY ANDROID")
        self.passed("Background/foreground resumes fresh live state")
        self.adb("reverse", "--remove", "tcp:38417")
        self.status("USB disconnected")
        assert "VERIFY ANDROID" not in self.footer("disconnected.png")
        self.passed("Transport loss clears stale task and shows recovery instructions")
        self.adb("reverse", "tcp:38417", "tcp:38417")
        self.status("Live desktop")
        self.select_fixture()
        self.expect_footer("VERIFY ANDROID")
        self.passed("Transport restoration automatically reconnects")
        old_token = self.token
        self.stop_bridge()
        self.status("USB disconnected")
        self.start_bridge()
        assert self.token != old_token
        self.status("Pairing rejected")
        self.passed("Bridge restart rotates token and rejects the old credential")
        self.pair(self.token)
        self.status("Live desktop")
        self.select_fixture()
        self.expect_footer("VERIFY ANDROID")
        self.passed("Replacement token pairs with restarted real bridge")
        self.sleeper.terminate()
        self.sleeper.wait(timeout=5)
        self.wait(lambda: self.session() is None, "Dead process remained collected")
        time.sleep(2)
        assert "VERIFY ANDROID" not in self.footer("session-ended.png")
        self.passed("Process exit removes session and selected task")
        self.button("DEMO")
        self.status("Offline demo")
        self.image().save(self.output / "demo-landscape.png")
        self.passed("Demo button leaves live mode")
        crashes = self.adb("logcat", "-d", "-s", "AndroidRuntime:E", "libc:F")
        assert "FATAL EXCEPTION" not in crashes and "Fatal signal" not in crashes, crashes
        self.passed("No Java or native fatal crash logged")

    def close(self):
        self.stop_bridge()
        if self.sleeper.poll() is None:
            self.sleeper.terminate()
            self.sleeper.wait(timeout=5)
        try:
            self.adb("reverse", "--remove", "tcp:38417")
        except subprocess.SubprocessError:
            pass
        (self.output / "results.json").write_text(json.dumps(self.results, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adb", default="adb")
    parser.add_argument("--serial", required=True, help="Explicit simulator serial; physical devices are refused")
    parser.add_argument("--bridge", default="target/debug/examples/android_bridge")
    parser.add_argument("--output", default="target/android-e2e")
    args = parser.parse_args()
    if not args.serial.startswith("emulator-"):
        parser.error("This suite is restricted to Android emulators")
    with tempfile.TemporaryDirectory(prefix="token-cafe-android-e2e-") as directory:
        suite = Suite(args, directory)
        try:
            suite.run()
        except Exception as error:
            suite.results.append({"check": "suite", "result": "failed", "error": str(error)})
            raise
        finally:
            suite.close()
    print(f"{len(suite.results)} E2E checks passed; evidence: {suite.output}")


if __name__ == "__main__":
    main()
