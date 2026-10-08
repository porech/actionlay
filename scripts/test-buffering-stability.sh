#!/usr/bin/env bash
# Exercise the previously intermittent regressions repeatedly, without retries.
set -euo pipefail
for iteration in 1 2 3; do
  echo "Buffering regression pass $iteration"
  for test in cached_video_keeps_decoding_during_a_slow_source_read \
    source_underrun_suspends_before_callback_silence_and_resumes_with_ready_samples \
    underrun_freezes_both_clocks_and_pause_cancels_autoresume; do
    cargo test -p actionlay-media --lib "player::tests::$test" -- --exact --test-threads=1
  done
done
