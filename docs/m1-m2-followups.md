# M1 + M2 follow-ups

Findings deferred during the M1 and M2 reviews (none blocks the merge). Triage for M3/M4.

## M1 (telemetry)
- Task 1: partial .part left on curl failure; curl --retry doesn't cover connection errors (--retry-all-errors, timeouts); cache hit skips checksum (run script unconditionally); guard --exclude=ci.yml too broad; README licence says Apache-2.0 only (upstream dual Apache/MIT).
- Task 2: no-metadata test ignores REQUIRE env; handler_name fallback untested; first gpmd stream only (undocumented); REQUIRE env any value incl. 0; fdsc zero-duration stderr noise.
- Task 3: pace NaN/negative → +inf undocumented/untested; PaceNm/Mps2/Percent/Deg/None conversions and most symbols untested; ALL_UNITS hand-maintained; Unit::None naming; Dimensionless doc confusing; metric id strings to verify vs original in Task 10.
- Task 4: MAX_DEPTH doc off by one; complex TYPE array syntax f[4] unsupported and TYPE must be trimmed by callers; text() joins multi-row c items; utc() repeat==1 only; test gaps (depth boundary, struct_size 0 with big repeat, BadStructSize simple type, numbers() on nested, unknown char in TYPE); apply_scale partial mutation on error (unreachable).
- Task 4: no test for null key inside nested body (per-level by construction).
- Task 5: GPS9 chosen per file not per packet (lossy edge); dedupe test coverage thin.
- Task 5: totals count streams not payloads (double STRM overstates); Tally keeps first reason only; no multi-kind ordering test.
- Task 6: SES zero-first-sample differs from original (doc claims parity; test sequence not a parity check); NaN propagates in filters; NaN dop/speed stays locked (use !(x <= max)); missing tests fix=1, DOP+heuristic, speed_max+heuristic, non-finite.
- Task 7: geodesic computed twice per 3D pair; NaN lon / non-finite t untested.
- Task 8: overlapping samples covered() vs Stale mismatch (unreachable if end = next start).
- Task 9: module doc line missing in telemetry.rs; available array rebuilt per sample (cheap); 2D point with NaN alt dropped from track; duration = last GPMF packet end, not video duration (M2 should know: coverage excludes tail past last packet).
- Task 10: lock state not cross-checked vs gps_fix in dashboard test; accel/grav magnitude-only; reproducibility (transitive deps, ffmpeg version, venv commands); provenance commit/sha in README; REQUIRE env empty/0.
- Task 11: empty-string sentinel for broken pipe; README "same derived values" claim stronger than evidence; gps-lock/gps-dop CSV cells.
- Task 11: serde_json dev-dep not via workspace deps; non-finite→null path untested; weak "file not read" assertion.
M1 minor (deferred, post-final): plan line 31 coverage note stale; QuietLog overlapping guards can leave FFmpeg log at Error permanently (benign: quieter logs); add_seconds checked None branch untested; InvalidData bound untested; CLI doesn't pass video_duration; Availability doesn't expose its timeline.

## M2 (layout, render, overlay)
- Task 2: anchor test derives expectations from fractions() (add literal checks for remaining anchors); Aspect::parse looser than schema pattern; scale_factor non-16:9 design aspect untested / width 0 guard; Aspect serialize non-canonical.
- Task 3: doc comments on Piece/FormatError/parse; unbounded output length for huge values.
- Task 4: unknown theme fields dropped (decide in Task 5 preserve-unknown); NaN/negative sizes sanitised in Task 6 validation; doc comments/units on defaults; unused defaults constants must be consumed by Tasks 5-8.
- Task 5: ints saved as floats / f32 rounding (goldens & M4 diffs should expect); key order not preserved (M4: preserve_order); unknown enum values reject whole layout (spec §6.4 "if possible") → M4/M3 decision; border group not pruned when empty (fold into Task 6); no test with unknown key inside a widget's style group; #[expect] vs #[allow].
- Task 6: non-atomic save (M4); error position lost in rebuilt error; aspect branch untested; additionalProperties:true can't catch key typos (doc).
- Task 7: metrics-derived tolerance; weight not checked through layout(); missing-glyph counter in path_at; reshaping per call (shape-run-cache if bench needs); font bytes copied per new(); icon square halo; provenance SOURCE files next to assets; THIRD_PARTY_LICENSES (spec §9) → release milestone M7; duplicate smol_str.
- Task 8: Zone::System DST untested; Fit mode / frame fill/border/radius untested; slow ink test in debug; NaN opacity in code-built layouts; cache thrash on alternating sizes (note for Tasks 10/12); FormatCache re-warn after clear; per-frame allocations (bench).
- Task 9: determinism test doesn't cover warm vs cold cache; bench not guarded vs debug builds.
- Task 10: Prefs::load swallows non-NotFound read errors; fixed tmp name race between instances; spawn expect on thread creation.
- Task 11: late catch-up branch untested; failed allocation consumes request; pixmap alloc under lock; resize window throttles seeks; layout-change starvation (M4 editor); spawn expect.
- Task 11: injected-panic test stderr noise; persistent panic logs up to 25/s (rate-limit).
- Task 12: stale overlay 1-2 frames after O re-enable; old pending request survives set_telemetry(None); 1 px overlay size mismatch softens text (fit_rect not pixel-snapped); cancelled telemetry loads run in full; sRGB targets blend in linear light (document).
- Final fix wave M2: re-review clean (commits 8ec4e7b..84beabe). Minor (deferred): one sized node disables aspect fallback; no custom-layout collision tests; None→Some race re-queues one old render. M2 COMPLETE.
