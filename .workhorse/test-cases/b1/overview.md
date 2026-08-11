# B1 test cases: HTTP/3 failure backoff

Scenarios covering the cooldown an origin earns as HTTP/3 keeps failing against it, and what ends
the run. The cooldown schedule is cheap to exercise at the cache level; the end-to-end paths need a
real HTTP/3 server with a blackholed UDP path.

## Cooldown schedule

- [x] A first failure blocks the origin for `upgradeFailedTtl` (verifies spec: H3UP)
- [x] Each consecutive failure doubles the cooldown: 5, 10, 20, 40 minutes on the defaults (verifies spec: H3UP)
- [x] The cooldown stops doubling at `upgradeFailedMaxTtl` (verifies spec: H3UP)
- [x] `upgradeFailedMaxTtl` at or below `upgradeFailedTtl` gives a flat cooldown that never backs off (verifies spec: H3UP)
- [x] An origin that fails again as soon as its cooldown lapses continues the run rather than restarting it (verifies spec: H3UP)

## Ending the run

- [x] A confirmed HTTP/3 response clears the count, so the next failure blocks for the base cooldown again (verifies spec: H3UP)
- [x] A confirmation racing a live cooldown clears the count without unblocking the origin (verifies spec: H3UP)
- [x] An origin left alone for a further cooldown beyond the one it earned is judged from the base again (verifies spec: H3UP)
- [ ] A background probe that succeeds after a cooldown lapses ends the run the same way a foreground request does (verifies spec: PROBE)

## Failure sources

- [ ] A failed foreground upgrade attempt counts towards the run (verifies spec: H3UP)
- [ ] A failed background probe counts towards the run (verifies spec: PROBE)
- [ ] A run of cancellation strikes reaching `upgradeCancelStrikes` counts as one failure, not several (verifies spec: H3UP)
- [ ] A hinted origin that fails backs off on the same schedule as an advertised one (verifies spec: H3UP)

## Operational

- [ ] An origin behind a permanently blackholed UDP path is retried on a lengthening interval across a long-running agent, not every five minutes
- [ ] Existing HTTP/3 upgrade, probe, and advertised-port behaviour is unaffected on a healthy path
