# Dependency Security Status

Chess Material Studio uses `cargo audit` and the RustSec advisory database to review its locked dependency graph. Not every reported warning represents an exploitable vulnerability in the application's current use, so residual warnings are recorded here with their root cause, current assessment, and exit condition. This is a point-in-time record: review it whenever `Cargo.lock` or a principal dependency changes.

## Current baseline

- **Review date:** 2026-09-17
- **Reference commit:** `bec03f1daad4e7ac675e9706a89c41b9ac15f537`
- **`cargo audit`:** 7 allowed warnings
- **Yanked warnings:** none currently reported after updating `chacha20` to 0.10.2

## Summary

| Advisory | Crate | Warning | Source / root cause | Current assessment | Exit condition |
| --- | --- | --- | --- | --- | --- |
| RUSTSEC-2020-0036 | `failure` 0.1.8 | unmaintained | `chess` 3.2.0 | Maintenance warning from the rules backend dependency. | A maintained `chess` release, viable fork, or rules-backend migration removes `failure`. |
| RUSTSEC-2019-0036 | `failure` 0.1.8 | unsound | `chess` 3.2.0 | No exploitable path was identified in the audited CMS usage; the affected crate remains unmaintained debt. | A maintained `chess` release, viable fork, or rules-backend migration removes `failure`. |
| RUSTSEC-2026-0097 | `rand` 0.7.3 | unsound | Build dependency of `chess` 3.2.0 | The advisory conditions were not observed in the current dependency configuration. | A maintained `chess` release, viable fork, or rules-backend migration removes `rand` 0.7. |
| RUSTSEC-2024-0436 | `paste` 1.0.15 | unmaintained | Apple Metal path: `metal` -> `wgpu-hal` -> WGPU/Iced | Maintenance warning; no runtime vulnerability is asserted by this advisory. | An Iced, WGPU, or Metal update removes `paste`. |
| RUSTSEC-2026-0206 | `rustybuzz` 0.20.1 | unmaintained | `usvg`/`resvg` in the Iced rendering stack | Maintenance warning; upstream identifies `harfrust`, but it is not a direct replacement in this dependency path. | An `usvg`, `resvg`, or Iced update migrates away from `rustybuzz`. |
| RUSTSEC-2026-0192 | `ttf-parser` 0.25.1 | unmaintained | Direct PDF export dependency and transitive Iced/font-rendering paths | Maintenance warning; replacing only the direct use would leave transitive paths. | Upstream removes the transitive paths, or a specific PDF-font migration addresses the direct use. |
| RUSTSEC-2026-0253 | `lru` 0.16.4 | unsound | `cryoglyph` 0.1.0 -> `iced_wgpu` | No exploitable path was identified in the audited Cryoglyph usage; its version constraint prevents an isolated update. | Cryoglyph/Iced accepts a corrected `lru`, or Iced removes this route. |

## Root-cause groups

### `chess` 3.2.0

`chess` 3.2.0 brings in both `failure` advisories and `rand` 0.7.3. No compatible published update that removes these dependencies was identified. Resolving this group requires an evaluated `chess` update, a maintained fork, or a rules-backend migration; it must not be made as an incidental dependency change.

`failure`/RUSTSEC-2019-0036 requires a problematic `Fail::__private_get_type_id__` implementation. `chess` uses `derive(Fail)`, while CMS does not implement `Fail` directly; no exploitable path was identified in the audited CMS usage. `rand` 0.7.3 is used only while building `chess`; the advisory conditions were not observed in the current configuration. CMS's direct `rand` dependency is 0.10.2.

### Iced / rendering stack

`paste`, `rustybuzz`, and `lru` are transitive rendering-stack dependencies. `paste` is reachable through the Apple Metal backend. `rustybuzz` is provided through `usvg`/`resvg`, and `lru` is constrained to the 0.16 series by `cryoglyph` 0.1.0. The transitive `ttf-parser` routes also belong to this stack. Resolution depends primarily on upstream changes in Iced, WGPU, Cryoglyph, `usvg`, or `resvg`.

Do not force `lru` 0.18 or later through a patch or fork without a dedicated task: the RustSec fix requires at least 0.18.2, while Cryoglyph currently restricts its accepted version range.

### PDF export

`ttf-parser` is also a direct dependency used by `src/export.rs` to parse the embedded PDF text and chess-symbol fonts. A local migration to `skrifa` requires a dedicated task with PDF export tests. That local migration alone would not remove the Iced/font-rendering transitive routes.

## Advisory notes

### `failure` 0.1.8 — RUSTSEC-2020-0036

- **Path and scope:** `chess` 3.2.0 -> `failure`; rules backend dependency.
- **Why it remains:** no compatible maintained `chess` update was identified.
- **Current context:** maintenance warning; CMS does not use `failure` directly.
- **Reevaluate when:** a maintained `chess` release is published, a viable fork exists, or a rules-backend migration is proposed.

### `failure` 0.1.8 — RUSTSEC-2019-0036

- **Path and scope:** `chess` 3.2.0 -> `failure`; rules backend dependency.
- **Why it remains:** the crate remains required by the current `chess` version.
- **Current context:** the advisory's problematic `Fail::__private_get_type_id__` condition was not observed in CMS's audited use; CMS implements no `Fail` type directly.
- **Reevaluate when:** a maintained `chess` release is published, a viable fork exists, or a rules-backend migration is proposed.

### `rand` 0.7.3 — RUSTSEC-2026-0097

- **Path and scope:** build dependency of `chess` 3.2.0.
- **Why it remains:** no isolated correction exists within the current `chess` 3.2.0 graph.
- **Current context:** the advisory conditions were not observed in the current build configuration; CMS directly uses corrected `rand` 0.10.2.
- **Reevaluate when:** a maintained `chess` release is published, a viable fork exists, or a rules-backend migration is proposed.

### `paste` 1.0.15 — RUSTSEC-2024-0436

- **Path and scope:** `metal` -> `wgpu-hal` -> WGPU/Iced; relevant to the Apple/Metal backend.
- **Why it remains:** this is an upstream rendering-stack dependency.
- **Current context:** maintenance warning; no runtime vulnerability is asserted by this advisory.
- **Reevaluate when:** an Iced, WGPU, or Metal update removes `paste`.

### `rustybuzz` 0.20.1 — RUSTSEC-2026-0206

- **Path and scope:** `usvg` -> `resvg` -> Iced rendering components.
- **Why it remains:** `harfrust` is an upstream-indicated alternative, not a drop-in replacement within the current `usvg`/`resvg` route.
- **Current context:** maintenance warning tracked pending upstream migration.
- **Reevaluate when:** an `usvg`, `resvg`, or Iced update migrates from `rustybuzz`.

### `ttf-parser` 0.25.1 — RUSTSEC-2026-0192

- **Path and scope:** direct CMS PDF export use plus Iced/font-rendering routes.
- **Why it remains:** a local replacement would leave the transitive graph unchanged.
- **Current context:** maintenance warning; the direct PDF use parses embedded font data.
- **Reevaluate when:** upstream removes transitive paths, or a dedicated PDF-font migration with PDF tests is approved.

### `lru` 0.16.4 — RUSTSEC-2026-0253

- **Path and scope:** `cryoglyph` 0.1.0 -> `iced_wgpu` -> Iced renderer.
- **Why it remains:** Cryoglyph restricts `lru` to the 0.16 series, while the RustSec correction requires 0.18.2 or later.
- **Current context:** no exploitable path was identified in the audited Cryoglyph usage; an isolated forced upgrade is not appropriate.
- **Reevaluate when:** Cryoglyph/Iced accepts a corrected `lru`, or an Iced update removes this route.

## Dependabot note

CMS directly uses `rand` 0.10.2. The remaining warning concerns `rand` 0.7.3, which is transitive to `chess` 3.2.0. Dependabot can assess the direct dependency and return `security_update_not_needed`; that result does not mean the transitive warning has disappeared. `cargo audit` remains the verification source for this warning.

## Review policy

Review this document when:

- `cargo audit` output changes;
- `chess` changes;
- Iced, WGPU, or Cryoglyph changes;
- PDF export or font-parsing code changes; or
- before removing any documented exception.
