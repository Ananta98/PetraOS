#!/usr/bin/env python3
"""
PetraOS CLI Test Harness
========================
Automated CLI & shell testing framework for PetraOS running in QEMU (via `make run`).
Provides:
  - Automated execution of single commands or JSON test suites.
  - Automated QEMU monitor integration for framebuffer screenshot capture (`screendump`).
  - Image conversion to PNG (via Pillow) and automatic visual failure diagnostics.
  - Serial log monitoring and assertion verification (contains, regex, exit_code).
  - Full Markdown & JSON test reporting.
"""

import argparse
import json
import os
import pty
import re
import select
import shutil
import socket
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

try:
    from PIL import Image
    HAS_PIL = True
except ImportError:
    HAS_PIL = False


# Default paths and settings
DEFAULT_REPO_ROOT = Path(__file__).resolve().parents[4] if "skills" in str(Path(__file__).resolve()) else Path(__file__).resolve().parent.parent
DEFAULT_TEST_SUITE = Path(__file__).resolve().parent / "default_tests.json"
DEFAULT_REPORTS_DIR = DEFAULT_REPO_ROOT / "test_reports"
DEFAULT_SCREENSHOTS_DIR = DEFAULT_REPORTS_DIR / "screenshots"
DEFAULT_MONITOR_SOCK = Path("/tmp/petra_qemu_monitor.sock")

# Common shell prompt patterns
PROMPT_PATTERNS = [
    re.compile(rb"bash-[0-9\.]+[#$]\s*"),
    re.compile(rb"bash[#$]\s*"),
    re.compile(rb"[a-zA-Z0-9_\-\.]+@[a-zA-Z0-9_\-\.]+:[^#$]*[#$]\s*"),
    re.compile(rb"\[[a-zA-Z0-9_\-\.]+@[a-zA-Z0-9_\-\.]+\s+[^#$]*\][#$]\s*"),
    re.compile(rb"[#$]\s+"),
]

# Regex for stripping ANSI escape sequences
ANSI_ESCAPE_RE = re.compile(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")

def strip_ansi(text: str) -> str:
    return ANSI_ESCAPE_RE.sub("", text)

# Fatal kernel panic or crash indicators in serial log
FATAL_PATTERNS = [
    re.compile(rb"KERNEL PANIC", re.IGNORECASE),
    re.compile(rb"DOUBLE FAULT", re.IGNORECASE),
    re.compile(rb"PAGE FAULT EXCEPTION", re.IGNORECASE),
    re.compile(rb"GENERAL PROTECTION FAULT", re.IGNORECASE),
    re.compile(rb"UNHANDLED EXCEPTION", re.IGNORECASE),
    re.compile(rb"kernel panic:.*", re.IGNORECASE),
]


class QemuMonitor:
    """Interface to QEMU monitor via UNIX domain socket."""

    def __init__(self, sock_path: Path):
        self.sock_path = sock_path
        self.sock: Optional[socket.socket] = None

    def connect(self, timeout: float = 10.0) -> bool:
        start = time.time()
        while time.time() - start < timeout:
            if self.sock_path.exists():
                try:
                    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    s.settimeout(3.0)
                    s.connect(str(self.sock_path))
                    self.sock = s
                    # Consume initial banner
                    try:
                        self.sock.recv(2048)
                    except socket.timeout:
                        pass
                    return True
                except (socket.error, ConnectionRefusedError):
                    time.sleep(0.2)
            else:
                time.sleep(0.1)
        return False

    def send_command(self, cmd: str) -> str:
        if not self.sock:
            return ""
        try:
            self.sock.sendall((cmd.strip() + "\n").encode("utf-8"))
            time.sleep(0.3)
            self.sock.settimeout(2.0)
            data = b""
            while True:
                try:
                    chunk = self.sock.recv(4096)
                    if not chunk:
                        break
                    data += chunk
                    if b"(qemu)" in data:
                        break
                except socket.timeout:
                    break
            return data.decode("utf-8", errors="replace")
        except socket.error as e:
            print(f"[WARN] Failed to send QEMU monitor command '{cmd}': {e}", file=sys.stderr)
            return ""

    def screendump(self, output_png_path: Path) -> Optional[Path]:
        """Capture QEMU framebuffer screenshot and convert to PNG."""
        if not self.sock:
            return None
        output_png_path.parent.mkdir(parents=True, exist_ok=True)
        raw_ppm_path = output_png_path.with_suffix(".ppm")
        if raw_ppm_path.exists():
            raw_ppm_path.unlink()

        self.send_command(f"screendump {raw_ppm_path}")
        time.sleep(0.5)

        if not raw_ppm_path.exists():
            # Retry once
            self.send_command(f"screendump {raw_ppm_path}")
            time.sleep(0.5)

        if raw_ppm_path.exists() and raw_ppm_path.stat().st_size > 0:
            try:
                if HAS_PIL:
                    img = Image.open(raw_ppm_path)
                    img.save(output_png_path, "PNG")
                    raw_ppm_path.unlink()
                    return output_png_path
                else:
                    # If Pillow is not available, retain PPM
                    return raw_ppm_path
            except Exception as e:
                print(f"[WARN] Error converting PPM to PNG: {e}", file=sys.stderr)
                return raw_ppm_path
        return None

    def close(self):
        if self.sock:
            try:
                self.sock.close()
            except Exception:
                pass
            self.sock = None
        if self.sock_path.exists():
            try:
                self.sock_path.unlink()
            except Exception:
                pass


class CliTestSession:
    """Manages execution of tests in QEMU (PetraOS) or Host."""

    def __init__(
        self,
        target: str = "qemu",
        repo_root: Path = DEFAULT_REPO_ROOT,
        headless: bool = True,
        monitor_sock: Path = DEFAULT_MONITOR_SOCK,
        screenshots_dir: Path = DEFAULT_SCREENSHOTS_DIR,
        boot_timeout: float = 40.0,
        cmd_timeout: float = 15.0,
        memory: str = "4G",
    ):
        self.target = target
        self.repo_root = repo_root
        self.headless = headless
        self.monitor_sock = monitor_sock
        self.screenshots_dir = screenshots_dir
        self.boot_timeout = boot_timeout
        self.cmd_timeout = cmd_timeout
        self.memory = memory

        self.proc: Optional[subprocess.Popen] = None
        self.master_fd: Optional[int] = None
        self.slave_fd: Optional[int] = None
        self.monitor: Optional[QemuMonitor] = None
        self.full_log: bytearray = bytearray()
        self.is_ready = False

    def start(self) -> bool:
        if self.target == "qemu":
            return self._start_qemu()
        elif self.target == "host":
            return self._start_host()
        else:
            raise ValueError(f"Unknown target: {self.target}")

    def _start_qemu(self) -> bool:
        """Launch PetraOS using `make run` with 4GB memory and monitor socket."""
        if self.monitor_sock.exists():
            self.monitor_sock.unlink()

        kvm_flag = "-enable-kvm -cpu host" if os.path.exists("/dev/kvm") and os.access("/dev/kvm", os.W_OK) else ""
        display_flag = "-display none" if self.headless else ""
        qemuflags = f"-m {self.memory} -serial stdio -monitor unix:{self.monitor_sock},server,nowait {display_flag} {kvm_flag}".strip()

        make_cmd = [
            "make",
            "run",
            f"QEMUFLAGS={qemuflags}",
        ]

        print(f"[INFO] Launching PetraOS in QEMU via: {' '.join(make_cmd)}")
        print(f"[INFO] Working directory: {self.repo_root}")
        print(f"[INFO] Configured Memory: {self.memory}")

        self.master_fd, self.slave_fd = pty.openpty()
        self.proc = subprocess.Popen(
            make_cmd,
            stdin=self.slave_fd,
            stdout=self.slave_fd,
            stderr=self.slave_fd,
            cwd=str(self.repo_root),
            close_fds=True,
        )
        os.close(self.slave_fd)
        self.slave_fd = None

        # Connect to monitor
        self.monitor = QemuMonitor(self.monitor_sock)
        if not self.monitor.connect(timeout=12.0):
            print("[WARN] Could not connect to QEMU monitor socket within 12s.", file=sys.stderr)

        # Wait for shell to become ready
        print("[INFO] Waiting for PetraOS boot & shell prompt...")
        self.is_ready = self._wait_for_shell_ready(timeout=self.boot_timeout)
        if not self.is_ready:
            print("[ERROR] Shell did not become ready within boot timeout!", file=sys.stderr)
            shot = self.capture_screenshot("boot_timeout_failure")
            if shot:
                print(f"[DIAGNOSTIC] Screenshot saved at: {shot}")
            return False

        print("[SUCCESS] Shell is ready to accept commands.")
        return True

    def _start_host(self) -> bool:
        """Launch host bash in PTY for local command testing."""
        print("[INFO] Launching local Host Bash session in PTY...")
        self.master_fd, self.slave_fd = pty.openpty()
        self.proc = subprocess.Popen(
            ["bash", "--norc", "--noprofile"],
            stdin=self.slave_fd,
            stdout=self.slave_fd,
            stderr=self.slave_fd,
            cwd=str(self.repo_root),
            close_fds=True,
        )
        os.close(self.slave_fd)
        self.slave_fd = None

        self.is_ready = self._wait_for_shell_ready(timeout=5.0)
        return self.is_ready

    def _read_available(self, timeout: float = 0.5) -> bytes:
        """Read available data from master PTY."""
        if self.master_fd is None:
            return b""
        rlist, _, _ = select.select([self.master_fd], [], [], timeout)
        if rlist:
            try:
                data = os.read(self.master_fd, 4096)
                self.full_log.extend(data)
                return data
            except (OSError, EOFError):
                return b""
        return b""

    def _wait_for_shell_ready(self, timeout: float) -> bool:
        """Wait until bash or shell prompt is detected and responding."""
        start = time.time()
        last_probe = 0.0

        while time.time() - start < timeout:
            if self.proc and self.proc.poll() is not None:
                print(f"[ERROR] Process exited prematurely with code {self.proc.returncode}!", file=sys.stderr)
                return False

            chunk = self._read_available(timeout=0.2)
            if chunk:
                sys.stdout.write(chunk.decode("utf-8", errors="replace"))
                sys.stdout.flush()

            # Check for crash patterns in serial output
            for pattern in FATAL_PATTERNS:
                if pattern.search(self.full_log):
                    print(f"\n[FATAL] Detected crash pattern in serial output: {pattern.pattern.decode()}", file=sys.stderr)
                    return False

            # Check if any prompt pattern is matched
            for prompt_re in PROMPT_PATTERNS:
                if prompt_re.search(self.full_log[-200:]):
                    return True

            # Send periodic newline probes after seeing init process activity
            now = time.time()
            if b"init" in self.full_log or now - start > 5.0:
                if now - last_probe > 2.0:
                    self._write_raw(b"\n")
                    last_probe = now

        return False

    def _write_raw(self, data: bytes):
        if self.master_fd is not None:
            os.write(self.master_fd, data)

    def capture_screenshot(self, tag: str = "snapshot") -> Optional[Path]:
        """Capture screenshot of the screen/display."""
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        safe_tag = re.sub(r"[^a-zA-Z0-9_\-]", "_", tag)
        png_path = self.screenshots_dir / f"{safe_tag}_{timestamp}.png"

        if self.target == "qemu" and self.monitor:
            shot = self.monitor.screendump(png_path)
            if shot:
                return shot

        # Fallback terminal buffer snapshot for host mode
        if HAS_PIL:
            try:
                lines = self.full_log.decode("utf-8", errors="replace").splitlines()[-35:]
                text = "\n".join(lines) if lines else "(No output recorded)"
                from PIL import ImageDraw
                img = Image.new("RGB", (900, 600), color=(20, 20, 24))
                draw = ImageDraw.Draw(img)
                draw.text((15, 15), f"CLI Terminal Snapshot [{tag}]\n" + "=" * 50 + "\n" + text, fill=(220, 220, 220))
                png_path.parent.mkdir(parents=True, exist_ok=True)
                img.save(png_path)
                return png_path
            except Exception as e:
                print(f"[WARN] Failed to render terminal fallback snapshot: {e}", file=sys.stderr)

        return None

    def run_command(self, cmd: str, timeout: Optional[float] = None) -> Tuple[str, int, float]:
        """
        Execute a shell command, wait for completion, and capture output and return code.
        Returns: (output_text, return_code, elapsed_time_seconds)
        """
        if timeout is None:
            timeout = self.cmd_timeout

        cmd_clean = cmd.strip()
        start_time = time.time()

        nonce = f"PETRA_{int(time.time()*1000)}"
        end_token = f"__END_{nonce}__"
        ret_prefix = f"__RET_{nonce}:"

        full_payload = f"{cmd_clean}\necho {ret_prefix}$?__\necho {end_token}\n"

        _ = self._read_available(timeout=0.1)

        self._write_raw(full_payload.encode("utf-8"))

        collected = bytearray()
        ret_code = -1
        completed = False

        while time.time() - start_time < timeout:
            if self.proc and self.proc.poll() is not None:
                break

            chunk = self._read_available(timeout=0.2)
            if chunk:
                collected.extend(chunk)
                sys.stdout.write(chunk.decode("utf-8", errors="replace"))
                sys.stdout.flush()

            if end_token.encode("utf-8") in collected:
                completed = True
                break

        elapsed = time.time() - start_time
        raw_text = collected.decode("utf-8", errors="replace")

        ret_match = re.search(rf"__RET_{nonce}:(\d+)__", raw_text)
        if ret_match:
            try:
                ret_code = int(ret_match.group(1))
            except ValueError:
                ret_code = -1

        clean_lines = []
        for raw_line in raw_text.splitlines():
            line = strip_ansi(raw_line).strip()
            if not line or nonce in line or end_token in line or ret_prefix in line or line == cmd_clean:
                continue
            clean_lines.append(line)

        output_str = "\n".join(clean_lines).strip()
        return output_str, ret_code, elapsed

    def execute_test(self, test: Dict[str, Any], screenshot_on_fail: bool = True) -> Dict[str, Any]:
        """Execute a single test specification."""
        name = test.get("name", "unnamed_test")
        cmd = test.get("command", "")
        expect_contains = test.get("expect_contains", [])
        if isinstance(expect_contains, str):
            expect_contains = [expect_contains]
        expect_not_contains = test.get("expect_not_contains", [])
        if isinstance(expect_not_contains, str):
            expect_not_contains = [expect_not_contains]
        expect_regex = test.get("expect_regex", None)
        expect_exit_code = test.get("expect_exit_code", None)
        timeout = test.get("timeout", self.cmd_timeout)

        print(f"\n▶ Running test: [{name}] : `{cmd}`")
        output, ret_code, elapsed = self.run_command(cmd, timeout=timeout)

        passed = True
        failure_reasons = []

        for exp in expect_contains:
            if exp not in output:
                passed = False
                failure_reasons.append(f"Expected substring '{exp}' not found in output.")

        for nexp in expect_not_contains:
            if nexp in output:
                passed = False
                failure_reasons.append(f"Forbidden substring '{nexp}' was detected in output.")

        if expect_regex:
            if not re.search(expect_regex, output):
                passed = False
                failure_reasons.append(f"Regex pattern '{expect_regex}' did not match output.")

        if expect_exit_code is not None:
            if ret_code != expect_exit_code:
                passed = False
                failure_reasons.append(f"Exit code {ret_code} did not match expected {expect_exit_code}.")

        screenshot_path = None
        if not passed:
            print(f"✖ [FAILED] {name}")
            for r in failure_reasons:
                print(f"  - {r}")
            if screenshot_on_fail:
                screenshot_path = self.capture_screenshot(f"fail_{name}")
                if screenshot_path:
                    print(f"  📸 Screenshot captured: {screenshot_path}")
        else:
            print(f"✔ [PASSED] {name} ({elapsed:.2f}s)")

        return {
            "name": name,
            "command": cmd,
            "status": "PASSED" if passed else "FAILED",
            "elapsed": round(elapsed, 2),
            "exit_code": ret_code,
            "output": output,
            "failure_reasons": failure_reasons,
            "screenshot": str(screenshot_path) if screenshot_path else None,
        }

    def close(self):
        """Clean up QEMU process, PTY, and monitor socket."""
        if self.monitor:
            try:
                self.monitor.send_command("quit")
            except Exception:
                pass
            self.monitor.close()

        if self.proc:
            if self.proc.poll() is None:
                try:
                    self.proc.terminate()
                    self.proc.wait(timeout=2.0)
                except Exception:
                    self.proc.kill()
            self.proc = None

        if self.master_fd is not None:
            try:
                os.close(self.master_fd)
            except Exception:
                pass
            self.master_fd = None


def generate_reports(results: List[Dict[str, Any]], reports_dir: Path, target: str) -> Tuple[Path, Path]:
    """Generate Markdown and JSON test reports."""
    reports_dir.mkdir(parents=True, exist_ok=True)
    json_path = reports_dir / "results.json"
    md_path = reports_dir / "report.md"

    passed_count = sum(1 for r in results if r["status"] == "PASSED")
    failed_count = sum(1 for r in results if r["status"] == "FAILED")
    total_count = len(results)

    summary_data = {
        "timestamp": datetime.now().isoformat(),
        "target": target,
        "total": total_count,
        "passed": passed_count,
        "failed": failed_count,
        "tests": results,
    }
    with open(json_path, "w", encoding="utf-8") as f:
        json.dump(summary_data, f, indent=2)

    with open(md_path, "w", encoding="utf-8") as f:
        f.write("# PetraOS CLI Test Report\n\n")
        f.write(f"- **Execution Time:** {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
        f.write(f"- **Target Environment:** `{target}`\n")
        f.write(f"- **Total Tests:** {total_count} | **Passed:** {passed_count} | **Failed:** {failed_count}\n\n")

        f.write("## Test Results Summary\n\n")
        f.write("| Status | Test Name | Command | Exit Code | Elapsed | Screenshot |\n")
        f.write("| :--- | :--- | :--- | :--- | :--- | :--- |\n")
        for r in results:
            status_icon = "✔ PASS" if r["status"] == "PASSED" else "✖ FAIL"
            shot_link = f"[Screenshot]({r['screenshot']})" if r.get("screenshot") else "-"
            f.write(f"| **{status_icon}** | {r['name']} | `{r['command']}` | {r['exit_code']} | {r['elapsed']}s | {shot_link} |\n")

        f.write("\n---\n\n")
        f.write("## Detailed Test Logs\n\n")
        for r in results:
            f.write(f"### Test: `{r['name']}`\n\n")
            f.write(f"- **Command:** `{r['command']}`\n")
            f.write(f"- **Status:** `{r['status']}`\n")
            if r.get("failure_reasons"):
                f.write(f"- **Failure Reasons:**\n")
                for reason in r["failure_reasons"]:
                    f.write(f"  - ⚠️ {reason}\n")
            if r.get("screenshot"):
                f.write(f"- **Failure Screenshot:** ![Screenshot]({r['screenshot']})\n")
            f.write("\n**Console Output:**\n```text\n")
            f.write(r["output"] if r["output"] else "(Empty output)")
            f.write("\n```\n\n")

    return md_path, json_path


def main():
    parser = argparse.ArgumentParser(description="PetraOS CLI & Shell Test Runner")
    parser.add_argument("--target", choices=["qemu", "host"], default="qemu", help="Target execution mode (default: qemu)")
    parser.add_argument("--cmd", type=str, help="Single command to execute and test")
    parser.add_argument("--expect", type=str, help="Expected substring in output for single command")
    parser.add_argument("--test-file", type=Path, default=DEFAULT_TEST_SUITE, help="JSON test suite file to run")
    parser.add_argument("--memory", type=str, default="4G", help="Memory to allocate to QEMU (default: 4G)")
    parser.add_argument("--boot-timeout", type=float, default=45.0, help="Seconds to wait for PetraOS boot (default: 45.0)")
    parser.add_argument("--cmd-timeout", type=float, default=15.0, help="Per-command timeout in seconds (default: 15.0)")
    parser.add_argument("--headless", action="store_true", default=True, help="Run QEMU with -display none (default: True)")
    parser.add_argument("--gui", dest="headless", action="store_false", help="Run QEMU with GUI display enabled")
    parser.add_argument("--screenshot-on-fail", action="store_true", default=True, help="Capture screenshot on test failure")
    parser.add_argument("--screenshot-always", action="store_true", help="Capture screenshot after every command")
    parser.add_argument("--screenshot-only", action="store_true", help="Boot, take a screenshot, and exit immediately")
    parser.add_argument("--reports-dir", type=Path, default=DEFAULT_REPORTS_DIR, help="Directory to save test reports and screenshots")

    args = parser.parse_args()

    screenshots_dir = args.reports_dir / "screenshots"
    screenshots_dir.mkdir(parents=True, exist_ok=True)

    session = CliTestSession(
        target=args.target,
        repo_root=DEFAULT_REPO_ROOT,
        headless=args.headless,
        screenshots_dir=screenshots_dir,
        boot_timeout=args.boot_timeout,
        cmd_timeout=args.cmd_timeout,
        memory=args.memory,
    )

    try:
        started = session.start()
        if not started:
            print("[ERROR] Failed to start test session.", file=sys.stderr)
            sys.exit(1)

        if args.screenshot_only:
            shot = session.capture_screenshot("manual_snapshot")
            print(f"[INFO] Snapshot captured at: {shot}")
            sys.exit(0)

        results = []

        if args.cmd:
            test_spec = {
                "name": "cli_single_cmd",
                "command": args.cmd,
                "expect_contains": [args.expect] if args.expect else [],
            }
            res = session.execute_test(test_spec, screenshot_on_fail=args.screenshot_on_fail)
            if args.screenshot_always:
                shot = session.capture_screenshot("always_single_cmd")
                res["screenshot"] = str(shot)
            results.append(res)
        else:
            if not args.test_file.exists():
                print(f"[ERROR] Test suite file not found: {args.test_file}", file=sys.stderr)
                sys.exit(1)

            with open(args.test_file, "r", encoding="utf-8") as f:
                suite = json.load(f)

            tests = suite.get("tests", [])
            print(f"\n==================================================")
            print(f" Running Test Suite: {suite.get('suite_name', 'PetraOS CLI Tests')} ({len(tests)} tests)")
            print(f"==================================================")

            for t in tests:
                res = session.execute_test(t, screenshot_on_fail=args.screenshot_on_fail)
                if args.screenshot_always:
                    shot = session.capture_screenshot(f"always_{t.get('name')}")
                    res["screenshot"] = str(shot)
                results.append(res)

        md_path, json_path = generate_reports(results, args.reports_dir, args.target)
        print(f"\n==================================================")
        print(f" Report generated at: {md_path}")
        print(f" JSON results at:     {json_path}")
        print(f"==================================================")

        failed = any(r["status"] == "FAILED" for r in results)
        sys.exit(1 if failed else 0)

    finally:
        session.close()


if __name__ == "__main__":
    main()
