"""PyInstaller entry point for the bundled OpenAI Whisper CLI."""

import os
import sys

from whisper.transcribe import cli


def configure_utf8_stdio() -> None:
    os.environ.setdefault("PYTHONUTF8", "1")
    os.environ.setdefault("PYTHONIOENCODING", "utf-8")
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if hasattr(sys.stderr, "reconfigure"):
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")


if __name__ == "__main__":
    configure_utf8_stdio()
    if sys.argv[1:] == ["--self-test"]:
        print("whisper runtime ok")
        raise SystemExit(0)
    cli()
