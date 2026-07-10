"""PyInstaller entry point for the bundled OpenAI Whisper CLI."""

from whisper.transcribe import cli


if __name__ == "__main__":
    cli()
