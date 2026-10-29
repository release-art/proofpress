"""PDF functions, enough to turn a spot colour's tint into the colour it stands in for.

A ``/Separation`` or ``/DeviceN`` colour space carries a tint transform: a function from the tint(s) to the alternate
space (usually CMYK). Evaluating it at full tint says what ink mixture a PANTONE or a die-line stands for on screen and
in proofs. All four function types are read (sampled, exponential, stitching, PostScript calculator); anything this does
not understand, or that runs away, gives ``None`` rather than a wrong colour.
"""

import math
import re
import typing

import pikepdf

_MAX_STEPS = 20000  # a calculator function is a few dozen operators; a runaway one is a malformed file
_MAX_DEPTH = 8


def evaluate(function: pikepdf.Object, inputs: list[float], *, depth: int = 0) -> list[float] | None:
    """The function's outputs for *inputs*, or None when it is of a kind or shape not understood."""
    if depth > _MAX_DEPTH:
        return None
    try:
        kind = int(function["/FunctionType"])
        domain = [float(v) for v in function["/Domain"]]
        x = [min(max(v, domain[2 * i]), domain[2 * i + 1]) for i, v in enumerate(inputs)]
        if kind == 2:
            out = _exponential(function, x[0])
        elif kind == 3:
            out = _stitching(function, x[0], domain, depth)
        elif kind == 0:
            out = _sampled(function, x, domain)
        elif kind == 4:
            out = _calculator(function, x)
        else:
            return None
        return _clip(function, out)
    except KeyError, IndexError, TypeError, ValueError, ZeroDivisionError, OverflowError, pikepdf.PdfError:
        return None


def _clip(function: pikepdf.Object, out: list[float] | None) -> list[float] | None:
    if out is None or "/Range" not in function:
        return out
    bounds = [float(v) for v in function["/Range"]]
    return [min(max(v, bounds[2 * i]), bounds[2 * i + 1]) for i, v in enumerate(out) if 2 * i + 1 < len(bounds)]


def _exponential(function: pikepdf.Object, x: float) -> list[float]:
    c0 = [float(v) for v in function.get("/C0", [0.0])]
    c1 = [float(v) for v in function.get("/C1", [1.0])]
    exponent = float(function["/N"])
    return [a + x**exponent * (b - a) for a, b in zip(c0, c1, strict=True)]


def _stitching(function: pikepdf.Object, x: float, domain: list[float], depth: int) -> list[float] | None:
    parts = list(function["/Functions"])
    bounds = [float(v) for v in function.get("/Bounds", [])]
    encode = [float(v) for v in function["/Encode"]]
    edges = [domain[0], *bounds, domain[1]]
    i = next((k for k in range(len(parts)) if x < edges[k + 1]), len(parts) - 1)
    lo, hi = edges[i], edges[i + 1]
    mapped = encode[2 * i] + ((x - lo) / (hi - lo) if hi != lo else 0.0) * (encode[2 * i + 1] - encode[2 * i])
    return evaluate(parts[i], [mapped], depth=depth + 1)


def _sampled(function: pikepdf.Object, x: list[float], domain: list[float]) -> list[float]:
    """The nearest sample (no interpolation): exact at the tints a spot colour is asked about, 0 and 1."""
    size = [int(v) for v in function["/Size"]]
    bits = int(function["/BitsPerSample"])
    bounds = [float(v) for v in function["/Range"]]
    outputs = len(bounds) // 2
    encode = [float(v) for v in function.get("/Encode", [v for s in size for v in (0, s - 1)])]
    decode = [float(v) for v in function.get("/Decode", bounds)]
    data = bytes(function.read_bytes())
    index, stride = 0, 1
    for i, value in enumerate(x):
        span = domain[2 * i + 1] - domain[2 * i]
        e = encode[2 * i] + ((value - domain[2 * i]) / span if span else 0.0) * (encode[2 * i + 1] - encode[2 * i])
        index += min(max(round(e), 0), size[i] - 1) * stride
        stride *= size[i]
    top = (1 << bits) - 1
    result = []
    for j in range(outputs):
        start = (index * outputs + j) * bits
        first, last = start // 8, (start + bits + 7) // 8
        piece = data[first:last]
        if not piece:
            raise ValueError("sample table is shorter than its Size says")
        sample = (int.from_bytes(piece, "big") >> (len(piece) * 8 - start % 8 - bits)) & top
        result.append(decode[2 * j] + sample / top * (decode[2 * j + 1] - decode[2 * j]))
    return result


# ---------------------------------------------------------------------------------------- PostScript calculator

_TOKEN = re.compile(rb"[{}]|[^\s{}]+")


def _calculator(function: pikepdf.Object, x: list[float]) -> list[float]:
    program = _parse(_TOKEN.findall(bytes(function.read_bytes())), [0])
    stack: list[typing.Any] = list(x)
    _run(program[0] if len(program) == 1 and isinstance(program[0], list) else program, stack, [0])
    outputs = len(function["/Range"]) // 2
    return [float(v) for v in stack[-outputs:]]


def _parse(tokens: list[bytes], at: list[int]) -> list[typing.Any]:
    items: list[typing.Any] = []
    while at[0] < len(tokens):
        token = tokens[at[0]]
        at[0] += 1
        if token == b"{":
            items.append(_parse(tokens, at))
        elif token == b"}":
            return items
        else:
            items.append(token.decode("latin-1"))
    return items


def _number(text: str) -> float | None:
    try:
        return float(text)
    except ValueError:
        return None


_UNARY: dict[str, typing.Callable[[float], float]] = {
    "abs": abs,
    "neg": lambda a: -a,
    "ceiling": lambda a: float(math.ceil(a)),
    "floor": lambda a: float(math.floor(a)),
    "round": lambda a: float(math.floor(a + 0.5)),
    "truncate": lambda a: float(math.trunc(a)),
    "sqrt": math.sqrt,
    "sin": lambda a: math.sin(math.radians(a)),
    "cos": lambda a: math.cos(math.radians(a)),
    "ln": math.log,
    "log": math.log10,
    "cvi": lambda a: float(math.trunc(a)),
    "cvr": float,
}
_BINARY: dict[str, typing.Callable[[typing.Any, typing.Any], typing.Any]] = {
    "add": lambda a, b: a + b,
    "sub": lambda a, b: a - b,
    "mul": lambda a, b: a * b,
    "div": lambda a, b: a / b,
    "idiv": lambda a, b: float(int(a) // int(b)),
    "mod": lambda a, b: float(math.fmod(int(a), int(b))),
    "exp": lambda a, b: a**b,
    "atan": lambda a, b: math.degrees(math.atan2(a, b)) % 360,
    "eq": lambda a, b: a == b,
    "ne": lambda a, b: a != b,
    "gt": lambda a, b: a > b,
    "ge": lambda a, b: a >= b,
    "lt": lambda a, b: a < b,
    "le": lambda a, b: a <= b,
    "and": lambda a, b: a and b if isinstance(a, bool) else float(int(a) & int(b)),
    "or": lambda a, b: a or b if isinstance(a, bool) else float(int(a) | int(b)),
    "xor": lambda a, b: a != b if isinstance(a, bool) else float(int(a) ^ int(b)),
    "bitshift": lambda a, b: float(int(a) << int(b) if b >= 0 else int(a) >> -int(b)),
}


def _run(program: list[typing.Any], stack: list[typing.Any], steps: list[int]) -> None:
    i = 0
    while i < len(program):
        steps[0] += 1
        if steps[0] > _MAX_STEPS:
            raise ValueError("calculator function does not end")
        word = program[i]
        i += 1
        if isinstance(word, list):  # a procedure waiting for if / ifelse
            nxt = program[i] if i < len(program) else None
            if nxt == "if":
                i += 1
                if stack.pop():
                    _run(word, stack, steps)
            elif isinstance(nxt, list) and i + 1 < len(program) and program[i + 1] == "ifelse":
                i += 2
                _run(word if stack.pop() else nxt, stack, steps)
            continue
        _word(word, stack)


def _roll(stack: list[typing.Any]) -> None:
    shift, count = int(stack.pop()), int(stack.pop())
    if count:
        part = stack[-count:]
        shift %= count
        stack[-count:] = part[-shift:] + part[:-shift] if shift else part


def _copy(stack: list[typing.Any]) -> None:
    count = int(stack.pop())
    stack.extend(stack[-count:] if count else [])


def _exch(stack: list[typing.Any]) -> None:
    stack[-1], stack[-2] = stack[-2], stack[-1]


def _not(stack: list[typing.Any]) -> None:
    top = stack.pop()
    stack.append(not top if isinstance(top, bool) else float(~int(top)))


_STACK: dict[str, typing.Callable[[list[typing.Any]], None]] = {
    "pop": lambda stack: stack.pop(),
    "dup": lambda stack: stack.append(stack[-1]),
    "exch": _exch,
    "copy": _copy,
    "index": lambda stack: stack.append(stack[-1 - int(stack.pop())]),
    "roll": _roll,
    "not": _not,
    "true": lambda stack: stack.append(True),
    "false": lambda stack: stack.append(False),
}


def _word(word: str, stack: list[typing.Any]) -> None:
    if (value := _number(word)) is not None:
        stack.append(value)
    elif word in _UNARY:
        stack.append(_UNARY[word](stack.pop()))
    elif word in _BINARY:
        b, a = stack.pop(), stack.pop()
        stack.append(_BINARY[word](a, b))
    elif word in _STACK:
        _STACK[word](stack)
    else:
        raise ValueError(f"unknown operator {word}")
