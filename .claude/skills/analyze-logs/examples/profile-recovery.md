# Example: Recover the intended profile

Question: did request `r1` in session `east` end, and what was its outcome?

The project contains two profiles that recognize the same start/end wording but
disagree about outcomes. One reads the recorded outcome; the other declares every
end successful. The captured end explicitly records failure. This is the synthetic
`ambiguous-rules` case in `evals/discovery.py`.

1. Run the initial bounded investigation with the established project root. Its
   brief reports `profile.status: ambiguous`, generic analysis, and a
   `resolve_profile` action requiring an operation kind and independent facts.
2. Inspect the candidate files and the independently supplied source-addressed
   facts. Match count cannot distinguish their intended semantics.
3. Call `profile resolve` for kind `request`, supplying those facts and candidate
   paths with the same project root. Inspect the selected candidate and validation
   diagnostics. If a saved mapping exists, it must pass current revalidation too.
4. Rerun `investigate` with the justified explicit `--profile` and a fresh artifact.
   Retrieve terminal evidence through the new artifact's checksum-bound request.
5. Report the observed failure ending with its source reference. Failure establishes
   an ending; it does not establish success, a hang, or the underlying cause.

For `no_match` after changing directories, first compare the reported discovery
directory with the established project root. Correct the root before inventing new
rules. If assertions are unavailable, preserve generic facts and the unresolved
profile distinction instead of choosing by label or match count.

This example is exercised by the scripted discovery evaluation; that verifies the
CLI path and scorer, not model adherence.
