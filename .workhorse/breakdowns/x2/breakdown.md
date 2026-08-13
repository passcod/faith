# X2 breakdown

Cards spawned from the work on disallowing streaming request bodies over HTTP/1.1.

## Move existing standards-deviating options into the quirks group · Y2

The `quirks` group introduced by X2 is the home for switches that turn off a rule Faith otherwise upholds, but it starts with only `h1RequestStreaming` in it while options of the same shape sit elsewhere in the option surface. Audit the agent options for the ones that qualify and move them in, so the group is the single place a caller looks to see what standard behaviour an agent has been opted out of. `http3.upgradeFollowAdvertisedPort` is the clear candidate: it is described as non-standards-compliant and exists for servers the caller controls, which is the quirks contract exactly. The audit decides the rest; a switch that merely tightens behaviour or trades off security within what a standard permits, such as `tls.required` or `tls.earlyData`, is not a quirk. Renaming the options that move is a breaking change to the agent option surface, which is accepted.
