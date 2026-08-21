# Perception fixtures (negatives set)

Tuning has historically been scored on one question -- "was the cat found?"
-- which cannot detect a regression that adds false positives. CV Mode
Phase 5 §4 starts the other half: a curated set of frames labelled *no
target present*, kept alongside known positives.

## Where

    ~/.local/share/hls-livecam-fixtures/
        negatives/   frames with no person and no cat present
        positives/   frames with a known, human-confirmed subject

**Not in this repo, deliberately.** The fixtures are camera frames of a
private home. They must not be committed here (this repo is public), pasted
into a report, or published in an artifact. The path above is the contract;
the contents are local to each machine.

## Current contents

| set | frames | notes |
|---|---|---|
| `negatives/tanzania-fp-2026-08-21` | 40 raw RGB + 2657 logged detect passes | hanging plaid blanket scored `person` up to 0.910 |

## Why this one matters

The blanket is a genuinely hard negative rather than a pipeline defect: it
is human-height, garment-textured, and has a shoulder-to-leg taper. It is
also stationary, so it reproduces on demand -- which is what makes it usable
as a regression fixture rather than an anecdote.

Any change to detector config, input scale, illumination correction or the
tracker should be scored against it. The number to watch is not "did the
score drop" but "did the count of false tracks drop", because the tracker's
promotion ladder, not the raw score, is what puts a label on screen.
