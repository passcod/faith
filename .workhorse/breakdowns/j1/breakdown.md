# Serve stale DNS entries while revalidating

Work spun out of this card rather than done in it.

## Restore the DNS comparison in the features benchmark · W2

The `dns:hickory` and `dns:system` rows in the features suite both request `localhost`, which the configurable-transports work made an always-exempt name handed to the system resolver whatever `dns.servers` says. Both rows therefore resolve through the same system resolver and the comparison measures nothing, which is live on main today because that work did not touch `bench/`. Restoring it means benching a name that is not exempt and pointing Faith at a nameserver the harness controls, which also closes the gap that the harness can delay an HTTP response but not a DNS answer, so no row can currently show a slow resolver at all. Separate from serving stale because the rows are wrong on their own terms and the fix stands whether or not stale serving ships, though this card depends on it for a row that means anything. Carries decisions about whether the harness runs its own nameserver or reuses the one under `test/lib/`, and what a slow-resolver row holds constant so the DNS cost is the only thing moving.
