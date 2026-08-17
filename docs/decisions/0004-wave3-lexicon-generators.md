# ADR 0004: Wave 3 bounded lexicon generators

- **Date:** 2026-08-17
- **Status:** Accepted
- **Gate:** Wave 3 (7B fuzzy, 7C diacritics)

## Context

Wave 3 must correct local typing mistakes and suggest per-token Vietnamese diacritics without adding a model dependency to generators, a network/runtime model, phrase-level delayed edits, or unbounded candidate work. Real fast VNI input can combine transpositions, duplicate/adjacent keys, and misplaced numeric tone/modifier keys in one token.

## Decisions

1. `FuzzyGenerator` scans the injected versioned lexicon and uses bounded weighted Damerau-Levenshtein distance. Adjacent-key substitution, transposition, and insertion/deletion cost less than unrelated substitution.
2. Fuzzy candidates must be explicit lexicon entries, contain a supported syllable/token shape, stay within the configured weighted-distance threshold, and are truncated to caller-provided `max_candidates` with deterministic score/NFC ordering.
3. Plain unaccented exact-fold matches are left to `DiacriticsGenerator`. Fuzzy may consider an exact-fold candidate when malformed VNI digits or an already-accented typo provide evidence.
4. Numeric VNI signals are not discarded: tone digits `1..5` and modifier digits `6..9` are compared with the candidate's Unicode combining features and outrank unigram frequency. This covers mixed errors such as `paht1 → phát`, `htong61 → thống`, and `ma674u → mẫu`.
5. Fuzzy may use the existing left-token bigram as a bounded ranking bonus. This resolves context-sensitive noisy forms such as `hẳn nal2 → hẳn là` without phrase-level beam search.
6. Explicit Latin lexicon entries remain eligible for mixed Vietnamese text (for example `proimtp → prompt`); arbitrary non-lexicon output is impossible.
7. `DiacriticsGenerator` accepts one ASCII alphabetic token, groups lexicon entries by accent-folded form, excludes the unchanged spelling, and returns caller-bounded top-k candidates ranked by normalized unigram frequency plus left bigram.
8. Diacritics remains `CandidateSource::Diacritics`, so the Wave 0 action cap forces `Suggest` even after promotion evidence. Accented input and whitespace/phrase input produce no diacritics candidates.
9. Both generators remain pure: they receive only `CompositionSnapshot`, `LeftContext`, and immutable `Lexicon`; personal evidence remains exclusively in rank/decision.

## Consequences

- User-provided noisy VNI examples are executable regression tests rather than calibration/training data.
- Wave 4 must benchmark full-size lexicon scans against the 15 ms candidate-generation gate and fit/version score calibration only on the calibration split.
- Whole-phrase restoration, delayed beam search, and transformer/LLM inference remain out of scope for v1.
