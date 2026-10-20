O = ../OUT

.PHONY: all
all:	manual.html design.html tcslog-prompt.html

manual.html: manual.rst
	rst2html manual.rst manual.html

design.html: design.rst
	rst2html design.rst design.html

tcslog-prompt.html: tcslog-prompt.rst
	rst2html tcslog-prompt.rst tcslog-prompt.html

.PHONY: clean
clean:
	rm -f manual.html design.html tcslog-prompt.html

.PHONY: distclean
distclean:
