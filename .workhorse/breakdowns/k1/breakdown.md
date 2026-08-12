# Follow-ups from the per-request timing breakdown

The timing breakdown carries every field of its shape, with phases Fáith cannot yet observe reading 0.
The phases below need instrumentation that reqwest does not expose today, so filling each one in is carved out as its own work.

## Populate the request-sent timestamp

Give `requestSent` a real value instead of 0, splitting the `fetchStart` to `responseStart` span into time spent sending and time spent waiting on the server.
reqwest gives no signal for a request body having been fully written, so this needs the outgoing body wrapped to observe its final poll, and a decision on what to report for requests whose body is empty or not constructed by Fáith.

## Populate the DNS, connect, and TLS phases

Report `domainLookupStart`, `domainLookupEnd`, `connectStart`, `connectEnd`, and `secureConnectionStart` so the connection acquisition span can be attributed to a phase.
These boundaries are not reachable without hooks upstream in reqwest or hyper, so this likely starts as an upstream contribution.
