# Make file for running non-built in tcslog tests

test: test-fixed test-variable test-variable-tsrc test-damage \
	test-overflow test-sessions test-rejected test-corrupt

.PHONY: test-fixed
test-fixed: recovery-fixed_1-1 recovery-fixed_8-8 recovery-fixed_5-17 \
	test-fixed-truncated_5-17

.PHONY: recovery-fixed_1-1
recovery-fixed_1-1:
	./recovery-fixed_1-1

.PHONY: recovery-fixed_8-8
recovery-fixed_8-8:
	./recovery-fixed_8-8

.PHONY: recovery-fixed_5-17
recovery-fixed_5-17:
	./recovery-fixed_5-17

.PHONY: test-fixed-truncated_5-17
test-fixed-truncated_5-17:
	./fixed-truncated_5-17

.PHONY: test-variable
test-variable: test-variable_0-10-17 test-variable_0-33-10 \
	test-variable-exact_6-20 test-variable-last_12-10

.PHONY: test-variable_0-10-17
test-variable_0-10-17:
	./variable_0-10-17

.PHONY: test-variable_0-33-10
test-variable_0-33-10:
	./variable_0-33-10

.PHONY: test-variable-exact_6-20
test-variable-exact_6-20:
	./variable-exact_6-20

.PHONY: test-variable-last_12-10
test-variable-last_12-10:
	./variable-last_12-10

.PHONY: test-variable-tsrc
test-variable-tsrc: test-variable-tsrc_0-10-21 \
	test-tsrc-truncated_10-24

.PHONY: test-variable-tsrc_0-10-21
test-variable-tsrc_0-10-21:
	./variable-tsrc_0-10-21

.PHONY: test-tsrc-truncated_10-24
test-tsrc-truncated_10-24:
	./tsrc-truncated_10-24

# Damage that leaves the segment file in place, which deletion cannot
# reproduce: an unopenable header still leaves a sequence gap, while a
# short data section leaves the sequence intact and so must be reported
# with a lost count of zero.
.PHONY: test-damage
test-damage: test-truncated_12-10 test-combined

.PHONY: test-truncated_12-10
test-truncated_12-10:
	./truncated_12-10

.PHONY: test-combined
test-combined:
	./combined_12-10

# Segment files the reader must refuse outright rather than read with
# the wrong assumptions: an unreadable version, a stored segment id that
# disagrees with the file name, and a file larger than the max_size its
# own header declares. All three run before the format tag in the header
# is consulted, so one format exercises them; only the overflow path,
# which decodes a data header, needs a variable-tsrc case of its own.
.PHONY: test-rejected
test-rejected: test-version_12-10 test-segid_12-10 test-oversize_12-10

.PHONY: test-version_12-10
test-version_12-10:
	./version_12-10

.PHONY: test-segid_12-10
test-segid_12-10:
	./segid_12-10

.PHONY: test-oversize_12-10
test-oversize_12-10:
	./oversize_12-10

# A record too large for the caller's buffer, which exercises the
# overflow path rather than any segment-file damage. The count reported
# covers payload only, so both variable formats must capture the same
# 256 bytes despite their data headers differing in width.
.PHONY: test-overflow
test-overflow: test-overflow_300-64 test-tsrc-overflow_300-64

.PHONY: test-overflow_300-64
test-overflow_300-64:
	./overflow_300-64

.PHONY: test-tsrc-overflow_300-64
test-tsrc-overflow_300-64:
	./tsrc-overflow_300-64

# Several sessions in one directory, so the reader has to end each
# before reading the next.
.PHONY: test-sessions
test-sessions: test-sessions_8-40

.PHONY: test-sessions_8-40
test-sessions_8-40:
	./sessions_8-40

# An unopenable segment header, in both variable formats: the
# variable-tsrc data header is the longer of the two and so offers the
# most bytes a resync could misread as a record length.
.PHONY: test-corrupt
test-corrupt: test-corrupt_12-10 test-tsrc-corrupt_10-24

.PHONY: test-corrupt_12-10
test-corrupt_12-10:
	./corrupt_12-10 -k

.PHONY: test-tsrc-corrupt_10-24
test-tsrc-corrupt_10-24:
	./tsrc-corrupt_10-24

# Rebuild every expected file from the current output of the tools,
# masking timestamps so the results stay reproducible. Review
# "git diff expected/" afterwards: this records whatever the tools
# currently do, so a regression is captured just as readily as a fix.
.PHONY: regenerate
regenerate:
	@for t in *; do \
		case $$t in Makefile|expected|dump-before|error-recovery-common) continue;; esac; \
		[ -f $$t ] && [ -x $$t ] || continue; \
		./$$t -g || exit 1; \
	done
