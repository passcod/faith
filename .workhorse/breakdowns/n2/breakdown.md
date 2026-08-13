# Direct file downloads

Cards broken out of the direct-file-download work on N2.

## Resume a partial download to file · R2

A caller who already holds part of a file wants to finish it: issue a `Range` request and have Faith append the response body to the existing file rather than truncating it or refusing to touch it.
This needs a third destination mode beyond N2's `overwrite` boolean, a way to name the offset to write from, and a decision about what happens when the server ignores the `Range` header and sends the whole body.
