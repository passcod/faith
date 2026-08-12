# Follow-ups from the per-request timing breakdown

The timing breakdown ships with the phases reachable from the request boundary.
The phases below need instrumentation that reqwest does not expose today, so they are carved out as their own work.

## Add a request-sent timestamp to the timing breakdown

Report the moment the request head and body have been fully written to the connection, as a `requestSent` field on the response's timing breakdown, splitting the current single `fetchStart` to `responseStart` span into time spent sending and time spent waiting on the server.
reqwest gives no signal for a request body having been fully written, so this needs the outgoing body wrapped to observe its final poll, and a decision on what to report for requests whose body is empty or not constructed by Fáith.

## Break connection setup into DNS, connect, and TLS phases

Report `domainLookupStart`, `domainLookupEnd`, `connectStart`, `connectEnd`, and `secureConnectionStart` so the connection acquisition span can be attributed to a phase.
These boundaries are not reachable without hooks upstream in reqwest or hyper, so this likely starts as an upstream contribution.
