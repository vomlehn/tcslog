O = ../OUT

.PHONY: all
all:	tcslog.html design.html tcslog-prompt.html

tcslog.html: tcslog.rst
	rst2html tcslog.rst tcslog.html

design.html: design.rst
	rst2html design.rst design.html

tcslog-prompt.html: tcslog-prompt.rst
	rst2html tcslog-prompt.rst tcslog-prompt.html

.PHONY: clean
clean:
	rm -f tcslog.html design.html tcslog-prompt.html

.PHONY: distclean
distclean:
