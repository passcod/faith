# Request body compression

E2 lands the option itself: a coding to compress in, a level Faith picks, and the headers that describe the result. Two knobs were deliberately left for later, each worth its own card.

## Tune request compression level and parameters · D3

`compress` takes a coding and nothing else, so every request in a given coding compresses at the one level Faith picked. A caller trading CPU against bytes on the wire, or reaching for a coding's own parameters (a zstd window, a brotli quality), has no way to say so. Widen the option to carry them.

## Flush the request compressor per chunk · E3

A `ReadableStream` body is compressed as its chunks arrive, but the compressor buffers on its own terms, so the bytes for a chunk the caller writes may not leave until later chunks fill it. That stalls the message exchange REQ describes, where a caller drives the request body from what it reads off the response body. Give the caller a way to have each write flushed through the compressor and out.
