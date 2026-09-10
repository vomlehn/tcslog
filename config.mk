# Optional per-system configuration for `make`-based builds.
#
# TIMER_RESOLUTION (nanoseconds) is intentionally not set here so that
# it is not committed with a value baked in. Provide it in one of two
# ways:
#
#   1. Preferred: copy `.cargo/config.toml.example` to
#      `.cargo/config.toml` and set TIMER_RESOLUTION there. Cargo
#      injects it into the environment for every cargo invocation, and
#      the file is gitignored so each machine keeps its own value.
#
#   2. Or export it in your shell / on the command line, e.g.
#         TIMER_RESOLUTION=1 make build
#      To feed it through this Makefile, uncomment and edit the line
#      below (do not commit the change):
#
# TCSLOG_CONFIG = TIMER_RESOLUTION=1

TCSLOG_CONFIG ?=
