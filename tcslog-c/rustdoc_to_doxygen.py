"""Turn the rustdoc sections cbindgen copies into tcslog.h into Doxygen.

cbindgen copies each function's doc comment from tcslog-c/src/lib.rs
into the header as it stands, so the header would document parameters
and results in rustdoc's form, which a C reader's tools do not know:

    * # Arguments
    *
    * * `h`   - The writer.
    * * `out` - Where the result goes.
    *
    * # Returns
    *
    * The status.

This filter rewrites the two sections as Doxygen commands at the end of
the comment, wrapping the text to 80 columns with every description in
one column:

    * @param h    The writer.
    * @param out  Where the result goes.
    * @return     The status.

Other sections (``# Errors``, ``# Safety``) are left as they are; Doxygen
reads them as Markdown headings.

usage: cbindgen ... | python3 -I rustdoc_to_doxygen.py > tcslog.h
"""

import re
import sys
import textwrap

WIDTH = 80
ENTRY = re.compile(r"\* `([^`]+)` +- (.*)")


def paragraphs(lines: list[str]) -> list[str]:
    """Joins a section's lines into its paragraphs and list items.

    Args:
        lines: The section's text, without the comment's `` * `` prefix.

    Returns:
        Each paragraph, or each item of a list, as one line, in order.
        A list item keeps its marker.
    """
    out: list[str] = []
    cur: list[str] = []
    for ln in lines:
        s = ln.strip()
        if not s or re.match(r"([*-]|\d+\.) ", s):
            if cur:
                out.append(" ".join(cur))
            cur = [s] if s else []
        else:
            cur.append(s)
    if cur:
        out.append(" ".join(cur))
    return out


def command(prefix: str, tag: str, col: int, text: list[str]) -> list[str]:
    """Writes one Doxygen command with its text wrapped under a column.

    Args:
        prefix: The comment's line prefix, `` * `` with any indent.
        tag:    The command and its name, such as ``@param h``.
        col:    The column, within the comment's text, where every
                description line starts.
        text:   The description's paragraphs and list items.

    Returns:
        The command's lines, the first naming it.
    """
    lines: list[str] = []
    for i, para in enumerate(text):
        first = tag.ljust(col) if i == 0 else " " * col
        lines += textwrap.wrap(para, WIDTH - len(prefix),
                               initial_indent=first,
                               subsequent_indent=" " * col,
                               break_long_words=False,
                               break_on_hyphens=False)
    return [(prefix + ln).rstrip() for ln in lines]


def sections(body: list[str], prefix: str) -> dict[str, list[str]]:
    """Finds the Arguments and Returns sections in one comment.

    Args:
        body:   The comment's lines between ``/**`` and `` */``.
        prefix: The prefix each of those lines starts with.

    Returns:
        The text of the ``# Arguments`` and ``# Returns`` sections that
        the comment has, without the prefix, by heading.
    """
    found: dict[str, list[str]] = {}
    cur = None
    for ln in body:
        text = ln[len(prefix):] if ln.startswith(prefix) else ""
        m = re.fullmatch(r"# (\w+)", text.strip())
        if m:
            cur = m.group(1) if m.group(1) in ("Arguments", "Returns") \
                else None
            if cur:
                found[cur] = []
        elif cur:
            found[cur].append(text)
    return found


def convert(body: list[str]) -> list[str]:
    """Rewrites one comment's Arguments and Returns sections as Doxygen.

    Args:
        body: The comment's lines between ``/**`` and `` */``.

    Returns:
        The comment's lines without the two sections, followed by a
        blank line and the commands that replace them.

    Raises:
        ValueError: A line in the Arguments section is neither an entry
            nor the continuation of one.
    """
    m = re.match(r"(\s*\* ?)", body[0]) if body else None
    if not m:
        return body
    prefix = m.group(1).rstrip() + " "
    secs = sections(body, prefix)
    if not secs:
        return body
    entries: list[tuple[str, list[str]]] = []
    for ln in secs.get("Arguments", []):
        em = ENTRY.match(ln.strip())
        if em:
            entries.append((em.group(1), [em.group(2)]))
        elif ln.strip() and entries:
            entries[-1][1].append(ln)
        elif ln.strip():
            raise ValueError(f"not an argument entry: {ln.strip()}")
    names = [f"@param {n}" for n, _ in entries] + ["@return"]
    col = max(len(n) for n in names) + (2 if entries else 1)
    cmds: dict[str, list[str]] = {
        "Arguments": [c for n, d in entries
                      for c in command(prefix, f"@param {n}", col,
                                       paragraphs(d))],
        "Returns": command(prefix, "@return", col,
                           paragraphs(secs.get("Returns", []))),
    }
    # The commands go last, where Doxygen comments usually have them,
    # so that the other sections' text (# Safety, say) is read as the
    # description's rather than as a run-on of the last command's.
    out: list[str] = []
    in_cmds = False
    for ln in body:
        text = ln[len(prefix):].strip() if ln.startswith(prefix) else ""
        h = re.fullmatch(r"# (\w+)", text)
        if h:
            in_cmds = h.group(1) in cmds
        if not in_cmds:
            out.append(ln)
    while out and not out[-1].strip(" *"):
        out.pop()
    return out + [prefix.rstrip()] + cmds["Arguments"] + cmds["Returns"]


def main() -> int:
    """Filters a header from standard input to standard output.

    Returns:
        The exit status: 0, or 1 if a comment could not be converted.
    """
    lines = sys.stdin.read().split("\n")
    out: list[str] = []
    i = 0
    while i < len(lines):
        ln = lines[i]
        if ln.strip() != "/**":
            out.append(ln)
            i += 1
            continue
        j = i + 1
        while j < len(lines) and lines[j].strip() != "*/":
            j += 1
        try:
            out += [ln] + convert(lines[i + 1:j])
        except ValueError as e:
            print(f"rustdoc_to_doxygen: line {i + 1}: {e}", file=sys.stderr)
            return 1
        i = j
    sys.stdout.write("\n".join(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
