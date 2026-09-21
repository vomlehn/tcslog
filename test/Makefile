# Make file for running non-built in tcslog tests

test: test-fixed test-variable

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
