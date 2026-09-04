"""Find a narrowband tone ("beep") in a captured WAV.

A beep is not just loud, it is NARROW and PROMINENT: energy concentrated in
one or two FFT bins that stands well above the spectrum either side of it.
Broadband room noise is loud too and would win a plain "strongest bin" search,
so prominence-above-local-median is the test, not absolute level.

Also reports how much of the recording each candidate is present in, which
separates a continuous whine from an intermittent beep, and checks whether the
strongest candidate has harmonics (a buzzer or alarm usually does; a pure
digital artifact usually does not).
"""
import sys
import wave

import numpy as np

path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/beep.wav"
w = wave.open(path)
rate = w.getframerate()
n = w.getnframes()
x = np.frombuffer(w.readframes(n), dtype="<i2").astype(np.float64) / 32768.0
print("analysed %.1fs at %d Hz" % (len(x) / rate, rate))

N = 8192
hop = 2048
win = np.hanning(N)
frames = 1 + (len(x) - N) // hop
if frames < 4:
    print("too short")
    sys.exit(1)

spec = np.empty((frames, N // 2), dtype=np.float64)
for i in range(frames):
    seg = x[i * hop: i * hop + N] * win
    spec[i] = np.abs(np.fft.rfft(seg)[: N // 2])

freqs = np.fft.rfftfreq(N, 1.0 / rate)[: N // 2]
eps = 1e-12
db = 20 * np.log10(spec + eps)

# Prominence: how far each bin stands above the local spectral background,
# measured per frame with a wide median filter, then summarised over time.
k = 61  # ~360 Hz of context either side at this resolution
pad = k // 2
prom = np.empty_like(db)
for i in range(frames):
    padded = np.pad(db[i], pad, mode="edge")
    local = np.array([np.median(padded[j: j + k]) for j in range(len(db[i]))])
    prom[i] = db[i] - local

peak_prom = np.percentile(prom, 95, axis=0)   # strong when present
presence = (prom > 10).mean(axis=0) * 100      # % of frames it stands out in

lo = np.searchsorted(freqs, 150)               # ignore rumble
order = np.argsort(peak_prom[lo:])[::-1] + lo

print("\ncandidate tones, by prominence above the local spectrum:")
print("   freq Hz   prominence   present   mean level")
seen = []
for idx in order:
    f = freqs[idx]
    if any(abs(f - s) < 40 for s in seen):
        continue
    seen.append(f)
    print("  %8.1f   %7.1f dB   %5.1f%%   %7.1f dB"
          % (f, peak_prom[idx], presence[idx], db[:, idx].mean()))
    if len(seen) >= 6:
        break

if seen:
    f0 = seen[0]
    print("\nstrongest candidate %.1f Hz -- harmonic check:" % f0)
    for h in (2, 3, 4):
        t = f0 * h
        if t >= freqs[-1]:
            break
        j = int(np.argmin(np.abs(freqs - t)))
        print("  %dx = %7.1f Hz  prominence %5.1f dB %s"
              % (h, t, peak_prom[j],
                 "<- present" if peak_prom[j] > 8 else ""))
