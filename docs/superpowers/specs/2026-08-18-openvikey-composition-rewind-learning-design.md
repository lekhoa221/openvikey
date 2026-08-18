# OpenViKey — Composition rewind learning (lát 1+2)

- **Date:** 2026-08-18
- **Status:** Implemented
- **Scope:** `openvikey-core`, `openvikey-session`, `openvikey-win`, `openvikey-lab`
- **ADRs:** [0008](../../decisions/0008-open-development-persistence.md), [0009](../../decisions/0009-personal-source-and-restore.md)

This spec describes the behavior that is in the tree. It does not copy the earlier frictionless-learning proposal.

## Goal

Learn from a natural rewrite inside one composition session (`x3uong| → x| → xưởng| + Space`): last peak `raw_keys` / normalized text is KEY, committed text at Space is VALUE. Space may auto-apply a unique TelexFix reconstruction when policy is confident. Immediate Backspace after that auto restores composition. Electron may learn.

## Non-goals

- Probation Auto (mass-18 TelexFix is unchanged; policy auto is a separate gate).
- Boundary Assist for Fuzzy / Abbreviation / Personal (Personal is Suggest-only).
- New `CaptureRecord` variants; rewind reconstructs from `Input`.
- `MODEL_VERSION` bump; personal store uses `#[serde(default)]`.
- Fields on `InputContext`; intervention lives on the session as `InterventionConfig`.
- Settings GUI.

## Learning unit

One composition session: empty buffer → Boundary. First word of a sentence uses `left_token_nfc = None`. Overlay is not KEY. Backspace is not Reject.

Multiple rewinds in one word keep **last peak only**. If rewind empties the buffer and Space commits empty, the miner cancels and does not attach to the next word. If rewind empties the buffer and the user types again without an empty commit, that is still the same session and the last peak stands.

Caret move, focus change, paste/`InsertText`, and a 10s timeout cancel rewind. Leftover committed prefix (`thu` remaining + a new unit) does not learn.

## Composition rewind miner

State machine in `openvikey-core` (`CompositionRewindMiner`), timestamps supplied by the caller:

```text
Idle --Backspace when composition nonempty--> Rewinding { peak }
Rewinding --more Backspace--> keep peak
Rewinding --Key--> Typing
Typing --Key--> Typing
Typing --Backspace--> Rewinding with a new peak
* --Space/Boundary--> Evaluate(peak, VALUE) then Idle
* --caret/focus/paste/timeout 10s / empty commit--> Cancel
```

Session snapshots the peak **before** `engine.process` (current composition + current overlay candidates). After process, the buffer has already shrunk.

VALUE is the committed token after Auto if Auto fired. Match is `candidate.text == VALUE` on **peak** candidates. `RuleContextKey.original_nfc` is peak normalized text, not raw keys.

- Match → `ImplicitCorrection` (+1.5 mass).
- No match → personal count `(original_nfc, replacement_nfc, input_method)`; promote at **k=2**.

Miner state is not persisted in `SessionSaveSnapshot`.

## Query-time left-token backoff

Evidence is still stored on the exact `RuleContextKey`. `positive_mass` / `negative_mass` / `confidence` **sum** siblings that share `(input_method, source, original_nfc, candidate_nfc, source_rule_id)` and ignore `left_token`. `state` is the **max** `DecisionState` in that group. Payload is not rewritten.

## Personal store

`AdaptiveModel.personal` is `#[serde(default)]`. Counts stay until `k >= 2`, then a row is promoted (cap 512 pairs). `PersonalGenerator` is a cloned table, ids `5_000_000 + idx`, `CandidateSource::Personal`, `max_action = Suggest`. Password / `allow_learning=false` does not write.

## Electron learning and weak signals

`allow_learning_for_foreground`: Viet mode, not a real terminal exe, not denylist. **Win32 profile is not required.** Chrome / Discord / Slack learn. Cursor / VS Code still need `--allow_terminal` to transform; if transform is on, learning is on. Terminal with `--allow_terminal` transforms but does not learn.

`ImplicitCorrection` mass is **+1.5**. `AutoSettled` needs 10 following events **and** ≥ 3s since the auto edit. Settlement mass per rule is capped at 24 × 0.3 = 7.2.

## TelexFix policy auto

`InterventionConfig` is set by the host on focus:

| Surface | `telex_fix_policy_auto` | Delimiters |
| --- | --- | --- |
| Win32 | true | Space and `. , ; : ? !` (not Enter) |
| Electron | true | Space only |
| Terminal / denylist | false | n/a |

Gate at commit (skips the mass-18 Auto path):

- policy auto enabled
- top candidate is TelexFix **or** merged evidence still contains `telex-fix:` / `vni-fix:`
- exactly one such candidate after dedupe
- original is not in the lexicon; replacement is
- `allow_transform`

Emits `ReplaceRange` and records the auto edit for undo. Does not `record_decision(Auto)` from mass and does not add Accept at Space.

Undo:

- Immediate Backspace while composition is empty and pending restore is live: inverse the injected token, `Engine::restore_raw_keys`, **no** `Undo +1.5`.
- Extra Space or a new word after the delimiter clears pending restore.
- `Ctrl+Shift+Z` remains semantic text undo (may record Undo). Same `edit_id` is not given feedback twice.

## Forget last rule

`HostHotkey::ForgetLastRule` = `Ctrl+Shift+.` (`vk 0xBE` + control + shift). Drops evidence or the personal pair of the latest intervention / implicit / personal learn in this session. Lab model dump still works.

## Hook path

Planner / detector still must not I/O, sleep, or `Mutex::lock` on the LL hook. Learning and persist notify stay after `try_lock`.
