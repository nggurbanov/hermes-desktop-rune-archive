These small assets contain only mathematically generated stereo sine tones.
The project-authored generator and manifest record exact commands, source
parameters, ffmpeg version and resulting hashes. They test actual compressed
container/codec decoding. They are included only in Rust test builds.

Regenerate offline with `python3 apps/rune/scripts/build-audio-test-fixtures.py`.
The application ships native Rust/Sonic/libopus decoding and never invokes or
requires ffmpeg. No user recordings or external copyrighted audio are included.
