"""What a keystroke spec means, and where a crop lands. No Windows API here.

The parsing rules are a subset of the .NET `SendKeys` syntax, because that is what a
reader will already have in their fingers — but only the subset this tool sends, so a
spec that parses is a spec that works.
"""

from __future__ import annotations

from dataclasses import dataclass

MODIFIERS = {"^": "ctrl", "+": "shift", "%": "alt"}

#: Named keys, spelled as `{NAME}`. Kept deliberately small: every entry here is one this
#: tool can actually send, so an unknown name is a spec error rather than a silent no-op.
NAMED_KEYS = frozenset(
    {
        "ENTER",
        "TAB",
        "ESC",
        "BACKSPACE",
        "DELETE",
        "INSERT",
        "HOME",
        "END",
        "PGUP",
        "PGDN",
        "UP",
        "DOWN",
        "LEFT",
        "RIGHT",
        "SPACE",
        *(f"F{n}" for n in range(1, 13)),
    }
)


class SpecError(ValueError):
    """The keystroke spec is malformed — raised before anything is sent."""


@dataclass(frozen=True)
class Chord:
    """One keystroke: a key, plus whichever modifiers are held down for it."""

    key: str
    named: bool = False
    ctrl: bool = False
    shift: bool = False
    alt: bool = False

    def __str__(self) -> str:
        held = "".join(m for m, on in (("ctrl+", self.ctrl), ("shift+", self.shift), ("alt+", self.alt)) if on)
        return f"{held}{{{self.key}}}" if self.named else f"{held}{self.key}"


def parse(spec: str) -> tuple[Chord, ...]:
    """Turn a keystroke spec into the chords it stands for.

    `^`, `+` and `%` hold ctrl, shift and alt for the *next* key only. `{NAME}` is a named
    key; `{X}` for a single character is that character literally, which is how a spec
    types a modifier sign or a brace.

        parse("cls{ENTER}")   -> c, l, s, {ENTER}
        parse("^+`")          -> ctrl+shift+`
        parse("{^}")          -> a literal caret
    """
    chords: list[Chord] = []
    pending = {"ctrl": False, "shift": False, "alt": False}
    index = 0

    while index < len(spec):
        char = spec[index]

        if char in MODIFIERS:
            pending[MODIFIERS[char]] = True
            index += 1
            continue

        if char == "{":
            close = spec.find("}", index + 1)
            # `{}}` is a literal closing brace: the token is empty and the brace follows.
            if close == index + 1 and spec[index + 1 : index + 3] == "}}":
                close = index + 2
            if close == -1:
                raise SpecError(f"unclosed {{ at position {index} in {spec!r}")
            token = spec[index + 1 : close]
            if len(token) == 1:
                chords.append(Chord(key=token, **pending))
            elif token.upper() in NAMED_KEYS:
                chords.append(Chord(key=token.upper(), named=True, **pending))
            else:
                raise SpecError(
                    f"unknown key {{{token}}} — named keys are {', '.join(sorted(NAMED_KEYS))}"
                )
            pending = {"ctrl": False, "shift": False, "alt": False}
            index = close + 1
            continue

        if char == "}":
            raise SpecError(f"unbalanced }} at position {index} in {spec!r}")

        chords.append(Chord(key=char, **pending))
        pending = {"ctrl": False, "shift": False, "alt": False}
        index += 1

    if any(pending.values()):
        raise SpecError(f"{spec!r} ends with a modifier that holds nothing")
    return tuple(chords)


@dataclass(frozen=True)
class Box:
    """A crop rectangle, in the same coordinates a capture is reported in."""

    left: int
    top: int
    width: int
    height: int

    def clamped(self, size: tuple[int, int]) -> Box:
        """Trim to what the image actually contains.

        A crop that runs off the edge is the normal case when reading a region back from a
        window that has since been resized; refusing it would be less useful than showing
        the part that exists.
        """
        image_width, image_height = size
        left = max(0, min(self.left, image_width))
        top = max(0, min(self.top, image_height))
        width = max(0, min(self.width, image_width - left))
        height = max(0, min(self.height, image_height - top))
        if width == 0 or height == 0:
            raise ValueError(f"{self} lies entirely outside a {image_width}x{image_height} image")
        return Box(left, top, width, height)

    def as_pil(self) -> tuple[int, int, int, int]:
        return (self.left, self.top, self.left + self.width, self.top + self.height)
