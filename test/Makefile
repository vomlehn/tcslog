# Make file for running non-built in tcslog tests

test: test-fixed test-variable test-variable-tsrc

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
test-variable: test-variable_0-10-17 test-variable_0-33-10

.PHONY: test-variable_0-10-17
test-variable_0-10-17:
	./run-error-recovery-variable_0-10-17

.PHONY: test-variable_0-33-10
test-variable_0-33-10:
	./run-error-recovery-variable_0-33-10

.PHONY: test-variable-tsrc
test-variable-tsrc: test-variable-tsrc_0-10-21

.PHONY: test-variable-tsrc_0-10-21
test-variable-tsrc_0-10-21:
	./run-error-recovery-variable-tsrc_0-10-21

# Rebuild every expected file from the current output of the tools,
# masking timestamps so the results stay reproducible. Review
# "git diff expected/" afterwards: this records whatever the tools
# currently do, so a regression is captured just as readily as a fix.
.PHONY: regenerate
regenerate:
	@for t in run-error-recovery-*; do ./$$t -g || exit 1; done
