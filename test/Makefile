# Make file for running non-built in tcslog tests

# Extra flags for the test scripts, which pass them through to
# error-recovery-common. Empty compares the verbose capture against the
# .expected files; `make test VERBOSE_EXPECTED=-q` compares the records
# alone against the -records.expected files instead. Both forms are
# stored, so either runs without regenerating the other.
VERBOSE_EXPECTED =

test: test-fixed test-variable test-variable-tsrc test-damage \
	test-overflow test-sessions test-rejected test-corrupt

.PHONY: test-fixed
test-fixed: recovery-fixed_1-1 recovery-fixed_8-8 recovery-fixed_5-17 \
	test-fixed-truncated_5-17

.PHONY: recovery-fixed_1-1
recovery-fixed_1-1:
	./recovery-fixed_1-1 $(VERBOSE_EXPECTED)

.PHONY: recovery-fixed_8-8
recovery-fixed_8-8:
	./recovery-fixed_8-8 $(VERBOSE_EXPECTED)

.PHONY: recovery-fixed_5-17
recovery-fixed_5-17:
	./recovery-fixed_5-17 $(VERBOSE_EXPECTED)

.PHONY: test-fixed-truncated_5-17
test-fixed-truncated_5-17:
	./fixed-truncated_5-17 $(VERBOSE_EXPECTED)

.PHONY: test-variable
test-variable: test-variable_0-10-17 test-variable_0-33-10 \
	test-variable-exact_6-20 test-variable-last_12-10

.PHONY: test-variable_0-10-17
test-variable_0-10-17:
	./variable_0-10-17 $(VERBOSE_EXPECTED)

.PHONY: test-variable_0-33-10
test-variable_0-33-10:
	./variable_0-33-10 $(VERBOSE_EXPECTED)

.PHONY: test-variable-exact_6-20
test-variable-exact_6-20:
	./variable-exact_6-20 $(VERBOSE_EXPECTED)

.PHONY: test-variable-last_12-10
test-variable-last_12-10:
	./variable-last_12-10 $(VERBOSE_EXPECTED)

.PHONY: test-variable-tsrc
test-variable-tsrc: test-variable-tsrc_0-10-21 \
	test-tsrc-truncated_10-24 test-tsrc-lost-header_8-24

.PHONY: test-variable-tsrc_0-10-21
test-variable-tsrc_0-10-21:
	./variable-tsrc_0-10-21 $(VERBOSE_EXPECTED)

.PHONY: test-tsrc-truncated_10-24
test-tsrc-truncated_10-24:
	./tsrc-truncated_10-24 $(VERBOSE_EXPECTED)

# A gap of exactly one segment file that swallowed one whole data
# header and no payload byte of the record it belonged to. That is the
# one gap whose arithmetic places the record continuing out of it, and
# combined_12-10 does recover it in variable-simple; here the lost
# header also held a timestamp and a record count, so the record must
# be given up instead of handed back with invented metadata.
.PHONY: test-tsrc-lost-header_8-24
test-tsrc-lost-header_8-24:
	./tsrc-lost-header_8-24 $(VERBOSE_EXPECTED)

# Damage that leaves the segment file in place, which deletion cannot
# reproduce: an unopenable header still leaves a sequence gap, while a
# short data section leaves the sequence intact and so must be reported
# with a lost count of zero.
.PHONY: test-damage
test-damage: test-truncated_12-10 test-combined test-hex-combined

.PHONY: test-truncated_12-10
test-truncated_12-10:
	./truncated_12-10 $(VERBOSE_EXPECTED)

.PHONY: test-combined
test-combined:
	./combined_12-10 $(VERBOSE_EXPECTED)

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
	./version_12-10 $(VERBOSE_EXPECTED)

.PHONY: test-segid_12-10
test-segid_12-10:
	./segid_12-10 $(VERBOSE_EXPECTED)

.PHONY: test-oversize_12-10
test-oversize_12-10:
	./oversize_12-10 $(VERBOSE_EXPECTED)

# A record too large for the caller's buffer, which exercises the
# overflow path rather than any segment-file damage. The count reported
# covers payload only, so both variable formats must capture the same
# 256 bytes despite their data headers differing in width.
.PHONY: test-overflow
test-overflow: test-overflow_300-64 test-tsrc-overflow_300-64

.PHONY: test-overflow_300-64
test-overflow_300-64:
	./overflow_300-64 $(VERBOSE_EXPECTED)

.PHONY: test-tsrc-overflow_300-64
test-tsrc-overflow_300-64:
	./tsrc-overflow_300-64 $(VERBOSE_EXPECTED)

# Several sessions in one directory, so the reader has to end each
# before reading the next.
.PHONY: test-sessions
test-sessions: test-sessions_8-40

.PHONY: test-sessions_8-40
test-sessions_8-40:
	./sessions_8-40 $(VERBOSE_EXPECTED)

# An unopenable segment header, in both variable formats: the
# variable-tsrc data header is the longer of the two and so offers the
# most bytes a resync could misread as a record length.
# The same log and damage as test-combined, read as hexadecimal rather
# than as the ASCII the rest of the suite stores. Having both covers
# each of tcslog-dump's two renderings end to end.
.PHONY: test-hex-combined
test-hex-combined:
	./hex-combined_12-10 $(VERBOSE_EXPECTED)

.PHONY: test-corrupt
test-corrupt: test-corrupt_12-10 test-tsrc-corrupt_10-24

.PHONY: test-corrupt_12-10
test-corrupt_12-10:
	./corrupt_12-10 $(VERBOSE_EXPECTED)

.PHONY: test-tsrc-corrupt_10-24
test-tsrc-corrupt_10-24:
	./tsrc-corrupt_10-24 $(VERBOSE_EXPECTED)

# Rebuild every expected file from the current output of the tools,
# masking timestamps so the results stay reproducible. Review
# "git diff expected/" afterwards: this records whatever the tools
# currently do, so a regression is captured just as readily as a fix.
.PHONY: regenerate
regenerate:
	@for t in *; do \
		case $$t in Makefile|expected) continue;; esac; \
		[ -f $$t ] && [ -x $$t ] || continue; \
		./$$t -g || exit 1; \
	done
