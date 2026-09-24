# Make file for running non-built in tcslog tests

test: test-fixed test-variable test-variable-tsrc test-damage \
	test-overflow test-sessions

.PHONY: test-fixed
test-fixed: test-fixed_1-1 test-fixed_8-8 test-fixed_5-17

.PHONY: test-fixed_1-1
test-fixed_1-1:
	./run-error-recovery-fixed_1-1

.PHONY: test-fixed_8-8
test-fixed_8-8:
	./run-error-recovery-fixed_8-8

.PHONY: test-fixed_5-17
test-fixed_5-17:
	./run-error-recovery-fixed_5-17

.PHONY: test-variable
test-variable: test-variable_0-10-17 test-variable_0-33-10 \
	test-variable-exact_6-20 test-variable-last_12-10

.PHONY: test-variable_0-10-17
test-variable_0-10-17:
	./run-error-recovery-variable_0-10-17

.PHONY: test-variable_0-33-10
test-variable_0-33-10:
	./run-error-recovery-variable_0-33-10

.PHONY: test-variable-exact_6-20
test-variable-exact_6-20:
	./run-error-recovery-variable-exact_6-20

.PHONY: test-variable-last_12-10
test-variable-last_12-10:
	./run-error-recovery-variable-last_12-10

.PHONY: test-variable-tsrc
test-variable-tsrc: test-variable-tsrc_0-10-21

.PHONY: test-variable-tsrc_0-10-21
test-variable-tsrc_0-10-21:
	./run-error-recovery-variable-tsrc_0-10-21

# Damage that leaves the segment file in place, which deletion cannot
# reproduce: an unopenable header still leaves a sequence gap, while a
# short data section leaves the sequence intact and so must be reported
# with a lost count of zero.
.PHONY: test-damage
test-damage: test-corrupt_12-10 test-truncated_12-10

.PHONY: test-corrupt_12-10
test-corrupt_12-10:
	./run-error-recovery-corrupt_12-10

.PHONY: test-truncated_12-10
test-truncated_12-10:
	./run-error-recovery-truncated_12-10

# A record too large for the caller's buffer, which exercises the
# overflow path rather than any segment-file damage.
.PHONY: test-overflow
test-overflow: test-overflow_300-64

.PHONY: test-overflow_300-64
test-overflow_300-64:
	./run-error-recovery-overflow_300-64

# Several sessions in one directory, so the reader has to end each
# before reading the next.
.PHONY: test-sessions
test-sessions: test-sessions_8-40

.PHONY: test-sessions_8-40
test-sessions_8-40:
	./run-error-recovery-sessions_8-40

# Rebuild every expected file from the current output of the tools,
# masking timestamps so the results stay reproducible. Review
# "git diff expected/" afterwards: this records whatever the tools
# currently do, so a regression is captured just as readily as a fix.
.PHONY: regenerate
regenerate:
	@for t in run-error-recovery-*; do ./$$t -g || exit 1; done
