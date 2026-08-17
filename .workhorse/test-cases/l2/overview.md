# Populate server timing

Scenarios verifying that a response's timing entry carries the origin's `Server-Timing` metrics.
The parse is exercised through a real origin echoing the header value, so each case states the header an origin sent.

## Reading the header

- [x] A header with several metrics reads as entries in header order, each carrying its name, `dur`, and `desc` (verifies spec: RESP)
- [x] A response without the header reads an empty list (verifies spec: RESP)
- [x] Metrics spread across repeated `Server-Timing` header lines all appear (verifies spec: RESP)
- [x] A metric name reported more than once yields one entry per occurrence (verifies spec: RESP)
- [x] A quoted description containing a comma and a semicolon reads whole, and the metric after it still parses (verifies spec: RESP)

## Malformed and partial metrics

- [x] A metric with no `dur` reads a duration of 0, and one with no `desc` an empty description (verifies spec: RESP)
- [x] A `dur` that is not a number reads 0, and one trailing junk after a number reads that number (verifies spec: RESP)
- [x] A parameter given twice counts as the first of the two, and the parameters after it are still read (verifies spec: RESP)
- [x] A metric with no name is dropped and the rest of the list stands (verifies spec: RESP)
- [x] A `Server-Timing` value that is not valid UTF-8 leaves the list empty (verifies spec: RESP)

## Alongside the rest of the entry

- [x] The metrics survive `JSON.stringify` of the timing entry (verifies spec: RESP)
- [x] A response served by the HTTP cache reports the metrics its stored headers carry (verifies spec: RESP)
