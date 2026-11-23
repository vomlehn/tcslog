O = ../OUT

.PHONY: all
all:	index.html manual.html design.html tcslog-prompt.html

# The page the site gives at docs/: what each document here is, made from
# README.rst, which GitHub shows when the directory is browsed.
index.html: README.rst
	rst2html README.rst index.html

manual.html: manual.rst
	rst2html manual.rst manual.html

design.html: design.rst
	rst2html design.rst design.html

tcslog-prompt.html: tcslog-prompt.rst
	rst2html tcslog-prompt.rst tcslog-prompt.html

.PHONY: clean
clean:
	rm -f index.html manual.html design.html tcslog-prompt.html

.PHONY: distclean
distclean:
