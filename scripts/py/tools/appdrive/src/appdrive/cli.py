"""Thin CLI adapter over `appdrive.core` and `appdrive.win32` (Rule 2 — no logic here)."""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path
from typing import Annotated

import cyclopts

from appdrive.core import Box, SpecError, parse

app = cyclopts.App(
    name="appdrive",
    help=(
        "Drive a running window on Windows: focus it, type at it, click it, and capture "
        "what it shows. Built for the by-hand checks that only a running app can answer."
    ),
)

Process = Annotated[str, cyclopts.Parameter(help="Process name owning the window (no .exe needed)")]
DEFAULT_PROCESS = "steward-ide"


def _window(process: str):
    from appdrive.win32 import WindowNotFound, find

    try:
        return find(process)
    except WindowNotFound as err:
        print(err, file=sys.stderr)
        raise SystemExit(2) from err


def _focused(process: str):
    from appdrive.win32 import focus

    window = _window(process)
    if not focus(window):
        print(f"could not bring {process} to the foreground; refusing to send input", file=sys.stderr)
        raise SystemExit(3)
    return window


@app.command
def find(*, process: Process = DEFAULT_PROCESS, as_json: Annotated[bool, cyclopts.Parameter(name=["--json"])] = False) -> int:
    """Report the window: its handle, process, title and rectangle.

    Coordinates from here are the frame `click` and `crop` both use.
    """
    window = _window(process)
    if as_json:
        json.dump(window.__dict__, sys.stdout, indent=2)
        sys.stdout.write("\n")
    else:
        print(f"{window.title!r} pid={window.pid} at {window.left},{window.top} {window.width}x{window.height}")
    return 0


@app.command
def focus(*, process: Process = DEFAULT_PROCESS) -> int:
    """Bring the window to the foreground. Non-zero if it could not be raised."""
    _focused(process)
    print(f"{process} is in the foreground")
    return 0


@app.command
def keys(
    spec: Annotated[str, cyclopts.Parameter(help="Keystrokes, e.g. 'cls{ENTER}' or '^+`'")],
    *,
    process: Process = DEFAULT_PROCESS,
    settle: Annotated[float, cyclopts.Parameter(help="Seconds to wait after typing")] = 0.5,
    shot: Annotated[Path | None, cyclopts.Parameter(help="Capture to this PNG once settled")] = None,
) -> int:
    """Type a keystroke spec into the window.

    `^` `+` `%` hold ctrl, shift and alt for the next key; `{ENTER}` and friends are named
    keys; `{X}` is the single character X, which is how you type a modifier sign literally.
    """
    from appdrive.win32 import capture, send

    try:
        chords = parse(spec)
    except SpecError as err:
        print(err, file=sys.stderr)
        return 1

    window = _focused(process)
    send(chords)
    time.sleep(settle)
    if shot:
        capture(window).save(shot)
        print(f"{shot} {window.width}x{window.height}")
    return 0


@app.command
def click(
    x: int,
    y: int,
    *,
    process: Process = DEFAULT_PROCESS,
    settle: Annotated[float, cyclopts.Parameter(help="Seconds to wait after clicking")] = 0.5,
    shot: Annotated[Path | None, cyclopts.Parameter(help="Capture to this PNG once settled")] = None,
) -> int:
    """Click a point given in window coordinates — the frame `find` and `shot` report."""
    from appdrive.win32 import capture, click as do_click

    window = _focused(process)
    do_click(window, x, y)
    time.sleep(settle)
    if shot:
        capture(window).save(shot)
        print(f"{shot} {window.width}x{window.height}")
    return 0


@app.command
def shot(out: Path, *, process: Process = DEFAULT_PROCESS) -> int:
    """Capture the window to a PNG, even when it is behind another window."""
    from appdrive.win32 import capture

    window = _window(process)
    image = capture(window)
    image.save(out)
    print(f"{out} {image.width}x{image.height}")
    return 0


@app.command
def crop(
    source: Path,
    out: Path,
    *,
    x: int = 0,
    y: int = 0,
    width: Annotated[int, cyclopts.Parameter(help="Region width; 0 means to the right edge")] = 0,
    height: Annotated[int, cyclopts.Parameter(help="Region height; 0 means to the bottom edge")] = 0,
    scale: Annotated[int, cyclopts.Parameter(help="Nearest-neighbour zoom factor")] = 2,
) -> int:
    """Cut a region out of a capture and enlarge it, so small text can be read back."""
    from PIL import Image

    from appdrive.win32 import crop as do_crop

    image = Image.open(source)
    box = Box(x, y, width or image.width - x, height or image.height - y)
    try:
        result = do_crop(image, box, scale)
    except ValueError as err:
        print(err, file=sys.stderr)
        return 1
    result.save(out)
    print(f"{out} {result.width}x{result.height}")
    return 0


@app.command
def close(*, process: Process = DEFAULT_PROCESS) -> int:
    """Close the window the way its X does, so the app runs its own shutdown."""
    from appdrive.win32 import close as do_close

    window = _window(process)
    do_close(window)
    print(f"asked {process} to close")
    return 0


if __name__ == "__main__":
    sys.exit(app())
