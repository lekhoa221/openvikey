# ADR 0010: Windows product is standalone by default

- **Date:** 2026-08-19
- **Status:** Accepted
- **Supersedes:** ADR 0007 as the default Windows product direction

## Context

ADR 0007 selected a hybrid hook + TSF roadmap. GĐ2a produced a runnable hook host; GĐ2b added a registered read-only TSF profile for password/InputScope and surrounding context. During live validation, the owner clarified the intended product UX: one application like UniKey, not a keyboard profile registered with Windows or selected through `Win + Space`.

The repository already has the important standalone components in `openvikey-win`: low-level hooks, `SendInput`, tray, overlay, session learning and persistence. TSF registration is therefore not required for the primary typing/learning path and creates an unwanted OS-visible product model.

## Decision

1. The default Windows product is one background `OpenViKey.exe` with tray, hooks, context safety, injection, learning, settings and persistence.
2. Running the default product does not register COM/TSF components, modify the Windows language list, require `Win + Space`, or require Administrator privileges.
3. GĐ2c TSF-primary is removed from the release critical path.
4. `openvikey-win-tsf` remains research/optional compatibility code and is not packaged or enabled by default.
5. Password detection moves to the standalone host using process deny rules, Win32 password styles and UI Automation `CurrentIsPassword`, with identity-bound asynchronous caching.
6. Missing TSF context is normal. It must not disable typing. External surrounding text is optional; the session's own composition/commit context remains the baseline.
7. Product UI and observable learning move ahead of any optional TSF work.
8. A clean-profile test with no OpenViKey TSF/COM/language registration is a release gate.

## Consequences

- The existing GĐ2b implementation remains useful evidence and optional code, but it no longer defines the product startup or installation model.
- The current working tree's fail-closed dependency on a TSF projection must be removed before standalone typing can be accepted.
- Host-side password classification must handle field focus within one top-level window and must never read text before sensitive classification.
- Some inaccessible/custom fields will pass through by default until explicitly opted in; safety takes precedence over claiming universal compatibility.
- Settings, learned-rule visibility, single-instance behavior, autostart and packaging become near-term product work.
- Production corpus, protected persistence, installer and signing remain separate release gates.

## Governing spec

[`docs/superpowers/specs/2026-08-19-openvikey-windows-standalone-design.md`](../superpowers/specs/2026-08-19-openvikey-windows-standalone-design.md)
