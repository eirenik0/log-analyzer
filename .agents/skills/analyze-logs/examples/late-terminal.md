# Example: An ending beyond the first page

The synthetic `late-terminal` discovery case contains a start, an intermediate
error, progress records, and a successful end at physical line 8. The first
five-record page cannot establish whether the operation ended.

1. Inspect processing coverage and per-goal support in the investigation brief.
   Complete processing means the retained artifact can be searched; it does not
   mean the first displayed page includes the final event.
2. Retrieve `/records` using the supplied literal arguments. The first response
   advertises remaining items and a cursor; retain the checksum and collection.
3. Continue with that exact cursor. Verify the terminal event's kind, full ID,
   session scope and occurrence. Retrieve the matching lifecycle finding and both
   source boundaries before reporting a duration.
4. Report the observed successful ending, preserving the earlier error as contrary
   evidence. A later success does not itself establish a retry or erase the error.

If processing stopped before the end, pagination cannot recover it. Report the
observed partial facts and the unprocessed extent, then change limits only within
the user's budget. Never convert an omitted page or processing cutoff into a hang.
