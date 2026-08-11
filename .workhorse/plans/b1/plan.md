# B1: exponential backoff on the HTTP/3 failed cooldown

The failed cache currently holds every origin for a flat `upgradeFailedTtl`, so a permanently
blocked UDP path is retried every five minutes forever. Give each origin a consecutive-failure
count, double the cooldown off it, and clear the count on a confirmed HTTP/3 response.

## Notes

The `failed` cache moves from `Cache<String, ()>` to an entry carrying the count and its own
expiry instants, following the pattern `advertised` already uses: the moka TTL is the outer bound
and the per-entry instant is the real one, checked on read. That avoids a per-entry `Expiry` policy
for a cooldown that varies per origin.

The entry outlives its cooldown, because the count has to survive the block it caused or it could
never escalate past one. It is kept for one further cooldown beyond the one it set, so an origin
that re-fails as soon as it is retried escalates while one left alone is judged afresh.

A confirmation zeroes the count but leaves a live cooldown in place. Clearing the whole entry would
let a confirmation racing a concurrent failure unblock the origin the failure just blocked, which
the existing `confirm_h3` contract deliberately avoids.

## Steps

- [x] `FailureEntry` (count, cooldown expiry, count expiry) replacing the unit value in `failed`
- [x] `failure_cooldown()`: base doubled per consecutive failure, capped, cap clamped to the base
- [x] `is_failed()` gate replacing the four `failed.contains_key` call sites
- [x] `record_h3_failure` counts, `confirm_h3` clears the count
- [x] `upgradeFailedMaxTtl` option, default 3600, plumbed to the cache config
- [x] Rust unit tests: schedule, cap, cap clamp, count retention, reset on confirmation
- [x] README option docs
