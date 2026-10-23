"""Deployment supervisor for an application and its local reverse proxy."""
import json
import os
import signal
import subprocess
import sys
import time

COMMAND = json.loads(__APP_COMMAND__)
processes = []
stopping = False


def request_stop(signum=None, frame=None):
    global stopping
    stopping = True


signal.signal(signal.SIGTERM, request_stop)
signal.signal(signal.SIGINT, request_stop)
status = 0
try:
    subprocess.run(["nginx", "-t", "-c", "/app/liara-proxy/nginx.conf"], check=True, timeout=30)
    for command in (COMMAND, ["nginx", "-c", "/app/liara-proxy/nginx.conf", "-g", "daemon off;"]):
        if stopping:
            break
        processes.append(subprocess.Popen(command, start_new_session=True))
    while not stopping:
        for process in processes:
            code = process.poll()
            if code is not None:
                print("Deployment process exited:", process.args[0], code, flush=True)
                status = code if code else 1
                request_stop()
                break
        time.sleep(0.2)
finally:
    for process in processes:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    deadline = time.monotonic() + 20
    for process in processes:
        try:
            process.wait(timeout=max(0.1, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
sys.exit(status)
