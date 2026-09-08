# Configuration variables for tcslog. The TCSLOG_CONFIG is expanded on the
# command line to define the configuration within the code. For example, set
# the timer resolution to one microsecond, i.e. 1000 nanoseconds:
#
# TCSLOG_CONFIG = TIMER_RESOLUTION=1000

# Dell XPS 17 laptop computer: the clock_getres() value will apply to pretty
# much all modern x86 processors.
TCSLOG_CONFIG = TIMER_RESOLUTION=1
