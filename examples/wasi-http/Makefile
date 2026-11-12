# The dependency tree under wit/deps is fetched, not hand-copied: `make fetch`
# restores exactly what wkg.lock names, so nothing here is a judgement call
# about which upstream files are needed. There is no build -- the gate refuses
# this world (see README.md), so there is nothing yet to compile or run.

.PHONY: all check-wit fetch update clean
all: check-wit

# The one thing that can be checked without a network: that what is on disk
# resolves, and which packages at which versions it resolves to. Derived from
# the resolution rather than echoed, so a dependency silently moving shows up
# as a changed line. `checks/smoke.py` holds this output to the letter.
check-wit:
	@wasm-tools component wit wit \
	  | sed -n 's/^package \([^ ;{]*\).*/vendored: \1/p' \
	  | sort

# Needs a network, so it is never run by a check. `fetch` restores the
# versions wkg.lock pins; `update` is the deliberate act of moving them, and
# rewrites the lock.
fetch:
	wkg wit fetch

update:
	wkg wit update

clean:
	rm -rf wit/deps
