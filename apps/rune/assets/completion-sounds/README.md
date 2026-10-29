# Hermes completion sound bank

These fourteen WAV files are rendered from the retained Hermes Desktop MIT
synthesis in `apps/desktop/src/lib/completion-sound.ts`. `manifest.json` records
the upstream baseline, exact source and generator hashes, Chromium version,
seed, PCM encoding, names and asset hashes. The accompanying LICENSE.txt
preserves the repository license.

Regenerate from the repository root with:

```sh
node apps/rune/scripts/build-completion-sounds.mjs [path-to-Chromium]
```

The build script uses the original oscillator/envelope/filter/convolution code
and `playVariant` signal path in `OfflineAudioContext`, never a live AudioContext
or audio output device. A fixed random seed gives one reproducible realization
of the original noise and reverb. Assets are stereo 48 kHz signed 16-bit PCM;
trailing sub-half-LSB silence is trimmed with 256 frames retained. Two successive
renders in the recorded browser produced identical hashes for every WAV.

Variant 0 (None) has no asset and never opens the native audio device. Playback
uses Rodio already present in the desktop. The generator has no runtime role.
