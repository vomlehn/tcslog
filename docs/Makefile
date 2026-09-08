O = ../OUT

all:	tcslog.html tcslog-prompt.html

tcslog.html: tcslog.rst
	rst2html tcslog.rst tcslog.html

tcslog-prompt.html: tcslog-prompt.rst
	rst2html tcslog-prompt.rst tcslog-prompt.html

.PHONY: clean
clean:
	rm -f tcslog.html tcslog-prompt.html

.PHONY: distclean
distclean:
	rm -f tcslog.rst
