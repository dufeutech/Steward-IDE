"""The Windows adapter: everything that touches user32/gdi32 lives here (Rule 2).

Two behaviours in here were learned the hard way and are the reason this is a tool rather
than a snippet — see `focus` and `capture`.
"""

from __future__ import annotations

import ctypes
import sys
import time
from ctypes import wintypes
from dataclasses import dataclass

from appdrive.core import Box, Chord

if sys.platform != "win32":  # pragma: no cover - the tool is Windows-only by nature
    raise ImportError("appdrive drives Win32 windows and only runs on Windows")

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

user32.SetProcessDPIAware()

WM_CLOSE = 0x0010
SW_RESTORE = 9
PW_RENDERFULLCONTENT = 2
INPUT_KEYBOARD = 1
INPUT_MOUSE = 0
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000

VK = {
    "ENTER": 0x0D,
    "TAB": 0x09,
    "ESC": 0x1B,
    "BACKSPACE": 0x08,
    "DELETE": 0x2E,
    "INSERT": 0x2D,
    "HOME": 0x24,
    "END": 0x23,
    "PGUP": 0x21,
    "PGDN": 0x22,
    "LEFT": 0x25,
    "UP": 0x26,
    "RIGHT": 0x27,
    "DOWN": 0x28,
    "SPACE": 0x20,
    **{f"F{n}": 0x6F + n for n in range(1, 13)},
}
VK_SHIFT, VK_CONTROL, VK_MENU = 0x10, 0x11, 0x12


class RECT(ctypes.Structure):
    _fields_ = [("left", wintypes.LONG), ("top", wintypes.LONG), ("right", wintypes.LONG), ("bottom", wintypes.LONG)]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", wintypes.WORD),
        ("wScan", wintypes.WORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
    ]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", wintypes.LONG),
        ("dy", wintypes.LONG),
        ("mouseData", wintypes.DWORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
    ]


class _INPUTUNION(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT)]


class INPUT(ctypes.Structure):
    _anonymous_ = ("u",)
    _fields_ = [("type", wintypes.DWORD), ("u", _INPUTUNION)]


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", wintypes.LONG),
        ("biHeight", wintypes.LONG),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", wintypes.LONG),
        ("biYPelsPerMeter", wintypes.LONG),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]


class WindowNotFound(RuntimeError):
    """No visible top-level window belongs to a process with that name."""


@dataclass(frozen=True)
class Window:
    handle: int
    pid: int
    title: str
    left: int
    top: int
    width: int
    height: int


def _process_name(pid: int) -> str:
    handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not handle:
        return ""
    try:
        size = wintypes.DWORD(260)
        buffer = ctypes.create_unicode_buffer(size.value)
        if not kernel32.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size)):
            return ""
        return buffer.value.rsplit("\\", 1)[-1].removesuffix(".exe")
    finally:
        kernel32.CloseHandle(handle)


def find(process: str) -> Window:
    """The first visible top-level window owned by a process of that name."""
    found: list[Window] = []
    enum_proc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def visit(handle: int, _param: int) -> bool:
        if not user32.IsWindowVisible(handle):
            return True
        pid = wintypes.DWORD()
        user32.GetWindowThreadProcessId(handle, ctypes.byref(pid))
        if _process_name(pid.value).lower() != process.lower().removesuffix(".exe"):
            return True
        length = user32.GetWindowTextLengthW(handle)
        title = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(handle, title, length + 1)
        rect = RECT()
        user32.GetWindowRect(handle, ctypes.byref(rect))
        found.append(
            Window(
                handle=handle,
                pid=pid.value,
                title=title.value,
                left=rect.left,
                top=rect.top,
                width=rect.right - rect.left,
                height=rect.bottom - rect.top,
            )
        )
        return False

    user32.EnumWindows(enum_proc(visit), 0)
    if not found:
        raise WindowNotFound(f"no visible window belongs to a process named {process!r}")
    return found[0]


def focus(window: Window) -> bool:
    """Bring the window to the foreground. Returns whether it actually got there.

    Windows refuses `SetForegroundWindow` from a process that does not already own the
    foreground, so this attaches to the current foreground thread's input queue for the
    length of the call.

    The widely-copied alternative — tapping ALT first to release the lock — is deliberately
    NOT used: a lone ALT tap leaves the window in menu-bar state, so the next space
    character opens the system menu and the letter after it picks an entry. A typed command
    beginning with a space chose **Close** that way and killed the app mid-session.
    """
    user32.ShowWindow(window.handle, SW_RESTORE)
    foreground = user32.GetForegroundWindow()
    their_thread = user32.GetWindowThreadProcessId(foreground, None)
    our_thread = kernel32.GetCurrentThreadId()

    user32.AttachThreadInput(our_thread, their_thread, True)
    try:
        user32.BringWindowToTop(window.handle)
        user32.SetForegroundWindow(window.handle)
    finally:
        user32.AttachThreadInput(our_thread, their_thread, False)

    time.sleep(0.4)
    return user32.GetForegroundWindow() == window.handle


def _send(*inputs: INPUT) -> None:
    array = (INPUT * len(inputs))(*inputs)
    user32.SendInput(len(inputs), array, ctypes.sizeof(INPUT))


def _key(vk: int, up: bool = False) -> INPUT:
    return INPUT(type=INPUT_KEYBOARD, ki=KEYBDINPUT(wVk=vk, wScan=0, dwFlags=KEYEVENTF_KEYUP if up else 0))


def _unicode(char: str, up: bool = False) -> INPUT:
    flags = KEYEVENTF_UNICODE | (KEYEVENTF_KEYUP if up else 0)
    return INPUT(type=INPUT_KEYBOARD, ki=KEYBDINPUT(wVk=0, wScan=ord(char), dwFlags=flags))


def send(chords: tuple[Chord, ...], delay: float = 0.01) -> None:
    """Type the chords into whatever currently has focus.

    Plain characters go in as Unicode scan codes rather than virtual keys, so what is typed
    does not depend on the machine's keyboard layout — the difference between a spec that
    works here and one that works everywhere. A chord with modifiers has to use a virtual
    key instead, because a modifier only means anything against one.
    """
    for chord in chords:
        held = [vk for vk, on in ((VK_CONTROL, chord.ctrl), (VK_SHIFT, chord.shift), (VK_MENU, chord.alt)) if on]
        for vk in held:
            _send(_key(vk))
        try:
            if chord.named:
                _send(_key(VK[chord.key]), _key(VK[chord.key], up=True))
            elif held:
                scan = user32.VkKeyScanW(ctypes.c_wchar(chord.key))
                vk = scan & 0xFF
                _send(_key(vk), _key(vk, up=True))
            else:
                _send(_unicode(chord.key), _unicode(chord.key, up=True))
        finally:
            for vk in reversed(held):
                _send(_key(vk, up=True))
        time.sleep(delay)


def click(window: Window, x: int, y: int, settle: float = 0.2) -> None:
    """Click a point given in window coordinates — the same frame a capture reports."""
    point = wintypes.POINT()
    user32.GetCursorPos(ctypes.byref(point))
    rect = RECT()
    user32.GetWindowRect(window.handle, ctypes.byref(rect))
    user32.SetCursorPos(rect.left + x, rect.top + y)
    time.sleep(settle)
    _send(INPUT(type=INPUT_MOUSE, mi=MOUSEINPUT(dwFlags=MOUSEEVENTF_LEFTDOWN)))
    time.sleep(0.05)
    _send(INPUT(type=INPUT_MOUSE, mi=MOUSEINPUT(dwFlags=MOUSEEVENTF_LEFTUP)))
    time.sleep(settle)
    user32.SetCursorPos(point.x, point.y)


def capture(window: Window):
    """A PIL image of the window's own surface.

    Uses `PrintWindow` with `PW_RENDERFULLCONTENT` rather than copying from the screen, so
    a window that is behind another one still yields its real content. Copying from the
    screen returns whatever is on top at those coordinates, which looks like a working
    capture right up until it silently photographs the wrong application.
    """
    from PIL import Image

    rect = RECT()
    user32.GetWindowRect(window.handle, ctypes.byref(rect))
    width, height = rect.right - rect.left, rect.bottom - rect.top

    window_dc = user32.GetWindowDC(window.handle)
    memory_dc = gdi32.CreateCompatibleDC(window_dc)
    bitmap = gdi32.CreateCompatibleBitmap(window_dc, width, height)
    previous = gdi32.SelectObject(memory_dc, bitmap)
    try:
        if not user32.PrintWindow(window.handle, memory_dc, PW_RENDERFULLCONTENT):
            raise RuntimeError("PrintWindow refused to render the window")

        info = BITMAPINFO()
        info.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
        info.bmiHeader.biWidth = width
        # Negative height asks GDI for a top-down image, matching PIL's row order.
        info.bmiHeader.biHeight = -height
        info.bmiHeader.biPlanes = 1
        info.bmiHeader.biBitCount = 32
        info.bmiHeader.biCompression = 0

        buffer = ctypes.create_string_buffer(width * height * 4)
        gdi32.GetDIBits(memory_dc, bitmap, 0, height, buffer, ctypes.byref(info), 0)
        return Image.frombuffer("RGB", (width, height), buffer, "raw", "BGRX", 0, 1)
    finally:
        gdi32.SelectObject(memory_dc, previous)
        gdi32.DeleteObject(bitmap)
        gdi32.DeleteDC(memory_dc)
        user32.ReleaseDC(window.handle, window_dc)


def crop(image, box: Box, scale: int):
    """Cut a region out and enlarge it without smoothing.

    Nearest-neighbour on purpose: the point of a zoom here is to read glyphs and compare
    column positions, and interpolation invents pixels between them.
    """
    from PIL import Image

    region = image.crop(box.clamped(image.size).as_pil())
    if scale == 1:
        return region
    return region.resize((region.width * scale, region.height * scale), Image.NEAREST)


def close(window: Window) -> None:
    """Ask the window to close, the way clicking its X does — never a forced kill.

    The difference matters for anything that checks cleanup on exit: a forced kill skips
    the application's own shutdown, so it cannot tell you whether that shutdown works.
    """
    user32.SendMessageW(window.handle, WM_CLOSE, 0, 0)
