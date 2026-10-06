=============
TcsLog Prompt
=============

.. warning::

    **Deprecated.** This document has been renamed. Its contents are now
    ``docs/design.rst``, rendered as ``docs/design.html``. Nothing is left
    here but this notice, and it will be removed in a later release.

What Moved
==========
``docs/tcslog-prompt.rst`` is now ``docs/design.rst``. The text is
unchanged apart from its title, which is now "TcsLog Design".

Why
===
The name dated from when the file was a Claude Code prompt that the
sources were generated from. That stopped being true some releases ago:
it is now the design specification, maintained by hand and read rather
than run, and the old name said otherwise to anyone who found it.

Where to Look Now
=================
``docs/design.rst``
    The design specification -- the on-disk format, the status codes and
    their ABI numbers, and the behaviour the library is required to have.
    Formerly this file.

``docs/tcslog.rst``
    The user guide: what the library does, how to call it, and how it
    behaves.

A change in behaviour belongs in both.
