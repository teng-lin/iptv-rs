"""Launch a packaged app with empty state; fail on an early crash or missing X window."""
import os
import signal
import subprocess
import sys
import tempfile
import time

with tempfile.TemporaryDirectory(prefix="patina-smoke-") as directory:
    env = os.environ.copy()
    env.update(HOME=directory, XDG_CONFIG_HOME=directory + "/config",
               XDG_DATA_HOME=directory + "/data", XDG_CACHE_HOME=directory + "/cache")
    with tempfile.TemporaryFile(mode="w+") as log:
        process = subprocess.Popen(sys.argv[1:], env=env, stdout=log,
                                   stderr=log, start_new_session=True)
        try:
            deadline = time.monotonic() + 30
            ready = None
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError(f"app exited early ({process.returncode})")
                window = sys.platform != "linux" or "Patina" in subprocess.check_output(
                    ["xwininfo", "-root", "-tree"], text=True)
                if window:
                    ready = ready or time.monotonic()
                    if time.monotonic() - ready >= 5:
                        print("Packaged app launched and stayed open with fresh state")
                        break
                time.sleep(0.2)
            else:
                raise RuntimeError("app did not create a Patina window")
        except Exception:
            log.seek(0)
            print(log.read(), file=sys.stderr)
            raise
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
