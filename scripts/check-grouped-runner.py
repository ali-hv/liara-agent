"""Exercise the fixed deployment supervisor with harmless local child processes."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

source = (Path(__file__).resolve().parents[1] / "src-tauri/src/grouped_runner.py").read_text()
with tempfile.TemporaryDirectory(prefix="liara-runner-test-") as folder:
    root = Path(folder)
    proxy = root / "nginx"
    proxy.write_text(f"#!{sys.executable}\nimport os,sys,time\nfrom pathlib import Path\nif '-t' in sys.argv: sys.exit(0)\nPath(os.environ['PROXY_PID']).write_text(str(os.getpid()))\ntime.sleep(60)\n")
    proxy.chmod(0o700)
    env = {**os.environ, "PATH": str(root) + os.pathsep + os.environ.get("PATH", ""), "PROXY_PID": str(root / "proxy.pid")}
    runner = root / "runner.py"
    def render(command):
        runner.write_text(source.replace("__APP_COMMAND__", json.dumps(json.dumps(command))))
    render([sys.executable, "-c", "raise SystemExit(7)"])
    result = subprocess.run([sys.executable, str(runner)], env=env, timeout=10, capture_output=True)
    assert result.returncode == 7, result.stderr.decode()
    render([sys.executable, "-c", f"import os,time;from pathlib import Path;Path({str(root / 'app.pid')!r}).write_text(str(os.getpid()));time.sleep(60)"])
    process = subprocess.Popen([sys.executable, str(runner)], env=env)
    try:
        deadline = time.monotonic() + 8
        while not ((root / "app.pid").exists() and (root / "proxy.pid").exists()):
            assert time.monotonic() < deadline, "Child processes did not start"
            time.sleep(0.05)
        process.terminate()
        assert process.wait(timeout=8) == 0
        for name in ("app.pid", "proxy.pid"):
            pid = int((root / name).read_text())
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                continue
            raise AssertionError(f"Child process still running: {pid}")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
print("Grouped runner: failure propagation and graceful shutdown passed")
